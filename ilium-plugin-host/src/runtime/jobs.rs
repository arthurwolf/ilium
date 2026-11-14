//! The runtime side of jobs: starting the fresh job instance of a queued job and running it on a
//! worker thread (api-jobs).
//!
//! `JobsImport::start` only queues a job in the [`JobBook`]. Every pump the runtime thread takes
//! the queued jobs and, for each one, builds a dedicated instance of the plugin's component with
//! the job's deadline (`deadline_ms`) and memory cap (`memory_mib`) as its limits, then runs the
//! `job-runner` export on a worker thread. The worker queues `request-completed` when the run
//! ends; the instance reports progress through its own `jobs` import. A trap of the deadline or
//! the memory cap ends the job as `timed-out` or `quota-exceeded`; the plugin itself keeps running.

use std::sync::Arc;

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::limits::LimitKey;
use ilium_plugin_api::{ErrorCode, PluginError};

use crate::calls::Trap;
use crate::imports::backed::SinkLog;
use crate::imports::clock::ClockImport;
use crate::imports::env::ImportEnv;
use crate::imports::generated_dispatch::ImportSet;
use crate::imports::jobs::{JobBook, JobDeclaration, JobsImport, QueuedJob};
use crate::instance::{ExportCall, ExportReply, PluginInstance};
use crate::store_data::GuestIdentity;
use crate::wasm_guest::WasmGuest;

use super::{restrict_to_grants, HostRuntime, InstanceContext, PluginState};

/// The declared job kinds of a manifest (`contributes.jobs`).
pub(super) fn declared_jobs(manifest: &serde_json::Value) -> Vec<JobDeclaration> {
    manifest
        .pointer("/contributes/jobs")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(JobDeclaration::from_manifest)
                .collect()
        })
        .unwrap_or_default()
}

/// How a trap that ended a job reads to the plugin that started it.
fn job_error_of(trap: &Trap) -> PluginError {
    let (code, message) = match &*trap.code.0 {
        "trap.deadline" => (ErrorCode::TimedOut, "the job ran past its deadline_ms"),
        "trap.memory" => (
            ErrorCode::QuotaExceeded,
            "the job used more than its memory_mib",
        ),
        _ => (ErrorCode::Internal, "the job instance trapped"),
    };
    PluginError {
        code,
        message: message.to_owned(),
        retry_after_ms: None,
    }
}

impl HostRuntime {
    /// Starts every queued job; returns how many started. A job whose plugin stopped or was
    /// restarted since `start` ends as `cancelled`.
    pub(super) fn start_queued_jobs(&mut self) -> usize {
        let queued = self.jobs.take_queued();
        let mut started = 0;
        for job in queued {
            let sink = self.events.sink(&job.plugin, job.instance);
            match self.launch_job(&job) {
                Ok(()) => started += 1,
                Err(failure) => self.jobs.finish(job.request, Err(failure), &sink),
            }
        }
        started
    }

    fn launch_job(&mut self, job: &QueuedJob) -> Result<(), PluginError> {
        let refuse = |code: ErrorCode, message: &str| PluginError {
            code,
            message: message.to_owned(),
            retry_after_ms: None,
        };
        let record = self
            .plugins
            .get(&job.plugin)
            .ok_or_else(|| refuse(ErrorCode::Cancelled, "the plugin is not loaded"))?;
        if record.state != PluginState::Active || record.instance_number != job.instance {
            return Err(refuse(ErrorCode::Cancelled, "the plugin instance ended"));
        }
        let compiled = record
            .compiled
            .clone()
            .ok_or_else(|| refuse(ErrorCode::Unsupported, "the plugin has no component"))?;
        let mut limits = record.limits.clone();
        limits.0.insert(LimitKey::JobDeadlineMs, job.deadline_ms);
        // The job's own cap, never above what the tier allows jobs.
        let memory_mib = limits
            .0
            .get(&LimitKey::JobMemoryMib)
            .map_or(job.memory_mib, |ceiling| (*ceiling).min(job.memory_mib));
        limits.0.insert(LimitKey::MemoryMib, memory_mib);
        let context = InstanceContext {
            plugin: job.plugin.clone(),
            side: self.side,
            tier: record.tier,
            instance: job.instance,
            granted: record.granted.clone(),
            results: Arc::clone(&self.results),
            events: self.events.clone(),
            ids: self.ids.clone(),
            session: self.session.clone(),
            clock_origin: self.clock_origin,
        };
        let env = ImportEnv::for_instance(&context);
        let job_imports = |set: &mut ImportSet| {
            set.jobs = Some(Box::new(JobsImport::for_job(
                env.clone(),
                self.jobs.clone(),
                job.request,
            )));
        };
        let mut set = ImportSet {
            log: Some(Box::new(SinkLog::new(
                Arc::clone(&self.log),
                &job.plugin,
                self.side,
            ))),
            clock: Some(Box::new(ClockImport::with_origin(self.clock_origin))),
            ..ImportSet::default()
        };
        job_imports(&mut set);
        if let Some(provider) = &self.provider {
            provider(&context, &mut set);
        }
        // The provider must not replace the job's own import.
        job_imports(&mut set);
        restrict_to_grants(&mut set, &record.granted);
        if set.jobs.is_none() {
            return Err(refuse(
                ErrorCode::NotPermitted,
                "the plugin holds no jobs grant",
            ));
        }
        let grants: Vec<Grant> = record
            .granted
            .iter()
            .filter_map(|grant| Grant::parse(grant).ok())
            .collect();
        let linker = self
            .linkers
            .get(&self.engine, self.side, record.tier, &grants)
            .map_err(|error| refuse(ErrorCode::Internal, &error.to_string()))?;
        let guest = WasmGuest::instantiate_linked(
            Arc::clone(&self.engine),
            &compiled,
            limits,
            &linker,
            set,
            GuestIdentity {
                plugin: job.plugin.clone(),
                side: self.side,
                granted: record.granted.clone(),
            },
        )
        .map_err(|error| refuse(ErrorCode::Internal, &error.to_string()))?;
        spawn_job_worker(
            PluginInstance::for_job(Box::new(guest)),
            job,
            self.jobs.clone(),
            self.events.sink(&job.plugin, job.instance),
        );
        Ok(())
    }
}

/// Runs the job on its own thread; the thread ends with the job.
fn spawn_job_worker(
    mut instance: PluginInstance,
    job: &QueuedJob,
    book: JobBook,
    sink: crate::runtime::pending_events::EventSink,
) {
    let request = job.request;
    let kind = job.kind.clone();
    let input = job.input.clone();
    let worker = sink.worker();
    std::thread::spawn(move || {
        let _worker = worker;
        let result = match instance.call_export(ExportCall::Job { kind, input }) {
            Ok(ExportReply::Job(result)) => result,
            Ok(_) => Err(PluginError {
                code: ErrorCode::Internal,
                message: "the job instance answered with another reply".to_owned(),
                retry_after_ms: None,
            }),
            Err(trap) => Err(job_error_of(&trap)),
        };
        book.finish(request, result, &sink);
    });
}
