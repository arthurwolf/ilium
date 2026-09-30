//! The `wgpu` implementation of [`GpuRunner`] and the adapter probe.
//!
//! Binding contract for every kernel (group 0):
//! * binding 0, storage read-write: `array<f32>` of `width * height` dot intensities;
//! * binding 1, uniform: a 16-byte header `{width: u32, height: u32, 0, 0}` followed
//!   by the job's `f32` uniforms padded with zeros to a multiple of 16 bytes.
//!
//! Kernels use `@workgroup_size(8, 8, 1)`; the dispatch covers `width x height`
//! rounded up, so a kernel must bounds-check against the header.

use crate::diagnose::{diagnose, preferred_adapter_index};
use crate::facts::{
    gather_system_facts, AdapterBackend, AdapterDeviceType, AdapterFact, ProbeFacts,
};
use crate::shaders::shader_source;
use ilium_ambient::gpu::{GpuAvailability, GpuJob, GpuKernel, GpuRunner};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::Duration;

/// GL is deliberately ignored: only native, compute-capable APIs count.
const PROBED_BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN
    .union(wgpu::Backends::METAL)
    .union(wgpu::Backends::DX12);
const WORKGROUP_SIZE: u32 = 8;
const HEADER_BYTES: usize = 16;
const GPU_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of the probe: the availability to publish and, when ready, the runner.
pub(crate) struct ProbeOutcome {
    pub availability: GpuAvailability,
    pub runner: Option<Arc<WgpuRunner>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PipelineKey {
    Kernel(GpuKernel),
    #[cfg(test)]
    SelfTest,
}

struct SizedBuffers {
    output_bytes: u64,
    uniform_bytes: u64,
    output: wgpu::Buffer,
    uniform: wgpu::Buffer,
    staging: wgpu::Buffer,
}

struct Pipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

struct RunnerState {
    pipelines: HashMap<PipelineKey, Pipeline>,
    buffers: Option<SizedBuffers>,
    bind_group: Option<(PipelineKey, wgpu::BindGroup)>,
}

/// Runs kernels on one wgpu device. All state is behind a mutex; `run` is
/// blocking and only called from the frame worker thread.
pub(crate) struct WgpuRunner {
    adapter_name: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    state: Mutex<RunnerState>,
}

fn convert_device_type(device_type: wgpu::DeviceType) -> AdapterDeviceType {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => AdapterDeviceType::Discrete,
        wgpu::DeviceType::IntegratedGpu => AdapterDeviceType::Integrated,
        wgpu::DeviceType::VirtualGpu => AdapterDeviceType::Virtual,
        wgpu::DeviceType::Cpu => AdapterDeviceType::Cpu,
        wgpu::DeviceType::Other => AdapterDeviceType::Other,
    }
}

fn convert_backend(backend: wgpu::Backend) -> AdapterBackend {
    match backend {
        wgpu::Backend::Vulkan => AdapterBackend::Vulkan,
        wgpu::Backend::Metal => AdapterBackend::Metal,
        wgpu::Backend::Dx12 => AdapterBackend::Dx12,
        _ => AdapterBackend::Other,
    }
}

/// Enumerates adapters, creates a device on the preferred hardware adapter and
/// classifies the result. Blocking; runs on the probe thread (and in tests).
pub(crate) fn probe() -> ProbeOutcome {
    let mut facts: ProbeFacts = gather_system_facts();
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = PROBED_BACKENDS;
    let instance = wgpu::Instance::new(descriptor);
    let adapters = pollster::block_on(instance.enumerate_adapters(PROBED_BACKENDS));
    facts.adapters = adapters
        .iter()
        .map(|adapter| {
            let info = adapter.get_info();
            AdapterFact {
                name: info.name,
                device_type: convert_device_type(info.device_type),
                backend: convert_backend(info.backend),
            }
        })
        .collect();

    let mut runner: Option<Arc<WgpuRunner>> = None;
    if let Some(index) = preferred_adapter_index(&facts.adapters) {
        match WgpuRunner::new(&adapters[index], facts.adapters[index].name.clone()) {
            Ok(created) => runner = Some(Arc::new(created)),
            Err(message) => facts.device_error = Some(message),
        }
    }
    let availability = diagnose(&facts);
    if !matches!(availability, GpuAvailability::Ready { .. }) {
        runner = None;
    }
    ProbeOutcome {
        availability,
        runner,
    }
}

