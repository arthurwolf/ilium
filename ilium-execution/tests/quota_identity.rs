use ilium_execution::{QuotaGroup, QuotaLimits};

#[test]
fn cloned_handles_share_admission_but_equal_independent_limits_do_not() {
    let limits = QuotaLimits {
        clients: 2,
        jobs: 4,
        service_jobs: 2,
        input_bytes: 1024,
        result_bytes: 1024,
        worker_threads: 2,
        worker_bytes: 4096,
    };
    let root = QuotaGroup::new(limits);
    let shared = root.clone();
    let independent = QuotaGroup::new(limits);
    assert!(root.shares_root(&shared));
    assert!(shared.shares_root(&root));
    assert!(!root.shares_root(&independent));
}
