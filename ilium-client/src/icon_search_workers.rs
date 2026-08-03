//! Owned, CPU-only semantic search for the icon catalogue.
//!
//! The worker owns the ONNX embedding model and its one vector per icon, so
//! neither model startup nor 12,000-vector ranking can ever block the TUI.
//! It intentionally uses no lexical candidate pass, database, GPU, or remote
//! inference service: every non-empty picker query is embedded and compared
//! directly against the complete in-memory dense-vector index.

use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use ilium_platform::thread_priority::{lower_current_thread, WorkerPriority};
use tokio::sync::mpsc::Sender as AsyncSender;

use crate::icon_settings::{
    icon_categories, semantic_picker_search_results, IconCatalogEntry, IconCatalogFamily,
    IconPickerSearchResults, IconSemanticSearchHit,
};

const MAX_RESULTS_PER_FAMILY: usize = 120;
const CATALOGUE_EMBEDDING_BATCH_SIZE: usize = 32;
/// Four CPU execution threads keep the one-time catalogue build practical
/// while leaving the rest of a typical development machine available to the
/// terminal and editor. Query vectors themselves are tiny after that build.
const CPU_THREADS: usize = 4;
// Bumped from ILICV001 because the on-disk header gained the catalogue
// fingerprint field below -- old-format files fail this check and are
// safely rebuilt rather than misread with the fingerprint bytes offset.
const INDEX_MAGIC: [u8; 8] = *b"ILICV002";

#[derive(Debug, Clone)]
pub struct IconSearchRequest {
    pub revision: u64,
    pub query: String,
}

#[derive(Debug)]
pub enum IconSemanticSearchEvent {
    Results {
        revision: u64,
        results: IconPickerSearchResults,
    },
    Failed {
        revision: u64,
        message: String,
    },
}

/// Keeps the one long-lived worker thread owned until the client exits.
pub struct IconSearchWorkers {
    requests_tx: Sender<IconSearchRequest>,
    handle: Option<JoinHandle<()>>,
}

impl IconSearchWorkers {
    pub fn new(events_tx: AsyncSender<IconSemanticSearchEvent>) -> Self {
        let (requests_tx, requests_rx) = mpsc::channel();
        // Thread creation is a fallible OS call (e.g. the process is already
        // at its thread-count limit), not a "cannot fail" invariant, and the
        // icon picker's semantic search is a supplementary feature -- so
        // degrade to "search silently reports unavailable" the same way
        // `SearchWorkers::start` propagates its own `thread::Builder::spawn`
        // failure, instead of taking the whole client down over it. When
        // spawn fails, `requests_rx` is dropped along with the unrun
        // closure, so every future `request()` send fails immediately and
        // is logged there.
        let handle = match thread::Builder::new()
            .name("ilium-icon-semantic-search".to_string())
            .spawn(move || run_worker(requests_rx, events_tx))
        {
            Ok(handle) => Some(handle),
            Err(error) => {
                tracing::error!(%error, "could not start the icon semantic search worker thread");
                None
            }
        };
        Self {
            requests_tx,
            handle,
        }
    }

    pub fn request(&mut self, request: IconSearchRequest) {
        if self.requests_tx.send(request).is_err() {
            tracing::error!("icon semantic search worker stopped unexpectedly");
        }
    }
}

impl Drop for IconSearchWorkers {
    fn drop(&mut self) {
        // Closing the request channel is the worker's explicit shutdown
        // signal. Model inference cannot be safely interrupted, so terminal
        // restoration never waits for it to finish.
        let (replacement_tx, _) = mpsc::channel();
        let old_tx = std::mem::replace(&mut self.requests_tx, replacement_tx);
        drop(old_tx);
        drop(self.handle.take());
    }
}

struct IndexedIcon {
    category_label: &'static str,
    family: IconCatalogFamily,
    entry: IconCatalogEntry,
    normalized_embedding: Vec<f32>,
}

struct IconSemanticIndex {
    model: TextEmbedding,
    icons: Vec<IndexedIcon>,
}

