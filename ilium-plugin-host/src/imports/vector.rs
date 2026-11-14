//! Host implementation of `ilium:host/vector`: embedding requests and host-held vector indexes.
//!
//! `embed` hands the texts to the embedding call of the [`InferenceBackend`] the process composed
//! (`wiring::wave_two::Backends::inference`), on a worker thread of this instance, and queues
//! `request-completed` when it answers; the plugin collects the vectors once with `take-vectors`.
//! Without a backend, or with a provider that offers no embeddings, the request completes
//! `unsupported`, so it never hangs. The index functions are answered synchronously from memory
//! and, when the instance has a store, written through to the plugin's own index folder
//! ([`super::vector_store`]) so the next instance finds them again (manual page `api-vector`).
//!
//! Every pending request and every index belongs to the import of one instance: another plugin
//! cannot take its vectors or reach its indexes. Dropping the import sets the cancel flag, which
//! lets waiting workers end without calling the backend.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ilium_plugin_api::generated::host::vector::HostVector;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::{IndexId, RequestId};
use ilium_plugin_api::generated::types::Outcome;

use super::env::{completion_event, CancelOnDrop, ImportEnv, WorkerGate};
use super::inference::InferenceBackend;
use super::vector_store::{StoredIndex, VectorStore};

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
/// Embedding requests one instance may hold before `embed` answers `busy`.
const PENDING_REQUEST_LIMIT: usize = 64;
/// Backend calls one instance runs at once; later requests wait for a free slot.
const CONCURRENT_CALLS: usize = 2;
/// The grant `embed` needs; the index functions need no more than the interface itself.
const EMBED_GRANT: &str = "inference.embed";

/// State of one embedding request.
#[derive(Debug)]
enum Held {
    /// Accepted, the provider has not answered yet.
    Pending,
    /// Answered, waiting for one `take-vectors`.
    Completed(Vec<Vec<f32>>),
}

fn lock<T>(state: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every writer does one insert or remove, so the state is consistent after a panic.
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Host side of the `vector` interface of one plugin instance. Cloning shares the pending
/// requests (a harness keeps one instance and hands out clones); the workers stop when the last
/// clone drops.
pub struct VectorImport {
    env: ImportEnv,
    backend: Option<Arc<dyn InferenceBackend>>,
    requests: Arc<Mutex<BTreeMap<u64, Held>>>,
    gate: WorkerGate,
    life: Arc<CancelOnDrop>,
    store: Option<VectorStore>,
    index_limit: usize,
    entry_limit: usize,
    next_index: u32,
    indexes: BTreeMap<u32, StoredIndex>,
}

impl std::fmt::Debug for VectorImport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VectorImport")
            .field("plugin", &self.env.plugin())
            .field("has_backend", &self.backend.is_some())
            .field("persisted", &self.store.is_some())
            .field("indexes", &self.indexes.len())
            .finish_non_exhaustive()
    }
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

/// Checks the provider's answer: one vector per text, all of one non-zero length.
fn check_vectors(texts: usize, vectors: &[Vec<f32>]) -> Result<(), PluginError> {
    if vectors.len() != texts {
        return Err(error(
            ErrorCode::Internal,
            "the provider answered with a different number of vectors than texts",
        ));
    }
    let Some(first) = vectors.first() else {
        return Ok(());
    };
    if first.is_empty() || vectors.iter().any(|vector| vector.len() != first.len()) {
        return Err(error(
            ErrorCode::Internal,
            "the provider answered with vectors of different or zero length",
        ));
    }
    Ok(())
}

impl VectorImport {
    /// An import with explicit quota limits (`vector_indexes`, `vector_entries`), no provider and
    /// no store: `embed` completes `unsupported` and indexes live only as long as the import.
    pub fn with_limits(index_limit: usize, entry_limit: usize) -> Self {
        Self {
            env: ImportEnv::unrestricted(),
            backend: None,
            requests: Arc::default(),
            gate: WorkerGate::new(CONCURRENT_CALLS),
            life: Arc::new(CancelOnDrop::default()),
            store: None,
            index_limit,
            entry_limit,
            next_index: 1,
            indexes: BTreeMap::new(),
        }
    }

