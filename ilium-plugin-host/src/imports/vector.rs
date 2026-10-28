//! Host implementation of `ilium:host/vector`: embedding requests and host-held vector indexes.
//!
//! `embed` queues a request for the selected inference provider; the host delivers the vectors with
//! [`VectorImport::complete_embedding`] and the plugin collects them with `take-vectors` once
//! `request-completed` fired. The index functions are answered synchronously from memory
//! (manual page `api-vector`).

use std::collections::{BTreeMap, BTreeSet};

use ilium_plugin_api::generated::host::vector::HostVector;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{IndexId, RequestId};

/// Largest number of texts one `embed` call accepts.
const EMBED_TEXT_LIMIT: usize = 256;
/// Largest size in bytes of one text given to `embed`.
const EMBED_TEXT_BYTES: usize = 8 * 1024;
/// Largest `k` that `top-k` accepts.
const TOP_K_LIMIT: u16 = 100;
/// Default number of indexes a plugin may hold (`vector_indexes`).
const DEFAULT_INDEX_LIMIT: usize = 4;
/// Default number of entries over all indexes (`vector_entries`).
const DEFAULT_ENTRY_LIMIT: usize = 50_000;

#[derive(Debug)]
struct Index {
    dimensions: usize,
    entries: BTreeMap<String, Vec<f32>>,
}

/// Host side of the `vector` interface.
#[derive(Debug)]
pub struct VectorImport {
    index_limit: usize,
    entry_limit: usize,
    next_request: u64,
    next_index: u32,
    /// Requests accepted but not yet completed by the provider.
    pending: BTreeSet<u64>,
    /// Completed requests whose vectors the plugin has not taken yet.
    completed: BTreeMap<u64, Vec<Vec<f32>>>,
    indexes: BTreeMap<u32, Index>,
}

impl Default for VectorImport {
    fn default() -> Self {
        Self::with_limits(DEFAULT_INDEX_LIMIT, DEFAULT_ENTRY_LIMIT)
    }
}

fn error(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    let dot: f32 = left.iter().zip(right).map(|(a, b)| a * b).sum();
    let left_norm = left.iter().map(|a| a * a).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|b| b * b).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    dot / (left_norm * right_norm)
}

impl VectorImport {
    /// Creates an import with explicit quota limits (`vector_indexes`, `vector_entries`).
    pub fn with_limits(index_limit: usize, entry_limit: usize) -> Self {
        Self {
            index_limit,
            entry_limit,
            next_request: 1,
            next_index: 1,
            pending: BTreeSet::new(),
            completed: BTreeMap::new(),
            indexes: BTreeMap::new(),
        }
    }

    /// Delivers the provider's vectors for `request`. Returns false for a request that is not pending.
    pub fn complete_embedding(&mut self, request: RequestId, vectors: Vec<Vec<f32>>) -> bool {
        if !self.pending.remove(&request.0) {
            return false;
        }
        self.completed.insert(request.0, vectors);
        true
    }

    fn total_entries(&self) -> usize {
        self.indexes.values().map(|index| index.entries.len()).sum()
    }
}

impl HostVector for VectorImport {
    fn embed(&mut self, texts: Vec<String>, _purpose: String) -> Result<RequestId, PluginError> {
        if texts.is_empty() {
            return Err(error(ErrorCode::InvalidArgument, "embed needs at least one text"));
        }
        if texts.len() > EMBED_TEXT_LIMIT {
            return Err(error(
                ErrorCode::InvalidArgument,
                "embed takes at most 256 texts per call",
            ));
        }
        if texts.iter().any(|text| text.len() > EMBED_TEXT_BYTES) {
            return Err(error(
                ErrorCode::InvalidArgument,
                "a text for embed is larger than 8 KiB",
            ));
        }
        let request = self.next_request;
        self.next_request = request.saturating_add(1);
        self.pending.insert(request);
        Ok(RequestId(request))
    }