fn run_worker(
    requests_rx: Receiver<IconSearchRequest>,
    events_tx: AsyncSender<IconSemanticSearchEvent>,
) {
    lower_current_thread(WorkerPriority::Lowest);
    let mut index: Option<IconSemanticIndex> = None;
    while let Ok(mut request) = requests_rx.recv() {
        // Once the index is available, collapse a typing burst to its newest
        // query before spending CPU on an embedding that cannot become visible.
        while let Ok(newer_request) = requests_rx.try_recv() {
            request = newer_request;
        }
        if index.is_none() {
            tracing::info!(
                model = ?EmbeddingModel::AllMiniLML6V2,
                cache_directory = %icon_model_cache_dir().display(),
                "icon semantic model initialization started"
            );
            match build_index() {
                Ok(new_index) => {
                    tracing::info!(
                        indexed_icons = new_index.icons.len(),
                        "icon semantic model initialization completed"
                    );
                    index = Some(new_index);
                }
                Err(message) => {
                    // Initialization can include a dependency-owned model download,
                    // so preserve its complete diagnostic even though the dependency
                    // does not expose the individual HTTP exchange to this adapter.
                    tracing::error!(%message, "icon semantic model initialization failed");
                    let _ = events_tx.blocking_send(IconSemanticSearchEvent::Failed {
                        revision: request.revision,
                        message,
                    });
                    continue;
                }
            }
        }
        let Some(index) = index.as_mut() else {
            // Every path that reaches here either already had `index` set
            // from a prior iteration or just set it in the `Ok` arm above
            // (the `Err` arm `continue`s before falling through) -- but
            // logging and retrying on the next request is strictly safer
            // than a panic if that invariant is ever violated by a future
            // edit to the match above.
            tracing::error!("icon semantic index unexpectedly empty after initialization");
            continue;
        };
        match index.search(&request.query) {
            Ok(results) => {
                let _ = events_tx.blocking_send(IconSemanticSearchEvent::Results {
                    revision: request.revision,
                    results,
                });
            }
            Err(message) => {
                tracing::error!(
                    revision = request.revision,
                    query = %request.query,
                    %message,
                    "icon semantic search failed"
                );
                let _ = events_tx.blocking_send(IconSemanticSearchEvent::Failed {
                    revision: request.revision,
                    message,
                });
            }
        }
    }
}

fn build_index() -> Result<IconSemanticIndex, String> {
    let cache_dir = icon_model_cache_dir();
    std::fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("could not create icon model cache: {error}"))?;
    let model_info = TextEmbedding::get_model_info(&EmbeddingModel::AllMiniLML6V2)
        .map_err(|error| format!("could not resolve icon embedding model metadata: {error}"))?;
    let model_endpoint =
        std::env::var("HF_ENDPOINT").unwrap_or_else(|_| "https://huggingface.co".to_owned());
    let diagnostic_endpoint = ilium_logging::redacted_url(&model_endpoint);
    tracing::info!(
        method = "GET",
        url = %diagnostic_endpoint,
        model_repository = %model_info.model_code,
        model_file = %model_info.model_file,
        cache_directory = %cache_dir.join("model").display(),
        "HTTP model artifact resolution started; FastEmbed may satisfy files from its local cache"
    );
    let mut model = TextEmbedding::try_new(
        // The standard MiniLM graph supports bounded batching, keeping the
        // first catalogue build practical on CPU-only machines.
        TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(cache_dir.join("model"))
            .with_intra_threads(CPU_THREADS)
            .with_show_download_progress(false),
    )
    .map_err(|error| format!("could not load local icon embedding model: {error}"))?;
    tracing::info!(
        method = "GET",
        url = %diagnostic_endpoint,
        model_repository = %model_info.model_code,
        "HTTP model artifact resolution completed"
    );
    let metadata = icon_categories()
        .iter()
        .flat_map(|category| {
            category
                .entries
                .iter()
                .copied()
                .map(move |entry| (category.label, category.family, entry))
        })
        .collect::<Vec<_>>();
    // The exact text handed to the embedding model, in catalogue order. This
    // is also the fingerprint's input below: any renamed/reordered/swapped
    // icon that changes what gets embedded, while leaving the entry count
    // unchanged, must still invalidate the durable cache rather than being
    // silently zipped onto stale vectors for the wrong icons.
    let documents = metadata
        .iter()
        .map(|(category, family, entry)| semantic_document(category, *family, *entry))
        .collect::<Vec<_>>();
    let cache_fingerprint = catalogue_fingerprint(model_info, &documents);
    let index_path = cache_dir.join("catalogue-v1.f32");
    let embeddings = match load_cached_embeddings(&index_path, metadata.len(), cache_fingerprint) {
        Some(embeddings) => embeddings,
        None => {
            let mut embeddings = Vec::with_capacity(metadata.len());
            for batch in documents.chunks(CATALOGUE_EMBEDDING_BATCH_SIZE) {
                let batch_embeddings = model
                    .embed(batch, Some(CATALOGUE_EMBEDDING_BATCH_SIZE))
                    .map_err(|error| format!("could not embed the icon catalogue: {error}"))?;
                if batch_embeddings.len() != batch.len() {
                    return Err(
                        "the icon embedding model returned an incomplete catalogue batch"
                            .to_string(),
                    );
                }
                embeddings.extend(batch_embeddings);
            }
            if embeddings.len() != metadata.len() {
                return Err(
                    "the icon embedding model returned an incomplete catalogue index".to_string(),
                );
            }
            persist_embeddings(&index_path, &embeddings, cache_fingerprint)?;
            embeddings
        }
    };
    let icons = metadata
        .into_iter()
        .zip(embeddings)
        .map(|((category_label, family, entry), embedding)| IndexedIcon {
            category_label,
            family,
            entry,
            normalized_embedding: normalize(embedding),
        })
        .collect();
    Ok(IconSemanticIndex { model, icons })
}