    /// The import of the instance `env` describes. `backend` answers `embed`; `store_folder`, when
    /// given, is the plugin's own index folder: the indexes saved there are loaded now and every
    /// change is written back.
    pub fn new(
        env: ImportEnv,
        backend: Option<Arc<dyn InferenceBackend>>,
        store_folder: Option<PathBuf>,
    ) -> Self {
        let mut import = Self::with_limits(DEFAULT_INDEX_LIMIT, DEFAULT_ENTRY_LIMIT);
        import.env = env;
        import.backend = backend;
        if let Some(folder) = store_folder {
            let store = VectorStore::new(folder);
            import.indexes = store.load();
            import.next_index = import
                .indexes
                .keys()
                .next_back()
                .map_or(1, |last| last.saturating_add(1));
            import.store = Some(store);
        }
        import
    }

    /// Delivers vectors for a pending `request` from outside the backend (a host that answers
    /// embeddings itself) and queues `request-completed`. Returns false for a request that is not
    /// pending.
    pub fn complete_embedding(&mut self, request: RequestId, vectors: Vec<Vec<f32>>) -> bool {
        {
            let mut requests = lock(&self.requests);
            if !matches!(requests.get(&request.0), Some(Held::Pending)) {
                return false;
            }
            requests.insert(request.0, Held::Completed(vectors));
        }
        self.env.complete(request, Outcome::Ok);
        true
    }

    fn total_entries(&self) -> usize {
        self.indexes.values().map(|index| index.entries.len()).sum()
    }

    /// Writes one index through to the store; an import without a store keeps it in memory.
    fn persist(&self, id: u32) -> Result<(), PluginError> {
        let (Some(store), Some(index)) = (&self.store, self.indexes.get(&id)) else {
            return Ok(());
        };
        store.save(id, index).map_err(|failure| {
            PluginError::new(
                ErrorCode::Io,
                format!("the vector index could not be saved: {failure}"),
            )
        })
    }

    /// Runs the backend's embedding call on a worker of this instance and completes `id`.
    fn dispatch(&self, id: RequestId, texts: Vec<String>, purpose: String) {
        let requests = Arc::clone(&self.requests);
        let sink = self.env.sink();
        let guard = sink.worker();
        let gate = self.gate.clone();
        let cancel = self.life.flag();
        let backend = self.backend.clone();
        let spawned = std::thread::Builder::new()
            .name("plugin-vector-embed".to_owned())
            .spawn(move || {
                let _guard = guard;
                let answer = match &backend {
                    None => Err(error(
                        ErrorCode::Unsupported,
                        "no inference provider is configured on this side",
                    )),
                    Some(backend) => {
                        let Some(_slot) = gate.enter(&cancel) else {
                            return;
                        };
                        if cancel.is_set() {
                            return;
                        }
                        backend
                            .embed(&texts, &purpose, &cancel)
                            .and_then(|vectors| {
                                check_vectors(texts.len(), &vectors).map(|()| vectors)
                            })
                    }
                };
                // An ended instance takes nothing; its requests are dropped with it.
                if cancel.is_set() {
                    return;
                }
                let outcome = {
                    let mut held = lock(&requests);
                    match answer {
                        Ok(vectors) => {
                            held.insert(id.0, Held::Completed(vectors));
                            Outcome::Ok
                        }
                        Err(failure) => {
                            held.remove(&id.0);
                            if failure.code == ErrorCode::Cancelled {
                                Outcome::Cancelled
                            } else {
                                Outcome::Failed(failure)
                            }
                        }
                    }
                };
                sink.send(completion_event(id, outcome));
            });
        if spawned.is_err() {
            lock(&self.requests).remove(&id.0);
            self.env.complete(
                id,
                Outcome::Failed(error(
                    ErrorCode::Internal,
                    "the embedding worker did not start",
                )),
            );
        }
    }
}