fn round_up(value: usize, multiple: usize) -> usize {
    value.div_ceil(multiple) * multiple
}

impl WgpuRunner {
    fn new(adapter: &wgpu::Adapter, adapter_name: String) -> Result<Self, String> {
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("ilium-gpu device"),
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor))
            .map_err(|error| format!("could not create a GPU device on {adapter_name}: {error}"))?;
        Ok(Self {
            adapter_name,
            device,
            queue,
            state: Mutex::new(RunnerState {
                pipelines: HashMap::new(),
                buffers: None,
                bind_group: None,
            }),
        })
    }

    fn lock_state(&self) -> MutexGuard<'_, RunnerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn build_pipeline(&self, key: PipelineKey, source: &str) -> Pipeline {
        let label = format!("ilium-gpu {key:?}");
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(&label),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let bind_group_layout = pipeline.get_bind_group_layout(0);
        Pipeline {
            pipeline,
            bind_group_layout,
        }
    }

    fn create_buffers(&self, output_bytes: u64, uniform_bytes: u64) -> SizedBuffers {
        let make = |label: &str, size: u64, usage: wgpu::BufferUsages| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        SizedBuffers {
            output_bytes,
            uniform_bytes,
            output: make(
                "ilium-gpu output",
                output_bytes,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            ),
            uniform: make(
                "ilium-gpu uniforms",
                uniform_bytes,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            staging: make(
                "ilium-gpu staging",
                output_bytes,
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
        }
    }

    /// Runs `source` (cached under `key`) over `width x height`.
    pub(crate) fn run_source(
        &self,
        key: PipelineKey,
        source: &str,
        job: &GpuJob,
        out: &mut [f32],
    ) -> Result<(), String> {
        let cell_count = (job.width as usize)
            .checked_mul(job.height as usize)
            .filter(|count| *count > 0)
            .ok_or_else(|| "GPU job has an empty or oversized output".to_string())?;
        if out.len() != cell_count {
            return Err(format!(
                "GPU output slice has {} cells, job needs {cell_count}",
                out.len()
            ));
        }
        let output_bytes = (cell_count * 4) as u64;
        let uniform_bytes = (HEADER_BYTES + round_up(job.uniforms.len() * 4, 16)) as u64;

        let mut uniform_data = Vec::with_capacity(uniform_bytes as usize);
        uniform_data.extend_from_slice(&job.width.to_ne_bytes());
        uniform_data.extend_from_slice(&job.height.to_ne_bytes());
        uniform_data.extend_from_slice(&[0u8; 8]);
        for value in &job.uniforms {
            uniform_data.extend_from_slice(&value.to_ne_bytes());
        }
        uniform_data.resize(uniform_bytes as usize, 0);

        let mut state = self.lock_state();
        state
            .pipelines
            .entry(key)
            .or_insert_with(|| self.build_pipeline(key, source));
        let buffers_fit = state
            .buffers
            .as_ref()
            .is_some_and(|b| b.output_bytes == output_bytes && b.uniform_bytes == uniform_bytes);
        if !buffers_fit {
            state.buffers = Some(self.create_buffers(output_bytes, uniform_bytes));
            state.bind_group = None;
        }
        let RunnerState {
            pipelines,
            buffers,
            bind_group,
        } = &mut *state;
        let (Some(pipeline), Some(buffers)) = (pipelines.get(&key), buffers.as_ref()) else {
            return Err("GPU resources were not created".to_string());
        };
        if bind_group.as_ref().map(|(bound_key, _)| *bound_key) != Some(key) {
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ilium-gpu bindings"),
                layout: &pipeline.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffers.output.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: buffers.uniform.as_entire_binding(),
                    },
                ],
            });
            *bind_group = Some((key, group));
        }
        let Some((_, group)) = bind_group.as_ref() else {
            return Err("GPU bind group was not created".to_string());
        };

        self.queue.write_buffer(&buffers.uniform, 0, &uniform_data);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ilium-gpu frame"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ilium-gpu compute"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(
                job.width.div_ceil(WORKGROUP_SIZE),
                job.height.div_ceil(WORKGROUP_SIZE),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&buffers.output, 0, &buffers.staging, 0, output_bytes);
        let submission = self.queue.submit(Some(encoder.finish()));

        let (sender, receiver) = mpsc::channel();
        buffers
            .staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                // The receiver may be gone after a timeout; nothing to do then.
                let _ = sender.send(result);
            });
        let waited = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(GPU_TIMEOUT),
        });
        let mapped = waited
            .map_err(|error| format!("GPU wait failed: {error}"))
            .and_then(|_| {
                receiver
                    .recv_timeout(GPU_TIMEOUT)
                    .map_err(|_| "GPU readback timed out".to_string())
            })
            .and_then(|result| result.map_err(|error| format!("GPU readback failed: {error}")));
        if let Err(message) = mapped {
            // A late map callback would leave the staging buffer mapped, so
            // drop all sized resources and start clean on the next call.
            state.buffers = None;
            state.bind_group = None;
            return Err(message);
        }
        let copied = buffers
            .staging
            .slice(..)
            .get_mapped_range()
            .map(|view| {
                for (target, chunk) in out.iter_mut().zip(view.chunks_exact(4)) {
                    *target = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                }
            })
            .map_err(|error| format!("GPU readback range failed: {error}"));
        buffers.staging.unmap();
        copied
    }

    /// Test hook: runs the built-in gradient kernel.
    #[cfg(test)]
    pub(crate) fn run_selftest(&self, job: &GpuJob, out: &mut [f32]) -> Result<(), String> {
        self.run_source(
            PipelineKey::SelfTest,
            crate::shaders::SELFTEST_SOURCE,
            job,
            out,
        )
    }
}