    fn take_vectors(&mut self, request: RequestId) -> Option<Vec<Vec<f32>>> {
        self.completed.remove(&request.0)
    }

    fn create_index(&mut self, _name: String, dimensions: u16) -> Result<IndexId, PluginError> {
        if dimensions == 0 {
            return Err(error(
                ErrorCode::InvalidArgument,
                "an index needs at least one dimension",
            ));
        }
        if self.indexes.len() >= self.index_limit {
            return Err(error(
                ErrorCode::QuotaExceeded,
                "the plugin holds the maximum number of vector indexes",
            ));
        }
        let id = self.next_index;
        self.next_index = id.saturating_add(1);
        self.indexes.insert(
            id,
            Index {
                dimensions: usize::from(dimensions),
                entries: BTreeMap::new(),
            },
        );
        Ok(IndexId(id))
    }

    fn upsert(
        &mut self,
        index: IndexId,
        entries: Vec<(String, Vec<f32>)>,
    ) -> Result<(), PluginError> {
        let dimensions = self
            .indexes
            .get(&index.0)
            .map(|held| held.dimensions)
            .ok_or_else(|| error(ErrorCode::NotFound, "unknown vector index"))?;
        if entries.iter().any(|(_, vector)| vector.len() != dimensions) {
            return Err(error(
                ErrorCode::InvalidArgument,
                "a vector length differs from the index dimensions",
            ));
        }
        let held = self.indexes.get(&index.0);
        let new_ids: BTreeSet<&String> = entries
            .iter()
            .map(|(id, _)| id)
            .filter(|id| held.is_some_and(|i| !i.entries.contains_key(*id)))
            .collect();
        if self.total_entries() + new_ids.len() > self.entry_limit {
            return Err(error(
                ErrorCode::QuotaExceeded,
                "the plugin holds the maximum number of vector entries",
            ));
        }
        if let Some(held) = self.indexes.get_mut(&index.0) {
            for (id, vector) in entries {
                held.entries.insert(id, vector);
            }
        }
        Ok(())
    }

    fn remove_entries(&mut self, index: IndexId, ids: Vec<String>) -> Result<(), PluginError> {
        let held = self
            .indexes
            .get_mut(&index.0)
            .ok_or_else(|| error(ErrorCode::NotFound, "unknown vector index"))?;
        for id in ids {
            held.entries.remove(&id);
        }
        Ok(())
    }

    fn top_k(
        &mut self,
        index: IndexId,
        query: Vec<f32>,
        k: u16,
    ) -> Result<Vec<(String, f32)>, PluginError> {
        if k == 0 || k > TOP_K_LIMIT {
            return Err(error(ErrorCode::InvalidArgument, "k must be 1 to 100"));
        }
        let held = self
            .indexes
            .get(&index.0)
            .ok_or_else(|| error(ErrorCode::NotFound, "unknown vector index"))?;
        if query.is_empty() || query.len() != held.dimensions {
            return Err(error(
                ErrorCode::InvalidArgument,
                "the query length differs from the index dimensions",
            ));
        }
        let mut hits: Vec<(String, f32)> = held
            .entries
            .iter()
            .map(|(id, vector)| (id.clone(), cosine(&query, vector)))
            .collect();
        hits.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        hits.truncate(usize::from(k));
        Ok(hits)
    }

    fn drop_index(&mut self, index: IndexId) {
        self.indexes.remove(&index.0);
    }
}

#[cfg(test)]
mod tests {
    use super::VectorImport;
    use ilium_plugin_api::generated::host::vector::HostVector;
    use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
    use ilium_plugin_api::generated::types::ids::{IndexId, RequestId};