impl HostVector for VectorImport {
    fn embed(&mut self, texts: Vec<String>, purpose: String) -> Result<RequestId, PluginError> {
        if !self.env.holds(EMBED_GRANT) {
            return Err(error(
                ErrorCode::NotPermitted,
                "inference.embed is not granted",
            ));
        }
        if texts.is_empty() {
            return Err(error(
                ErrorCode::InvalidArgument,
                "embed needs at least one text",
            ));
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
        let request = {
            let mut requests = lock(&self.requests);
            if requests.len() >= PENDING_REQUEST_LIMIT {
                return Err(error(
                    ErrorCode::Busy,
                    "64 embedding requests are already pending",
                ));
            }
            let request = self.env.next_request();
            requests.insert(request.0, Held::Pending);
            request
        };
        self.dispatch(request, texts, purpose);
        Ok(request)
    }

    fn take_vectors(&mut self, request: RequestId) -> Option<Vec<Vec<f32>>> {
        let mut requests = lock(&self.requests);
        if !matches!(requests.get(&request.0), Some(Held::Completed(_))) {
            return None;
        }
        match requests.remove(&request.0) {
            Some(Held::Completed(vectors)) => Some(vectors),
            _ => None,
        }
    }

    fn create_index(&mut self, name: String, dimensions: u16) -> Result<IndexId, PluginError> {
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
            StoredIndex {
                name,
                dimensions: usize::from(dimensions),
                entries: BTreeMap::new(),
            },
        );
        if let Err(failure) = self.persist(id) {
            // An index that cannot be saved is not created: the next instance would not find it.
            self.indexes.remove(&id);
            return Err(failure);
        }
        Ok(IndexId(id))
    }

    fn upsert(
        &mut self,
        index: IndexId,
        entries: Vec<(String, Vec<f32>)>,
    ) -> Result<(), PluginError> {
        let held = self
            .indexes
            .get(&index.0)
            .ok_or_else(|| error(ErrorCode::NotFound, "unknown vector index"))?;
        if entries
            .iter()
            .any(|(_, vector)| vector.len() != held.dimensions)
        {
            return Err(error(
                ErrorCode::InvalidArgument,
                "a vector length differs from the index dimensions",
            ));
        }
        let new_ids: std::collections::BTreeSet<&String> = entries
            .iter()
            .map(|(id, _)| id)
            .filter(|id| !held.entries.contains_key(*id))
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
        self.persist(index.0)
    }

