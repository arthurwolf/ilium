//! Host implementation of `ilium:host/vector` (skeleton: every body is a stub).
//!
//! Written once by the skeleton lane from the generated `HostVector` trait; hand-owned afterwards.

use ilium_plugin_api::generated::host::vector::HostVector;
use ilium_plugin_api::generated::types::errors::PluginError;
use ilium_plugin_api::generated::types::ids::{IndexId, RequestId};

/// Host side of the `vector` interface.
#[derive(Debug)]
pub struct VectorImport;

impl HostVector for VectorImport {
    fn embed(&mut self, _texts: Vec<String>, _purpose: String) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::vector::embed")
    }

    fn take_vectors(&mut self, _request: RequestId) -> Option<Vec<Vec<f32>>> {
        unimplemented!("host::imports::vector::take_vectors")
    }

    fn create_index(&mut self, _name: String, _dimensions: u16) -> Result<IndexId, PluginError> {
        unimplemented!("host::imports::vector::create_index")
    }

    fn upsert(
        &mut self,
        _index: IndexId,
        _entries: Vec<(String, Vec<f32>)>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::vector::upsert")
    }

    fn remove_entries(&mut self, _index: IndexId, _ids: Vec<String>) -> Result<(), PluginError> {
        unimplemented!("host::imports::vector::remove_entries")
    }

    fn top_k(
        &mut self,
        _index: IndexId,
        _query: Vec<f32>,
        _k: u16,
    ) -> Result<Vec<(String, f32)>, PluginError> {
        unimplemented!("host::imports::vector::top_k")
    }

    fn drop_index(&mut self, _index: IndexId) {
        unimplemented!("host::imports::vector::drop_index")
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
        let mut import = VectorImport;
        let request = import.embed(vec!["hello".to_string()], "note search".to_string());
        assert!(request.is_ok(), "embed of one text must return a request id");
    }

    /// REQ: API-061
    #[test]
    fn embed_rejects_empty_text_list_with_invalid_argument() {
        let mut import = VectorImport;
        let result = import.embed(Vec::new(), "note search".to_string());
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn embed_accepts_exactly_256_texts() {
        let mut import = VectorImport;
        let texts = vec!["x".to_string(); EMBED_TEXT_LIMIT];
        let result = import.embed(texts, "note search".to_string());
        assert!(result.is_ok(), "256 texts is the limit and must be accepted");
    }

    /// REQ: API-060
    #[test]
    fn embed_rejects_257_texts_instead_of_truncating() {
        let mut import = VectorImport;
        let texts = vec!["x".to_string(); EMBED_TEXT_LIMIT + 1];
        let result = import.embed(texts, "note search".to_string());
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-060
    #[test]
    fn embed_accepts_text_of_exactly_8_kib() {
        let mut import = VectorImport;
        let text = "a".repeat(EMBED_TEXT_BYTES);
        let result = import.embed(vec![text], "note search".to_string());
        assert!(result.is_ok(), "an 8 KiB text is the limit and must be accepted");
    }

    /// REQ: API-060
    #[test]
    fn embed_rejects_text_one_byte_over_8_kib() {
        let mut import = VectorImport;
        let text = "a".repeat(EMBED_TEXT_BYTES + 1);
        let result = import.embed(vec![text], "note search".to_string());
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: ABI-014
    #[test]
    fn take_vectors_returns_none_before_embed_completes_and_on_repeat() {
        let mut import = VectorImport;
        let request = import
            .embed(vec!["hello".to_string()], "note search".to_string())
            .expect("embed of one text must return a request id");
        assert_eq!(import.take_vectors(request), None, "take before completion must be none");
        assert_eq!(import.take_vectors(request), None, "a second take must be none");
    }

    /// REQ: ABI-014
    #[test]
    fn take_vectors_of_unknown_request_returns_none() {
        let mut import = VectorImport;
        assert_eq!(import.take_vectors(RequestId(u64::MAX)), None);
    }

    /// REQ: API-001
    #[test]
    fn create_index_returns_id_for_minimum_dimension() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 1);
        assert!(index.is_ok(), "one dimension is the smallest valid index");
    }

    /// REQ: API-061
    #[test]
    fn create_index_rejects_zero_dimensions_with_invalid_argument() {
        let mut import = VectorImport;
        let result = import.create_index("notes".to_string(), 0);
        expect_code(result, ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn create_index_fifth_index_is_quota_exceeded() {
        let mut import = VectorImport;
        for position in 0..INDEX_LIMIT {
            let result = import.create_index(format!("index-{position}"), 3);
            assert!(result.is_ok(), "index {position} is within the default limit");
        }
        let result = import.create_index("one-too-many".to_string(), 3);
        expect_code(result, ErrorCode::QuotaExceeded);
    }

    /// REQ: API-001
    #[test]
    fn upsert_accepts_entries_matching_index_dimensions() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 3).expect("index");
        let entries = vec![("note-1".to_string(), vec![0.1, 0.2, 0.3])];
        assert!(import.upsert(index, entries).is_ok());
    }

    /// REQ: API-061
    #[test]
    fn upsert_rejects_vector_length_differing_from_index_dimensions() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 3).expect("index");
        let entries = vec![("note-1".to_string(), vec![0.1, 0.2])];
        expect_code(import.upsert(index, entries), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn upsert_beyond_entry_limit_is_quota_exceeded() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 1).expect("index");
        let entries: Vec<(String, Vec<f32>)> = (0..=ENTRY_LIMIT)
            .map(|position| (format!("note-{position}"), vec![1.0]))
            .collect();
        expect_code(import.upsert(index, entries), ErrorCode::QuotaExceeded);
    }

    /// REQ: API-002
    #[test]
    fn remove_entries_unknown_index_is_not_found() {
        let mut import = VectorImport;
        let result = import.remove_entries(IndexId(u32::MAX), vec!["note-1".to_string()]);
        expect_code(result, ErrorCode::NotFound);
    }

    /// REQ: API-001
    #[test]
    fn remove_entries_ignores_id_not_in_index() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 2).expect("index");
        let result = import.remove_entries(index, vec!["missing".to_string()]);
        assert!(result.is_ok(), "an id that is not in the index is ignored");
    }

    /// REQ: API-001
    #[test]
    fn top_k_returns_nearest_entry_first_with_cosine_score() {
        let mut import = VectorImport;
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
        assert_eq!(hits[0].0, "a", "the entry in the same direction ranks first");
        assert!((hits[0].1 - 1.0).abs() < 1e-6, "cosine of identical vectors is 1.0");
        assert!(hits[0].1 >= hits[1].1, "scores are highest first");
    }

    /// REQ: API-061
    #[test]
    fn top_k_with_zero_k_is_invalid_argument() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(import.top_k(index, vec![1.0, 0.0], 0), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn top_k_accepts_k_of_exactly_100() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 2).expect("index");
        let result = import.top_k(index, vec![1.0, 0.0], TOP_K_LIMIT);
        assert!(result.is_ok(), "k = 100 is the largest accepted value");
    }

    /// REQ: API-061
    #[test]
    fn top_k_with_k_above_100_is_invalid_argument() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(
            import.top_k(index, vec![1.0, 0.0], TOP_K_LIMIT + 1),
            ErrorCode::InvalidArgument,
        );
    }

    /// REQ: API-061
    #[test]
    fn top_k_query_with_wrong_dimension_is_invalid_argument() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(import.top_k(index, vec![1.0, 0.0, 0.0], 1), ErrorCode::InvalidArgument);
    }

    /// REQ: API-061
    #[test]
    fn top_k_empty_query_is_invalid_argument() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 2).expect("index");
        expect_code(import.top_k(index, Vec::new(), 1), ErrorCode::InvalidArgument);
    }

    /// REQ: API-002
    #[test]
    fn top_k_unknown_index_is_not_found() {
        let mut import = VectorImport;
        expect_code(
            import.top_k(IndexId(u32::MAX), vec![1.0, 0.0], 1),
            ErrorCode::NotFound,
        );
    }

    /// REQ: API-002
    #[test]
    fn drop_index_of_unknown_id_is_ignored() {
        let mut import = VectorImport;
        import.drop_index(IndexId(u32::MAX));
    }

    /// REQ: API-002
    #[test]
    fn drop_index_twice_is_ignored_the_second_time() {
        let mut import = VectorImport;
        let index = import.create_index("notes".to_string(), 2).expect("index");
        import.drop_index(index);
        import.drop_index(index);
    }
}