    /// Largest number of texts that one `embed` call accepts.
    const EMBED_TEXT_LIMIT: usize = 256;
    /// Largest size in bytes of one text given to `embed` (8 KiB).
    const EMBED_TEXT_BYTES: usize = 8 * 1024;
    /// Largest `k` that `top-k` accepts.
    const TOP_K_LIMIT: u16 = 100;
    /// Default number of indexes a plugin may hold (`vector_indexes`).
    const INDEX_LIMIT: usize = 4;
    /// Default number of entries over all indexes (`vector_entries`).
    const ENTRY_LIMIT: usize = 50_000;

    /// Fails the test unless `result` is an error with the expected code.
    fn expect_code<T: std::fmt::Debug>(result: Result<T, PluginError>, expected: ErrorCode) {
        match result {
            Ok(value) => panic!("expected {expected:?}, got Ok({value:?})"),
            Err(error) => assert_eq!(error.code, expected, "message: {}", error.message),
        }
    }

    /// REQ: API-001
    #[test]
    fn embed_returns_request_id_for_one_valid_text() {
        let mut import = VectorImport::default();
        let request = import.embed(vec!["hello".to_string()], "note search".to_string());
        assert!(
            request.is_ok(),
            "embed of one text must return a request id"
        );
    }

    /// REQ: API-061
    #[test]
    fn embed_rejects_empty_text_list_with_invalid_argument() {
        let mut import = VectorImport::default();
        let result = import.embed(Vec::new(), "note search".to_string());
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn embed_accepts_exactly_256_texts() {
        let mut import = VectorImport::default();
        let texts = vec!["x".to_string(); EMBED_TEXT_LIMIT];
        let result = import.embed(texts, "note search".to_string());
        assert!(
            result.is_ok(),
            "256 texts is the limit and must be accepted"
        );
    }

    /// REQ: API-060
    #[test]
    fn embed_rejects_257_texts_instead_of_truncating() {
        let mut import = VectorImport::default();
        let texts = vec!["x".to_string(); EMBED_TEXT_LIMIT + 1];
        let result = import.embed(texts, "note search".to_string());
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn embed_accepts_text_of_exactly_8_kib() {
        let mut import = VectorImport::default();
        let text = "a".repeat(EMBED_TEXT_BYTES);
        let result = import.embed(vec![text], "note search".to_string());
        assert!(
            result.is_ok(),
            "an 8 KiB text is the limit and must be accepted"
        );
    }

    /// REQ: API-060
    #[test]
    fn embed_rejects_text_one_byte_over_8_kib() {
        let mut import = VectorImport::default();
        let text = "a".repeat(EMBED_TEXT_BYTES + 1);
        let result = import.embed(vec![text], "note search".to_string());
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-014
    #[test]
    fn take_vectors_returns_none_before_embed_completes_and_on_repeat() {
        let mut import = VectorImport::default();
        let request = import
            .embed(vec!["hello".to_string()], "note search".to_string())
            .expect("embed of one text must return a request id");
        assert_eq!(
            import.take_vectors(request),
            None,
            "take before completion must be none"
        );
        assert_eq!(
            import.take_vectors(request),
            None,
            "a second take must be none"
        );
    }

    /// REQ: ABI-014
    #[test]
    fn take_vectors_of_unknown_request_returns_none() {
        let mut import = VectorImport::default();
        assert_eq!(import.take_vectors(RequestId(u64::MAX)), None);
    }

    /// REQ: API-001
    #[test]
    fn create_index_returns_id_for_minimum_dimension() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 1);
        assert!(index.is_ok(), "one dimension is the smallest valid index");
    }

    /// REQ: API-061
    #[test]
    fn create_index_rejects_zero_dimensions_with_invalid_argument() {
        let mut import = VectorImport::default();
        let result = import.create_index("notes".to_string(), 0);
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn create_index_fifth_index_is_quota_exceeded() {
        let mut import = VectorImport::default();
        for position in 0..INDEX_LIMIT {
            let result = import.create_index(format!("index-{position}"), 3);
            assert!(
                result.is_ok(),
                "index {position} is within the default limit"
            );
        }
        let result = import.create_index("one-too-many".to_string(), 3);
        expect_code(result, ErrorCode::QuotaExceeded);
    }

    /// REQ: API-001
    #[test]
    fn upsert_accepts_entries_matching_index_dimensions() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 3).expect("index");
        let entries = vec![("note-1".to_string(), vec![0.1, 0.2, 0.3])];
        assert!(import.upsert(index, entries).is_ok());
    }

