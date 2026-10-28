//! reference-kernel-and-boot.md safe-start table, as far as the binary shows it.

use crate::common::*;

/// REQ: HST-033, HST-043
#[test]
fn hst_043_the_manual_documents_the_precedence_of_no_plugins() {
    let page = manual_page("reference-host-architecture");
    assert!(page.contains("--no-plugins"));
    assert!(page.contains("--safe-plugins"));
}
