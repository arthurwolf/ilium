//! The versioned ABI conformance corpus and its reports (reference-host-conformance.md;
//! ABI-073, ABI-074, ABI-075, ABI-082, ABI-120).

use crate::version::ApiVersion;

/// One case of the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusCase {
    /// Stable unique identifier.
    pub id: String,
    /// The input the case feeds.
    pub input: String,
    /// The expected result or refusal code.
    pub expected: String,
    /// The ABI matrix row the case covers.
    pub matrix_row: String,
}

/// The corpus: a version and its cases (ABI-073).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusManifest {
    /// The API version the corpus checks.
    pub version: String,
    /// The cases.
    pub cases: Vec<CorpusCase>,
}

/// The outcome of running the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusReport;

impl CorpusReport {
    /// Whether every case passed.
    pub fn all_passed(&self) -> bool {
        unimplemented!("api::conformance::CorpusReport::all_passed")
    }

    /// The ids of the failed cases.
    pub fn failures(&self) -> Vec<String> {
        unimplemented!("api::conformance::CorpusReport::failures")
    }
}

/// Which import and error-code pairs, limit edges and revoked grants have a case (ABI-120).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport;

impl CoverageReport {
    /// The pairs that no case covers.
    pub fn uncovered_pairs(&self) -> Vec<String> {
        unimplemented!("api::conformance::CoverageReport::uncovered_pairs")
    }

    /// Whether the release gate fails.
    pub fn fails_release(&self) -> bool {
        unimplemented!("api::conformance::CoverageReport::fails_release")
    }
}

/// The result of one import code case (ABI-106).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseOutcome {
    /// Whether the import returned only codes from its table.
    pub passed: bool,
}

/// ABI-073: the corpus.
pub fn corpus_manifest() -> CorpusManifest {
    unimplemented!("api::conformance::corpus_manifest")
}

/// ABI-071, ABI-075: runs the whole corpus.
pub fn corpus_report() -> CorpusReport {
    unimplemented!("api::conformance::corpus_report")
}

/// ABI-082: runs the corpus with every message tail replaced by `tail`.
pub fn run_corpus_with_message_override(_tail: &str) -> CorpusReport {
    unimplemented!("api::conformance::run_corpus_with_message_override")
}

/// ABI-120: the coverage of the corpus.
pub fn coverage_report() -> CoverageReport {
    unimplemented!("api::conformance::coverage_report")
}

/// ABI-075: whether a host may claim conformance for `version`.
pub fn may_claim_conformance(_version: ApiVersion) -> bool {
    unimplemented!("api::conformance::may_claim_conformance")
}

/// ABI-106: runs the code case of the import `import`.
pub fn run_import_code_case(_import: &str) -> CaseOutcome {
    unimplemented!("api::conformance::run_import_code_case")
}