impl GpuRunner for WgpuRunner {
    fn adapter_name(&self) -> String {
        self.adapter_name.clone()
    }

    fn run(&self, job: &GpuJob, out: &mut [f32]) -> Result<(), String> {
        let source =
            shader_source(job.kernel).ok_or_else(|| "kernel shader not available".to_string())?;
        self.run_source(PipelineKey::Kernel(job.kernel), source, job, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_job(width: u32, height: u32, gain: f32) -> GpuJob {
        GpuJob {
            kernel: GpuKernel::FbmClouds,
            width,
            height,
            uniforms: vec![gain, 0.0, 0.0, 0.0],
        }
    }

    fn expected(width: u32, height: u32, gain: f32) -> Vec<f32> {
        let span = (width + height - 2) as f32;
        (0..height)
            .flat_map(|y| (0..width).map(move |x| gain * (x + y) as f32 / span))
            .collect()
    }

    /// Runs the gradient kernel on a real adapter; skips (passes) when none exists.
    #[test]
    fn selftest_kernel_runs_on_a_real_adapter() {
        let outcome = probe();
        let Some(runner) = outcome.runner else {
            println!("SKIP: no usable GPU adapter ({:?})", outcome.availability);
            return;
        };
        println!("selftest adapter: {}", runner.adapter_name());

        // Sizes that are not multiples of the workgroup size exercise the
        // bounds check; repeated and changing sizes exercise buffer reuse.
        for (width, height, gain) in [
            (37u32, 19u32, 1.0f32),
            (37, 19, 2.0),
            (64, 64, 0.5),
            (5, 3, 1.0),
        ] {
            let mut out = vec![-1.0f32; (width * height) as usize];
            runner
                .run_selftest(&gradient_job(width, height, gain), &mut out)
                .expect("gradient run");
            let wanted = expected(width, height, gain);
            for (index, (got, want)) in out.iter().zip(&wanted).enumerate() {
                assert!(
                    (got - want).abs() < 1e-5,
                    "{width}x{height} cell {index}: got {got}, want {want}"
                );
            }
        }
    }

    #[test]
    fn scene_kernels_report_missing_shader() {
        let outcome = probe();
        let Some(runner) = outcome.runner else {
            println!("SKIP: no usable GPU adapter ({:?})", outcome.availability);
            return;
        };
        let mut out = vec![0.0f32; 16];
        for kernel in [GpuKernel::DitheredWaves, GpuKernel::DithrPatterns] {
            let job = GpuJob {
                kernel,
                width: 4,
                height: 4,
                uniforms: Vec::new(),
            };
            assert_eq!(
                runner.run(&job, &mut out),
                Err("kernel shader not available".to_string())
            );
        }
    }

    #[test]
    fn mismatched_output_length_is_rejected() {
        let outcome = probe();
        let Some(runner) = outcome.runner else {
            println!("SKIP: no usable GPU adapter ({:?})", outcome.availability);
            return;
        };
        let mut out = vec![0.0f32; 3];
        let result = runner.run_selftest(&gradient_job(4, 4, 1.0), &mut out);
        assert!(result.is_err());
    }
}