    /// REQ: API-061
    #[test]
    fn upsert_rejects_vector_length_differing_from_index_dimensions() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 3).expect("index");
        let entries = vec![("note-1".to_string(), vec![0.1, 0.2])];
        expect_code(import.upsert(index, entries), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn upsert_beyond_entry_limit_is_quota_exceeded() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 1).expect("index");
        let entries: Vec<(String, Vec<f32>)> = (0..=ENTRY_LIMIT)
            .map(|position| (format!("note-{position}"), vec![1.0]))
            .collect();
        expect_code(import.upsert(index, entries), ErrorCode::QuotaExceeded);
    }

    /// REQ: API-002
    #[test]
    fn remove_entries_unknown_index_is_not_found() {
        let mut import = VectorImport::default();
        let result = import.remove_entries(IndexId(u32::MAX), vec!["note-1".to_string()]);
        expect_code(result, ErrorCode::NotFound);
    }

    /// REQ: API-001
    #[test]
    fn remove_entries_ignores_id_not_in_index() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        let result = import.remove_entries(index, vec!["missing".to_string()]);
        assert!(result.is_ok(), "an id that is not in the index is ignored");
    }

    /// REQ: API-001
    #[test]
    fn top_k_returns_nearest_entry_first_with_cosine_score() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        import
            .upsert(
                index,
                vec![
                    ("a".to_string(), vec![1.0, 0.0]),
                    ("b".to_string(), vec![0.0, 1.0]),
                ],
            )
            .expect("upsert");
        let hits = import.top_k(index, vec![1.0, 0.0], 2).expect("top-k");
        assert_eq!(hits.len(), 2, "both entries fit within k = 2");
        assert_eq!(
            hits[0].0, "a",
            "the entry in the same direction ranks first"
        );
        assert!(
            (hits[0].1 - 1.0).abs() < 1e-6,
            "cosine of identical vectors is 1.0"
        );
        assert!(hits[0].1 >= hits[1].1, "scores are highest first");
    }

    /// REQ: API-061
    #[test]
    fn top_k_with_zero_k_is_invalid_argument() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(
            import.top_k(index, vec![1.0, 0.0], 0),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-061
    #[test]
    fn top_k_accepts_k_of_exactly_100() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        let result = import.top_k(index, vec![1.0, 0.0], TOP_K_LIMIT);
        assert!(result.is_ok(), "k = 100 is the largest accepted value");
    }

    /// REQ: API-061
    #[test]
    fn top_k_with_k_above_100_is_invalid_argument() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(
            import.top_k(index, vec![1.0, 0.0], TOP_K_LIMIT + 1),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-061
    #[test]
    fn top_k_query_with_wrong_dimension_is_invalid_argument() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(
            import.top_k(index, vec![1.0, 0.0, 0.0], 1),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-061
    #[test]
    fn top_k_empty_query_is_invalid_argument() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(
            import.top_k(index, Vec::new(), 1),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-002
    #[test]
    fn top_k_unknown_index_is_not_found() {
        let mut import = VectorImport::default();
        expect_code(
            import.top_k(IndexId(u32::MAX), vec![1.0, 0.0], 1),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-002
    #[test]
    fn drop_index_of_unknown_id_is_ignored() {
        let mut import = VectorImport::default();
        import.drop_index(IndexId(u32::MAX));
    }

    /// REQ: API-002
    #[test]
    fn drop_index_twice_is_ignored_the_second_time() {
        let mut import = VectorImport::default();
        let index = import.create_index("notes".to_string(), 2).expect("index");
        import.drop_index(index);
        import.drop_index(index);
    }
}
