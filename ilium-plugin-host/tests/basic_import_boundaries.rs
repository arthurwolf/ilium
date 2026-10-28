use ilium_plugin_api::generated::{
    primitives::basic::HostBasic,
    types::{
        ids::NodeId,
        text::{Modifiers, Span, Style},
        tree::{NodeClass, PrimitiveKind},
    },
};
use ilium_plugin_api::kernel::TreeNode;
use ilium_plugin_api::{grants::Grant, ErrorCode, ProcessKind};
use ilium_plugin_host::{imports::basic::BasicImport, testing::FakeKernel};
use std::sync::{Arc, Mutex};

fn kernel() -> Arc<FakeKernel> {
    let kernel = Arc::new(FakeKernel::new(ProcessKind::Client));
    kernel.set_tree(vec![TreeNode {
        id: NodeId(13),
        parent: None,
        class: NodeClass::Pane,
        content_kind: Some("test.plugin.notes".into()),
        title: "original".into(),
        cwd: None,
        agent: None,
        locked: false,
        revision: 1,
    }]);
    kernel.register_pane_kind("test.plugin", "test.plugin.notes", PrimitiveKind::Basic);
    kernel
}
fn grants() -> Arc<Mutex<Vec<Grant>>> {
    Arc::new(Mutex::new(vec![
        Grant::parse("panes.provide-kind").unwrap(),
        Grant::parse("tree.metadata").unwrap(),
    ]))
}
#[test]
fn held_import_observes_revocation_and_does_not_mutate() {
    let kernel = kernel();
    let allowed = grants();
    let mut import = BasicImport::new(
        kernel.clone(),
        "test.plugin".into(),
        allowed.clone(),
        |_| {},
    );
    import.set_dirty(NodeId(13), true).unwrap();
    allowed.lock().unwrap().clear();
    assert_eq!(
        import.set_dirty(NodeId(13), false).unwrap_err().code,
        ErrorCode::NotPermitted
    );
    assert!(kernel.pane_is_dirty(NodeId(13)));
}
#[test]
fn unicode_limits_count_characters_and_keep_rich_status() {
    let kernel = kernel();
    let mut import = BasicImport::new(kernel.clone(), "test.plugin".into(), grants(), |_| {});
    import.set_title(NodeId(13), "界".repeat(256)).unwrap();
    assert_eq!(
        import
            .set_title(NodeId(13), "界".repeat(257))
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    let spans = vec![
        Span {
            text: "界".repeat(128),
            style: Style {
                add: Modifiers::BOLD,
                ..Style::default()
            },
        },
        Span {
            text: "é".repeat(128),
            style: Style::default(),
        },
    ];
    import.set_status(NodeId(13), spans.clone()).unwrap();
    assert_eq!(kernel.pane_status_spans(NodeId(13)), Some(spans.clone()));
    assert_eq!(
        import
            .set_status(
                NodeId(13),
                vec![Span {
                    text: "a\u{2028}b".into(),
                    style: Style::default()
                }]
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(kernel.pane_status_spans(NodeId(13)), Some(spans));
}
#[test]
fn foreign_plugin_and_manual_title_cannot_be_overwritten() {
    let kernel = kernel();
    let mut foreign = BasicImport::new(kernel.clone(), "test".into(), grants(), |_| {});
    assert_eq!(
        foreign.set_dirty(NodeId(13), true).unwrap_err().code,
        ErrorCode::NotPermitted
    );
    assert!(!kernel.pane_is_dirty(NodeId(13)));
    kernel.set_manual_title(NodeId(13), "user title");
    let mut owner = BasicImport::new(kernel, "test.plugin".into(), grants(), |_| {});
    assert_eq!(
        owner
            .set_title(NodeId(13), "plugin title".into())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn registered_owned_terminal_kind_is_not_a_basic_pane() {
    let kernel = kernel();
    kernel.register_pane_kind("test.plugin", "test.plugin.notes", PrimitiveKind::Terminal);
    let mut import = BasicImport::new(kernel.clone(), "test.plugin".into(), grants(), |_| {});
    assert_eq!(
        import.set_dirty(NodeId(13), true).unwrap_err().code,
        ErrorCode::NotPermitted
    );
    assert_eq!(import.size(NodeId(13)), None);
    assert!(!kernel.pane_is_dirty(NodeId(13)));
}

#[test]
fn pending_call_cannot_write_using_pre_revocation_grants() {
    let kernel = kernel();
    let grants = grants();
    let mut gate = grants.lock().unwrap();
    let (started, ready) = std::sync::mpsc::channel();
    let worker_kernel = kernel.clone();
    let worker_grants = grants.clone();
    let worker = std::thread::spawn(move || {
        let mut import =
            BasicImport::new(worker_kernel, "test.plugin".into(), worker_grants, |_| {});
        started.send(()).unwrap();
        import.set_dirty(NodeId(13), true)
    });
    ready.recv().unwrap();
    // The request exists before revocation, but has not entered its authorized transaction.
    gate.clear();
    drop(gate);
    assert_eq!(
        worker.join().unwrap().unwrap_err().code,
        ErrorCode::NotPermitted
    );
    assert!(!kernel.pane_is_dirty(NodeId(13)));
}

#[test]
fn backend_mutation_holds_the_same_gate_that_revocation_requires() {
    use ilium_plugin_host::imports::basic::{BasicBackend, BasicChange};
    struct PausedBackend {
        kernel: Arc<FakeKernel>,
        entered: std::sync::mpsc::SyncSender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl BasicBackend for PausedBackend {
        fn apply_basic(
            &self,
            plugin: &str,
            pane: NodeId,
            change: BasicChange,
        ) -> Result<(), ilium_plugin_api::PluginError> {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            self.kernel.apply_basic(plugin, pane, change)
        }
        fn basic_size(&self, plugin: &str, pane: NodeId) -> Option<(u16, u16)> {
            self.kernel.basic_size(plugin, pane)
        }
    }
    let kernel = kernel();
    let grants = grants();
    let (entered, waiting) = std::sync::mpsc::sync_channel(0);
    let (release, resume) = std::sync::mpsc::channel();
    let backend = Arc::new(PausedBackend {
        kernel: kernel.clone(),
        entered,
        release: Mutex::new(resume),
    });
    let worker_grants = grants.clone();
    let worker = std::thread::spawn(move || {
        BasicImport::new(backend, "test.plugin".into(), worker_grants, |_| {})
            .set_dirty(NodeId(13), true)
    });
    waiting.recv().unwrap();
    assert!(matches!(
        grants.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    grants.lock().unwrap().clear();
    let mut import = BasicImport::new(kernel.clone(), "test.plugin".into(), grants, |_| {});
    assert_eq!(
        import.set_dirty(NodeId(13), false).unwrap_err().code,
        ErrorCode::NotPermitted
    );
    assert!(kernel.pane_is_dirty(NodeId(13)));
}