/// Ties the durable vector cache to both the exact catalogue text that was
/// embedded and the model that embedded it. Cardinality alone is a weak
/// cache key: a renamed, reordered, or swapped icon that keeps the same
/// total count -- or a fastembed upgrade that changes the model's output
/// dimension -- would otherwise pass a count-only check and get silently
/// zipped onto stale vectors for the wrong icons, or fed into `dot_product`
/// alongside a differently-sized query vector with no error anywhere.
fn catalogue_fingerprint(
    model_info: &fastembed::ModelInfo<EmbeddingModel>,
    documents: &[String],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    model_info.model_code.hash(&mut hasher);
    model_info.model_file.hash(&mut hasher);
    model_info.dim.hash(&mut hasher);
    documents.hash(&mut hasher);
    hasher.finish()
}

/// Loads the durable vector matrix only when it is complete, has the current
/// format marker, and matches this exact generated catalogue's cardinality
/// and fingerprint (see `catalogue_fingerprint`). Any mismatch simply
/// rebuilds safely from the source catalogue.
fn load_cached_embeddings(
    path: &std::path::Path,
    expected_count: usize,
    expected_fingerprint: u64,
) -> Option<Vec<Vec<f32>>> {
    let mut file = BufReader::new(std::fs::File::open(path).ok()?);
    let mut magic = [0_u8; INDEX_MAGIC.len()];
    file.read_exact(&mut magic).ok()?;
    if magic != INDEX_MAGIC {
        return None;
    }
    let count = read_u32(&mut file)? as usize;
    let dimensions = read_u32(&mut file)? as usize;
    let fingerprint = read_u64(&mut file)?;
    if count != expected_count
        || fingerprint != expected_fingerprint
        || dimensions == 0
        || dimensions > 4096
    {
        return None;
    }
    // One sized read for the whole payload instead of one syscall per
    // float -- a full catalogue is tens of thousands of vectors, and this
    // cache's entire purpose is to make startup fast.
    let mut payload = vec![0_u8; count * dimensions * 4];
    file.read_exact(&mut payload).ok()?;
    let mut embeddings = Vec::with_capacity(count);
    for chunk in payload.chunks_exact(dimensions * 4) {
        let embedding = chunk
            .chunks_exact(4)
            .map(|bytes| {
                // `chunks_exact(4)` guarantees exactly 4 bytes per item.
                f32::from_le_bytes(bytes.try_into().expect("chunk is exactly 4 bytes"))
            })
            .collect();
        embeddings.push(embedding);
    }
    Some(embeddings)
}

fn persist_embeddings(
    path: &std::path::Path,
    embeddings: &[Vec<f32>],
    fingerprint: u64,
) -> Result<(), String> {
    let dimensions = embeddings.first().map_or(0, Vec::len);
    if dimensions == 0
        || embeddings
            .iter()
            .any(|embedding| embedding.len() != dimensions)
    {
        return Err("the icon embedding model returned inconsistent vector dimensions".to_string());
    }
    // Multiple Ilium clients can start before the first shared index exists.
    // Give each writer its own staging file; `rename` then publishes one
    // complete deterministic matrix without concurrent writers corrupting
    // each other's partial output.
    let temporary_path = path.with_extension(format!("f32.{}.tmp", std::process::id()));
    let result = write_embeddings_file(&temporary_path, embeddings, dimensions, fingerprint)
        .and_then(|()| {
            std::fs::rename(&temporary_path, path)
                .map_err(|error| format!("could not publish icon vector cache: {error}"))
        });
    if result.is_err() {
        // The staging file is per-PID (see above), so a write failure that
        // is not cleaned up here accumulates one orphaned ~18MB file per
        // failed attempt instead of leaving the cache directory as it was
        // found. Best-effort: a failure to remove it does not change the
        // outcome already carried by `result`.
        let _ = std::fs::remove_file(&temporary_path);
    }
    result
}