    fn remove_entries(&mut self, index: IndexId, ids: Vec<String>) -> Result<(), PluginError> {
        let held = self
            .indexes
            .get_mut(&index.0)
            .ok_or_else(|| error(ErrorCode::NotFound, "unknown vector index"))?;
        let mut removed = false;
        for id in ids {
            removed |= held.entries.remove(&id).is_some();
        }
        if removed {
            self.persist(index.0)?;
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
        if self.indexes.remove(&index.0).is_none() {
            return;
        }
        if let Some(store) = &self.store {
            // `drop-index` has no error result. A file that cannot be removed comes back with
            // the next instance, which is the safe direction: no data is lost silently.
            let _ = store.remove(index.0);
        }
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

    use super::super::env::{CancelFlag, ImportEnv};
    use super::super::inference::InferenceBackend;
    use ilium_plugin_api::generated::host::inference::{Completion, CompletionRequest};
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::requests::{Outcome, RequestRef};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// Embeds each text as `[length, 1.0]`.
    struct LengthEmbedder;

    impl InferenceBackend for LengthEmbedder {
        fn complete(
            &self,
            _request: &CompletionRequest,
            _cancel: &CancelFlag,
        ) -> Result<Completion, PluginError> {
            Err(PluginError::new(ErrorCode::Unsupported, "embeddings only"))
        }

        fn models(&self, _cancel: &CancelFlag) -> Result<Vec<String>, PluginError> {
            Ok(Vec::new())
        }

        fn embed(
            &self,
            texts: &[String],
            _purpose: &str,
            _cancel: &CancelFlag,
        ) -> Result<Vec<Vec<f32>>, PluginError> {
            Ok(texts
                .iter()
                .map(|text| vec![text.len() as f32, 1.0])
                .collect())
        }
    }

    /// Waits for the `request-completed` of `request` on the queue of `env`.
    fn outcome_of(env: &ImportEnv, request: RequestId) -> Outcome {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let found = env
                .events()
                .drain()
                .into_iter()
                .find_map(|queued| match queued.event {
                    Event::RequestCompleted(result)
                        if result.request == RequestRef::Call(request) =>
                    {
                        Some(result.outcome)
                    }
                    _ => None,
                });
            if let Some(outcome) = found {
                return outcome;
            }
            assert!(Instant::now() < deadline, "embed never completed");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// REQ: API-001
    #[test]
    fn embed_completes_through_the_backend_and_the_vectors_are_taken_once() {
        let env = ImportEnv::unrestricted();
        let mut import = VectorImport::new(env.clone(), Some(Arc::new(LengthEmbedder)), None);
        let request = import
            .embed(
                vec!["ab".to_string(), "abcd".to_string()],
                "note search".to_string(),
            )
            .expect("accepted");
        assert_eq!(outcome_of(&env, request), Outcome::Ok);
        assert_eq!(
            import.take_vectors(request),
            Some(vec![vec![2.0, 1.0], vec![4.0, 1.0]]),
            "one vector per text, in text order"
        );
        assert_eq!(import.take_vectors(request), None, "taken only once");
    }

    /// REQ: API-001
    #[test]
    fn embed_without_a_provider_completes_unsupported_instead_of_hanging() {
        let env = ImportEnv::unrestricted();
        let mut import = VectorImport::new(env.clone(), None, None);
        let request = import
            .embed(vec!["hello".to_string()], "note search".to_string())
            .expect("accepted");
        match outcome_of(&env, request) {
            Outcome::Failed(failure) => assert_eq!(failure.code, ErrorCode::Unsupported),
            other => panic!("expected unsupported, got {other:?}"),
        }
        assert_eq!(import.take_vectors(request), None);
    }

    /// REQ: API-001
    #[test]
    fn embed_without_the_inference_embed_grant_is_not_permitted() {
        let env = ImportEnv::unrestricted().with_grants(vec!["inference".to_string()]);
        let mut import = VectorImport::new(env, Some(Arc::new(LengthEmbedder)), None);
        expect_code(
            import.embed(vec!["hello".to_string()], "note search".to_string()),
            ErrorCode::NotPermitted,
        );
    }

    /// REQ: API-001
    #[test]
    fn another_instance_cannot_take_the_vectors_of_a_request() {
        let env = ImportEnv::unrestricted();
        let mut owner = VectorImport::new(env.clone(), Some(Arc::new(LengthEmbedder)), None);
        let mut other = VectorImport::new(
            env.clone().with_identity("other.plugin", 2),
            Some(Arc::new(LengthEmbedder)),
            None,
        );
        let request = owner
            .embed(vec!["abc".to_string()], "note search".to_string())
            .expect("accepted");
        assert_eq!(outcome_of(&env, request), Outcome::Ok);
        assert_eq!(other.take_vectors(request), None);
        assert!(owner.take_vectors(request).is_some());
    }

    /// REQ: API-001
    #[test]
    fn indexes_persist_in_the_store_folder_for_the_next_instance() {
        let scratch = tempfile::tempdir().expect("scratch folder");
        let folder = scratch.path().join("vector").join("example.notes");
        let index = {
            let mut first =
                VectorImport::new(ImportEnv::unrestricted(), None, Some(folder.clone()));
            let index = first.create_index("notes".to_string(), 2).expect("index");
            first
                .upsert(
                    index,
                    vec![
                        ("a".to_string(), vec![1.0, 0.0]),
                        ("b".to_string(), vec![0.0, 1.0]),
                    ],
                )
                .expect("upsert");
            first
                .remove_entries(index, vec!["b".to_string()])
                .expect("remove");
            index
        };
        let mut second = VectorImport::new(ImportEnv::unrestricted(), None, Some(folder.clone()));
        let hits = second.top_k(index, vec![1.0, 0.0], 5).expect("top-k");
        assert_eq!(hits.len(), 1, "the removed entry stays removed");
        assert_eq!(hits[0].0, "a");
        let fresh = second.create_index("more".to_string(), 3).expect("index");
        assert_ne!(fresh, index, "a reloaded store never reuses an index id");
        second.drop_index(index);
        let mut third = VectorImport::new(ImportEnv::unrestricted(), None, Some(folder));
        expect_code(third.top_k(index, vec![1.0, 0.0], 1), ErrorCode::NotFound);
    }
}
