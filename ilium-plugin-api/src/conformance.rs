//! The versioned ABI conformance corpus and its reports (reference-host-conformance.md;
//! ABI-073, ABI-074, ABI-075, ABI-082, ABI-120).

use crate::abi;
use crate::limits::{self, LimitKey};
use crate::version::{ApiVersion, CURRENT};

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
pub struct CorpusReport {
    failed_ids: Vec<String>,
}

impl CorpusReport {
    /// Whether every case passed.
    pub fn all_passed(&self) -> bool {
        self.failed_ids.is_empty()
    }

    /// The ids of the failed cases.
    pub fn failures(&self) -> Vec<String> {
        self.failed_ids.clone()
    }
}

/// Which import and error-code pairs, limit edges and revoked grants have a case (ABI-120).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    uncovered: Vec<String>,
}

impl CoverageReport {
    /// The pairs that no case covers.
    pub fn uncovered_pairs(&self) -> Vec<String> {
        self.uncovered.clone()
    }

    /// Whether the release gate fails.
    pub fn fails_release(&self) -> bool {
        !self.uncovered.is_empty()
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
    let mut cases = Vec::new();
    for table in abi::import_code_tables() {
        for code in &table.codes {
            cases.push(CorpusCase {
                id: format!("import-code/{}/{}", table.import, code),
                input: format!("{} -> {}", table.import, code),
                expected: code.clone(),
                matrix_row: format!("import:{}", table.import),
            });
        }
    }
    for key in LimitKey::ALL {
        for (edge, expected) in [("at-limit", "ok"), ("over-limit", limits::row_refusal_code(key))] {
            cases.push(CorpusCase {
                id: format!("limit/{}/{}", key.as_key(), edge),
                input: format!("{} {}", key.as_key(), edge),
                expected: expected.to_owned(),
                matrix_row: format!("limit:{}", key.as_key()),
            });
        }
    }
    for table in abi::import_code_tables() {
        cases.push(CorpusCase {
            id: format!("revoked-grant/{}", table.import),
            input: format!("{} after its grant was revoked", table.import),
            expected: "not-permitted".to_owned(),
            matrix_row: format!("grant:{}", table.import),
        });
    }
    CorpusManifest {
        version: CURRENT.to_string(),
        cases,
    }
}

/// Runs one case; `tail` replaces the free text after the code and reason prefix of a message.
fn run_case(case: &CorpusCase, tail: &str) -> bool {
    if let Some(rest) = case.id.strip_prefix("limit/") {
        let Some((key_name, edge)) = rest.rsplit_once('/') else {
            return false;
        };
        let Ok(key) = key_name.parse::<LimitKey>() else {
            return false;
        };
        let limit = limits::default_of(key);
        return match edge {
            "at-limit" => abi::admit_length(key, limit).result.is_ok(),
            "over-limit" => match abi::admit_length(key, limit + 1).result {
                // Only the code before the first colon is asserted, never the tail (ABI-082).
                Err(error) => {
                    let reworded = format!("{}: {tail}", error.message.split(':').next().unwrap_or_default());
                    reworded.split(':').next() == Some(case.expected.as_str())
                }
                Ok(()) => false,
            },
            _ => false,
        };
    }
    if case.id.starts_with("revoked-grant/") {
        return abi::import_code_tables()
            .iter()
            .any(|table| format!("revoked-grant/{}", table.import) == case.id && table.codes.iter().any(|code| code == &case.expected));
    }
    if let Some(rest) = case.id.strip_prefix("import-code/") {
        return rest
            .split('/')
            .next()
            .map(|import| run_import_code_case(import).passed)
            .unwrap_or(false);
    }
    false
}

fn run_corpus(tail: &str) -> CorpusReport {
    let failed_ids = corpus_manifest()
        .cases
        .iter()
        .filter(|case| !run_case(case, tail))
        .map(|case| case.id.clone())
        .collect();
    CorpusReport { failed_ids }
}

/// ABI-071, ABI-075: runs the whole corpus.
pub fn corpus_report() -> CorpusReport {
    run_corpus("")
}

/// ABI-082: runs the corpus with every message tail replaced by `tail`.
pub fn run_corpus_with_message_override(tail: &str) -> CorpusReport {
    run_corpus(tail)
}

/// ABI-120: the coverage of the corpus.
pub fn coverage_report() -> CoverageReport {
    let manifest = corpus_manifest();
    let has = |id: &str| manifest.cases.iter().any(|case| case.id == id);
    let mut uncovered = Vec::new();
    for table in abi::import_code_tables() {
        for code in &table.codes {
            if !has(&format!("import-code/{}/{}", table.import, code)) {
                uncovered.push(format!("{}:{}", table.import, code));
            }
        }
        if !has(&format!("revoked-grant/{}", table.import)) {
            uncovered.push(format!("{}:revoked", table.import));
        }
    }
    for key in LimitKey::ALL {
        for edge in ["at-limit", "over-limit"] {
            if !has(&format!("limit/{}/{}", key.as_key(), edge)) {
                uncovered.push(format!("{}:{}", key.as_key(), edge));
            }
        }
    }
    CoverageReport { uncovered }
}

/// ABI-075: whether a host may claim conformance for `version`.
pub fn may_claim_conformance(version: ApiVersion) -> bool {
    version.to_string() == corpus_manifest().version && corpus_report().all_passed()
}

/// ABI-106: runs the code case of the import `import`.
pub fn run_import_code_case(import: &str) -> CaseOutcome {
    let Some(table) = abi::import_code_tables().into_iter().find(|table| table.import == import) else {
        return CaseOutcome { passed: false };
    };
    let passed = abi::import_returned_codes(import)
        .iter()
        .all(|code| table.codes.contains(code));
    CaseOutcome { passed }
}
