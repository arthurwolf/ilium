//! Plain data describing what the probe found, plus the ordinary `std::fs`
//! code that gathers the operating-system part of it. No GPU API is touched
//! here, so this module is compiled in every build.

use std::path::{Path, PathBuf};

/// Kind of device an adapter reports, mirrored from the graphics API so the
/// pure diagnosis does not depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterDeviceType {
    Discrete,
    Integrated,
    Virtual,
    /// A CPU rasterizer.
    Cpu,
    Other,
}

/// Graphics API an adapter was found through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterBackend {
    Vulkan,
    Metal,
    Dx12,
    Other,
}

/// One enumerated adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterFact {
    pub name: String,
    pub device_type: AdapterDeviceType,
    pub backend: AdapterBackend,
}

/// One `/dev/dri/renderD*` node and whether this user can open it read-write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriNode {
    pub path: String,
    pub accessible: bool,
}

/// Everything `diagnose` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeFacts {
    pub adapters: Vec<AdapterFact>,
    pub loader_found: bool,
    pub icd_files_found: bool,
    pub dri_nodes: Vec<DriNode>,
    /// Message from a failed device/queue creation on the chosen adapter.
    pub device_error: Option<String>,
}

#[cfg(target_os = "linux")]
const LOADER_DIRECTORIES: &[&str] = &[
    "/usr/lib",
    "/usr/lib64",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/lib/aarch64-linux-gnu",
    "/usr/lib/arm-linux-gnueabihf",
    "/lib",
    "/lib64",
    "/lib/x86_64-linux-gnu",
    "/lib/aarch64-linux-gnu",
    "/usr/local/lib",
];

#[cfg(target_os = "linux")]
const ICD_DIRECTORIES: &[&str] = &["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"];

const LOADER_FILE_NAME: &str = "libvulkan.so.1";

/// Gathers the operating-system facts. Adapters and `device_error` start
/// empty; the caller fills them in. Off Linux the Linux-only facts are
/// skipped and reported as "fine" (`loader_found` / `icd_files_found` true,
/// no DRI nodes) so the adapter list alone decides.
pub fn gather_system_facts() -> ProbeFacts {
    #[cfg(target_os = "linux")]
    {
        ProbeFacts {
            adapters: Vec::new(),
            loader_found: vulkan_loader_present(LOADER_DIRECTORIES),
            icd_files_found: icd_present(ICD_DIRECTORIES),
            dri_nodes: dri_render_nodes(Path::new("/dev/dri")),
            device_error: None,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        ProbeFacts {
            adapters: Vec::new(),
            loader_found: true,
            icd_files_found: true,
            dri_nodes: Vec::new(),
            device_error: None,
        }
    }
}

/// True when any directory holds a file named `libvulkan.so.1`.
pub fn vulkan_loader_present(directories: &[&str]) -> bool {
    directories
        .iter()
        .any(|directory| Path::new(directory).join(LOADER_FILE_NAME).exists())
}

/// True when an ICD manifest (`*.json`) exists in any directory, or the user
/// points the loader at one explicitly through the environment.
pub fn icd_present(directories: &[&str]) -> bool {
    let environment_override = ["VK_ICD_FILENAMES", "VK_DRIVER_FILES"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    environment_override || directories.iter().any(|d| directory_has_json(Path::new(d)))
}

fn directory_has_json(directory: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
}

/// Lists `renderD*` nodes in `directory` (sorted) and whether each can be
/// opened for reading and writing.
pub fn dri_render_nodes(directory: &Path) -> Vec<DriNode> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("renderD"))
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let accessible = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .is_ok();
            DriNode {
                path: path.display().to_string(),
                accessible,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_is_found_only_when_the_file_exists() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().to_string_lossy().into_owned();
        assert!(!vulkan_loader_present(&[path.as_str()]));
        std::fs::write(directory.path().join(LOADER_FILE_NAME), b"").expect("write");
        assert!(vulkan_loader_present(&["/nonexistent-dir", path.as_str()]));
    }

    #[test]
    fn icd_needs_a_json_manifest() {
        if ["VK_ICD_FILENAMES", "VK_DRIVER_FILES"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
        {
            return; // environment override makes the negative case meaningless
        }
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().to_string_lossy().into_owned();
        assert!(!icd_present(&[path.as_str()]));
        std::fs::write(directory.path().join("readme.txt"), b"").expect("write");
        assert!(!icd_present(&[path.as_str()]));
        std::fs::write(directory.path().join("nvidia_icd.json"), b"{}").expect("write");
        assert!(icd_present(&[path.as_str()]));
    }

    #[test]
    fn dri_nodes_list_render_nodes_only_and_report_access() {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("renderD129"), b"").expect("write");
        std::fs::write(directory.path().join("renderD128"), b"").expect("write");
        std::fs::write(directory.path().join("card0"), b"").expect("write");
        let nodes = dri_render_nodes(directory.path());
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].path.ends_with("renderD128"));
        assert!(nodes[1].path.ends_with("renderD129"));
        assert!(nodes.iter().all(|node| node.accessible));
        assert!(dri_render_nodes(Path::new("/nonexistent-dri-dir")).is_empty());
    }
}