/// Writes the header and full vector payload as two sized `write_all` calls
/// through a `BufWriter`, rather than one syscall per 4-byte float, then
/// flushes and syncs before returning so `persist_embeddings`'s rename
/// always publishes durably-written bytes.
fn write_embeddings_file(
    temporary_path: &std::path::Path,
    embeddings: &[Vec<f32>],
    dimensions: usize,
    fingerprint: u64,
) -> Result<(), String> {
    let file = std::fs::File::create(temporary_path)
        .map_err(|error| format!("could not create icon vector cache: {error}"))?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(&INDEX_MAGIC)
        .and_then(|()| writer.write_all(&(embeddings.len() as u32).to_le_bytes()))
        .and_then(|()| writer.write_all(&(dimensions as u32).to_le_bytes()))
        .and_then(|()| writer.write_all(&fingerprint.to_le_bytes()))
        .map_err(|error| format!("could not write icon vector cache header: {error}"))?;
    let mut payload = Vec::with_capacity(embeddings.len() * dimensions * 4);
    for embedding in embeddings {
        for value in embedding {
            payload.extend_from_slice(&value.to_le_bytes());
        }
    }
    writer
        .write_all(&payload)
        .map_err(|error| format!("could not write icon vector cache: {error}"))?;
    // `sync_all` on the file handle only durably persists bytes the OS
    // already has; a `BufWriter` holds unflushed bytes in userspace, and its
    // drop-time flush swallows errors, so flush explicitly and check it
    // before syncing.
    writer
        .flush()
        .map_err(|error| format!("could not flush icon vector cache: {error}"))?;
    writer
        .into_inner()
        .map_err(|error| format!("could not finalize icon vector cache: {error}"))?
        .sync_all()
        .map_err(|error| format!("could not finalize icon vector cache: {error}"))
}

fn read_u32<R: Read>(reader: &mut R) -> Option<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> Option<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

impl IconSemanticIndex {
    fn search(&mut self, query: &str) -> Result<IconPickerSearchResults, String> {
        let query_embedding = self
            .model
            .embed(vec![format!("query: {query}")], Some(1))
            .map_err(|error| format!("could not embed icon search text: {error}"))?
            .into_iter()
            .next()
            .ok_or_else(|| "the icon embedding model returned no query vector".to_string())?;
        let query_embedding = normalize(query_embedding);
        let mut official_hits = Vec::new();
        let mut nerd_font_hits = Vec::new();
        for indexed_icon in &self.icons {
            let score = dot_product(&query_embedding, &indexed_icon.normalized_embedding);
            let target = match indexed_icon.family {
                IconCatalogFamily::OfficialUtf8 => &mut official_hits,
                IconCatalogFamily::NerdFont => &mut nerd_font_hits,
            };
            target.push((score, indexed_icon));
        }
        let mut hits = ranked_hits(official_hits);
        hits.extend(ranked_hits(nerd_font_hits));
        Ok(semantic_picker_search_results(hits))
    }
}

fn ranked_hits(mut scored: Vec<(f32, &IndexedIcon)>) -> Vec<IconSemanticSearchHit> {
    scored.sort_unstable_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(Ordering::Equal));
    scored
        .into_iter()
        .take(MAX_RESULTS_PER_FAMILY)
        .map(|(_, indexed_icon)| IconSemanticSearchHit {
            category_label: indexed_icon.category_label,
            family: indexed_icon.family,
            entry: indexed_icon.entry,
        })
        .collect()
}

fn semantic_document(category: &str, family: IconCatalogFamily, entry: IconCatalogEntry) -> String {
    let family_description = match family {
        IconCatalogFamily::OfficialUtf8 => "portable official Unicode UTF-8 emoji symbol",
        IconCatalogFamily::NerdFont => "Nerd Font developer private-use icon",
    };
    format!(
        "passage: icon name {name}. category {category}. family {family_description}. visual symbol for {name}.",
        name = entry.name,
    )
}

fn normalize(mut embedding: Vec<f32>) -> Vec<f32> {
    let magnitude = embedding
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if magnitude > f32::EPSILON {
        for value in &mut embedding {
            *value /= magnitude;
        }
    }
    embedding
}

fn dot_product(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| left * right)
        .sum()
}

fn icon_model_cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "ilium")
        .map(|directories| directories.cache_dir().join("icon-embeddings"))
        .unwrap_or_else(|| std::env::temp_dir().join("ilium-icon-embeddings"))
}

#[cfg(test)]
mod tests {
    use super::{load_cached_embeddings, persist_embeddings};

    #[test]
    fn vector_cache_round_trips_one_dense_vector_per_catalogue_entry() {
        let temporary_dir = tempfile::tempdir().expect("temporary vector-cache directory");
        let cache_path = temporary_dir.path().join("catalogue-v1.f32");
        let embeddings = vec![vec![0.25, -0.5, 1.0], vec![0.0, 0.125, -0.25]];
        let fingerprint = 0xC0FF_EE00_1234_5678;

        persist_embeddings(&cache_path, &embeddings, fingerprint).expect("persist vector matrix");

        assert_eq!(
            load_cached_embeddings(&cache_path, embeddings.len(), fingerprint),
            Some(embeddings)
        );
        assert!(load_cached_embeddings(&cache_path, 3, fingerprint).is_none());
        assert!(load_cached_embeddings(&cache_path, 2, fingerprint.wrapping_add(1)).is_none());
    }
}
