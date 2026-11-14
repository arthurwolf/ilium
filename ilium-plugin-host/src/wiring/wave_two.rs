//! Equips the wave-two imports of a new plugin instance: `process`, `fs` and `http`. `jobs` is
//! equipped by the runtime itself, because a job needs the runtime's queue.
//!
//! Every import is built fresh for the instance, from the instance's own grants, so what one
//! plugin opens, spawns or streams is never visible to another. Dropping the instance drops the
//! imports, which stops their watches, streams and child processes.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::side::Side;

use crate::imports::env::ImportEnv;
use crate::imports::fs::FsImport;
use crate::imports::generated_dispatch::ImportSet;
use crate::imports::http::HttpImport;
use crate::imports::process::ProcessImport;
use crate::runtime::InstanceContext;

/// Where a process keeps what the wave-two imports need from the disk.
#[derive(Debug, Clone, Default)]
pub struct Backends {
    /// The project folder: the `project` scope of `fs` and the working folder of `process`.
    pub project_dir: PathBuf,
    /// The data folder; `fs` keeps its per-plugin scopes under `plugins/fs/<plugin>/`.
    pub data_dir: PathBuf,
}

impl Backends {
    /// The backends of a process working in `project_dir` and keeping data in `data_dir`.
    pub fn new(project_dir: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            project_dir: project_dir.into(),
            data_dir: data_dir.into(),
        }
    }

    /// `<data dir>/plugins/fs/<plugin>`, the root of the three private `fs` scopes of a plugin.
    pub fn fs_root(&self, plugin: &str) -> PathBuf {
        self.data_dir.join("plugins").join("fs").join(plugin)
    }
}

/// Builds the `process`, `fs` and `http` imports of one instance and puts them in `set`.
/// `process` runs on the server only. An `fs` scope folder that cannot be created leaves `fs`
/// out, so the instance still starts and a call reports the interface as unavailable.
pub fn equip(backends: &Backends, context: &InstanceContext, set: &mut ImportSet) {
    let env = ImportEnv::for_instance(context);
    set.http = Some(Box::new(HttpImport::new(env.clone())));
    let has_project = !backends.project_dir.as_os_str().is_empty();
    if context.side == Side::Server {
        let mut process = ProcessImport::new(env.clone());
        if has_project {
            process.set_scope_root("project", backends.project_dir.clone());
        }
        set.process = Some(Box::new(process));
    }
    let root = backends.fs_root(&context.plugin);
    let mut fs = FsImport::new(env);
    if fs
        .configure_scope_roots(root.join("package"), root.join("data"), root.join("cache"))
        .is_err()
    {
        return;
    }
    if has_project {
        let grants: Vec<Grant> = context
            .granted
            .iter()
            .filter_map(|grant| Grant::parse(grant).ok())
            .collect();
        if fs
            .configure_project_scope(backends.project_dir.clone(), Arc::new(Mutex::new(grants)))
            .is_err()
        {
            return;
        }
    }
    set.fs = Some(Box::new(fs));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::env::{RequestIds, SessionCounters};
    use crate::runtime::pending_events::PendingEvents;
    use ilium_plugin_api::Tier;
    use std::time::Instant;

    fn context(side: Side, granted: &[&str]) -> InstanceContext {
        InstanceContext {
            plugin: "test.equip".to_owned(),
            side,
            tier: Tier::T1,
            instance: 1,
            granted: granted.iter().map(|grant| (*grant).to_owned()).collect(),
            results: Arc::new(Mutex::new(Default::default())),
            events: PendingEvents::new(),
            ids: RequestIds::new(),
            session: SessionCounters::new(),
            clock_origin: Instant::now(),
        }
    }

    #[test]
    fn the_server_gets_process_fs_and_http_and_the_client_no_process() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = Backends::new(scratch.path().join("project"), scratch.path().join("data"));
        std::fs::create_dir_all(&backends.project_dir).expect("project folder");
        let mut server_set = ImportSet::default();
        equip(&backends, &context(Side::Server, &[]), &mut server_set);
        assert!(server_set.process.is_some());
        assert!(server_set.fs.is_some());
        assert!(server_set.http.is_some());
        let mut client_set = ImportSet::default();
        equip(&backends, &context(Side::Client, &[]), &mut client_set);
        assert!(client_set.process.is_none());
        assert!(client_set.fs.is_some());
        assert!(client_set.http.is_some());
    }

    #[test]
    fn each_plugin_gets_its_own_scope_folders() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let backends = Backends::new(scratch.path().join("project"), scratch.path().join("data"));
        assert_ne!(backends.fs_root("a.plugin"), backends.fs_root("b.plugin"));
        let mut set = ImportSet::default();
        equip(&backends, &context(Side::Server, &[]), &mut set);
        assert!(backends.fs_root("test.equip").join("data").is_dir());
    }
}
