//! Pure classification of probe facts into a [`GpuAvailability`].

use crate::facts::{AdapterDeviceType, AdapterFact, ProbeFacts};
use ilium_ambient::gpu::{GpuAvailability, GpuUnavailable};

const SOFTWARE_NAME_MARKERS: &[&str] = &["llvmpipe", "lavapipe", "swiftshader"];

/// True for CPU rasterizers: reported as a CPU device, or named like one.
pub fn is_software_adapter(adapter: &AdapterFact) -> bool {
    if adapter.device_type == AdapterDeviceType::Cpu {
        return true;
    }
    let lowered = adapter.name.to_lowercase();
    SOFTWARE_NAME_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

fn device_rank(device_type: AdapterDeviceType) -> u8 {
    match device_type {
        AdapterDeviceType::Discrete => 0,
        AdapterDeviceType::Integrated => 1,
        AdapterDeviceType::Virtual => 2,
        AdapterDeviceType::Other => 3,
        AdapterDeviceType::Cpu => 4,
    }
}

/// Index of the adapter the GPU backend should use: the first hardware
/// adapter of the best kind (discrete, then integrated, virtual, other).
/// `None` when there is no hardware adapter.
pub fn preferred_adapter_index(adapters: &[AdapterFact]) -> Option<usize> {
    adapters
        .iter()
        .enumerate()
        .filter(|(_, adapter)| !is_software_adapter(adapter))
        .min_by_key(|(index, adapter)| (device_rank(adapter.device_type), *index))
        .map(|(index, _)| index)
}

/// Decides whether the GPU option can be used, and if not, why not.
///
/// Order matters: a usable hardware adapter wins over missing Linux files
/// (Metal and DX12 need none); with no hardware adapter the most specific
/// explanation is reported first.
pub fn diagnose(facts: &ProbeFacts) -> GpuAvailability {
    if let Some(index) = preferred_adapter_index(&facts.adapters) {
        let adapter = facts.adapters[index].name.clone();
        return match &facts.device_error {
            Some(message) => GpuAvailability::Unavailable(GpuUnavailable::Failed(message.clone())),
            None => GpuAvailability::Ready { adapter },
        };
    }
    if let Some(software) = facts.adapters.first() {
        return GpuAvailability::Unavailable(GpuUnavailable::SoftwareOnly {
            adapter: software.name.clone(),
        });
    }
    if !facts.loader_found {
        return GpuAvailability::Unavailable(GpuUnavailable::NoVulkanLoader);
    }
    if !facts.dri_nodes.is_empty() && facts.dri_nodes.iter().all(|node| !node.accessible) {
        return GpuAvailability::Unavailable(GpuUnavailable::NoDeviceAccess);
    }
    if let Some(message) = &facts.device_error {
        return GpuAvailability::Unavailable(GpuUnavailable::Failed(message.clone()));
    }
    GpuAvailability::Unavailable(GpuUnavailable::NoDriver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{AdapterBackend, DriNode};

    fn adapter(name: &str, device_type: AdapterDeviceType) -> AdapterFact {
        AdapterFact {
            name: name.to_string(),
            device_type,
            backend: AdapterBackend::Vulkan,
        }
    }

    fn facts() -> ProbeFacts {
        ProbeFacts {
            adapters: Vec::new(),
            loader_found: true,
            icd_files_found: true,
            dri_nodes: Vec::new(),
            device_error: None,
        }
    }

    fn node(accessible: bool) -> DriNode {
        DriNode {
            path: "/dev/dri/renderD128".to_string(),
            accessible,
        }
    }

    fn unavailable(reason: GpuUnavailable) -> GpuAvailability {
        GpuAvailability::Unavailable(reason)
    }

    #[test]
    fn discrete_adapter_is_ready() {
        let mut probe = facts();
        probe.adapters = vec![adapter(
            "NVIDIA GeForce RTX 3090",
            AdapterDeviceType::Discrete,
        )];
        assert_eq!(
            diagnose(&probe),
            GpuAvailability::Ready {
                adapter: "NVIDIA GeForce RTX 3090".to_string()
            }
        );
    }

    #[test]
    fn best_hardware_adapter_is_preferred_over_software_and_integrated() {
        let mut probe = facts();
        probe.adapters = vec![
            adapter("llvmpipe (LLVM 19)", AdapterDeviceType::Cpu),
            adapter("Intel UHD", AdapterDeviceType::Integrated),
            adapter("NVIDIA RTX 3060", AdapterDeviceType::Discrete),
            adapter("NVIDIA RTX 3090", AdapterDeviceType::Discrete),
        ];
        assert_eq!(preferred_adapter_index(&probe.adapters), Some(2));
        assert_eq!(
            diagnose(&probe),
            GpuAvailability::Ready {
                adapter: "NVIDIA RTX 3060".to_string()
            }
        );
    }

    #[test]
    fn integrated_only_is_ready() {
        let mut probe = facts();
        probe.adapters = vec![adapter("Apple M2", AdapterDeviceType::Integrated)];
        assert_eq!(
            diagnose(&probe),
            GpuAvailability::Ready {
                adapter: "Apple M2".to_string()
            }
        );
    }

    #[test]
    fn ready_ignores_missing_linux_files_when_an_adapter_exists() {
        let mut probe = facts();
        probe.adapters = vec![adapter("Apple M2", AdapterDeviceType::Integrated)];
        probe.loader_found = false;
        probe.icd_files_found = false;
        probe.dri_nodes = vec![node(false)];
        assert!(matches!(diagnose(&probe), GpuAvailability::Ready { .. }));
    }

    #[test]
    fn software_adapters_are_recognised_by_type_and_by_name() {
        assert!(is_software_adapter(&adapter(
            "anything",
            AdapterDeviceType::Cpu
        )));
        for name in [
            "llvmpipe (LLVM 19.1.0, 256 bits)",
            "Lavapipe",
            "Google SwiftShader Device",
            "LLVMPIPE",
        ] {
            assert!(
                is_software_adapter(&adapter(name, AdapterDeviceType::Other)),
                "{name}"
            );
        }
        assert!(!is_software_adapter(&adapter(
            "NVIDIA RTX 3090",
            AdapterDeviceType::Discrete
        )));
    }

    #[test]
    fn only_software_adapters_is_software_only() {
        let mut probe = facts();
        probe.adapters = vec![adapter("llvmpipe (LLVM 19)", AdapterDeviceType::Cpu)];
        assert_eq!(
            diagnose(&probe),
            unavailable(GpuUnavailable::SoftwareOnly {
                adapter: "llvmpipe (LLVM 19)".to_string()
            })
        );
    }

    #[test]
    fn software_named_adapter_with_misreported_type_is_software_only() {
        let mut probe = facts();
        probe.adapters = vec![adapter("lavapipe", AdapterDeviceType::Discrete)];
        assert_eq!(
            diagnose(&probe),
            unavailable(GpuUnavailable::SoftwareOnly {
                adapter: "lavapipe".to_string()
            })
        );
    }

    #[test]
    fn software_only_wins_over_loader_and_access_problems() {
        let mut probe = facts();
        probe.adapters = vec![adapter("lavapipe", AdapterDeviceType::Cpu)];
        probe.loader_found = false;
        probe.dri_nodes = vec![node(false)];
        assert!(matches!(
            diagnose(&probe),
            GpuAvailability::Unavailable(GpuUnavailable::SoftwareOnly { .. })
        ));
    }

    #[test]
    fn missing_loader_is_reported_first_without_adapters() {
        let mut probe = facts();
        probe.loader_found = false;
        probe.icd_files_found = false;
        probe.dri_nodes = vec![node(false)];
        assert_eq!(
            diagnose(&probe),
            unavailable(GpuUnavailable::NoVulkanLoader)
        );
    }

    #[test]
    fn inaccessible_render_nodes_mean_no_device_access() {
        let mut probe = facts();
        probe.dri_nodes = vec![node(false), node(false)];
        assert_eq!(
            diagnose(&probe),
            unavailable(GpuUnavailable::NoDeviceAccess)
        );
    }

    #[test]
    fn one_accessible_render_node_is_not_an_access_problem() {
        let mut probe = facts();
        probe.dri_nodes = vec![node(false), node(true)];
        assert_eq!(diagnose(&probe), unavailable(GpuUnavailable::NoDriver));
    }

    #[test]
    fn loader_present_but_no_icd_is_no_driver() {
        let mut probe = facts();
        probe.icd_files_found = false;
        assert_eq!(diagnose(&probe), unavailable(GpuUnavailable::NoDriver));
    }

    #[test]
    fn loader_and_icd_present_but_no_adapter_is_no_driver() {
        assert_eq!(diagnose(&facts()), unavailable(GpuUnavailable::NoDriver));
    }

    #[test]
    fn device_error_on_a_hardware_adapter_is_failed() {
        let mut probe = facts();
        probe.adapters = vec![adapter("NVIDIA RTX 3090", AdapterDeviceType::Discrete)];
        probe.device_error = Some("out of memory".to_string());
        assert_eq!(
            diagnose(&probe),
            unavailable(GpuUnavailable::Failed("out of memory".to_string()))
        );
    }

    #[test]
    fn device_error_without_adapters_is_failed_when_nothing_more_specific_applies() {
        let mut probe = facts();
        probe.device_error = Some("instance creation failed".to_string());
        assert_eq!(
            diagnose(&probe),
            unavailable(GpuUnavailable::Failed(
                "instance creation failed".to_string()
            ))
        );
    }

    #[test]
    fn empty_facts_on_a_machine_without_linux_files_are_not_ready() {
        let probe = ProbeFacts {
            adapters: Vec::new(),
            loader_found: true,
            icd_files_found: true,
            dri_nodes: Vec::new(),
            device_error: None,
        };
        assert!(matches!(
            diagnose(&probe),
            GpuAvailability::Unavailable(GpuUnavailable::NoDriver)
        ));
    }
}
