//! Owned, CPU-only semantic search for the icon catalogue.
//!
//! The worker owns the ONNX embedding model and its one vector per icon, so
//! neither model startup nor 12,000-vector ranking can ever block the TUI.
//! It intentionally uses no lexical candidate pass, database, GPU, or remote
//! inference service: every non-empty picker query is embedded and compared
//! directly against the complete in-memory dense-vector index.

use std::cmp::Ordering;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
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
const INDEX_MAGIC: [u8; 8] = *b"ILICV001";

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
        let handle = thread::Builder::new()
            .name("ilium-icon-semantic-search".to_string())
            .spawn(move || run_worker(requests_rx, events_tx))
            .expect("the icon search worker thread must start");
        Self {
            requests_tx,
            handle: Some(handle),
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
    crate::background_priority::lower_current_thread();
    let mut index: Option<IconSemanticIndex> = None;
    while let Ok(mut request) = requests_rx.recv() {
        // Once the index is available, collapse a typing burst to its newest
        // query before spending CPU on an embedding that cannot become visible.
        while let Ok(newer_request) = requests_rx.try_recv() {
            request = newer_request;
        }
        if index.is_none() {
            match build_index() {
                Ok(new_index) => index = Some(new_index),
                Err(message) => {
                    let _ = events_tx.blocking_send(IconSemanticSearchEvent::Failed {
                        revision: request.revision,
                        message,
                    });
                    continue;
                }
            }
        }
        let Some(index) = index.as_mut() else {
            unreachable!("a successful icon index initialization must populate the index");
        };
        match index.search(&request.query) {
            Ok(results) => {
                let _ = events_tx.blocking_send(IconSemanticSearchEvent::Results {
                    revision: request.revision,
                    results,
                });
            }
            Err(message) => {
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
    let mut model = TextEmbedding::try_new(
        // The standard MiniLM graph supports bounded batching, keeping the
        // first catalogue build practical on CPU-only machines.
        TextInitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(cache_dir.join("model"))
            .with_intra_threads(CPU_THREADS)
            .with_show_download_progress(false),
    )
    .map_err(|error| format!("could not load local icon embedding model: {error}"))?;
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
    let index_path = cache_dir.join("catalogue-v1.f32");
    let embeddings = match load_cached_embeddings(&index_path, metadata.len()) {
        Some(embeddings) => embeddings,
        None => {
            let mut embeddings = Vec::with_capacity(metadata.len());
            for batch in metadata.chunks(CATALOGUE_EMBEDDING_BATCH_SIZE) {
                let documents = batch
                    .iter()
                    .map(|(category, family, entry)| semantic_document(category, *family, *entry))
                    .collect::<Vec<_>>();
                let batch_embeddings = model
                    .embed(documents, Some(CATALOGUE_EMBEDDING_BATCH_SIZE))
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
            persist_embeddings(&index_path, &embeddings)?;
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

/// Loads the durable vector matrix only when it is complete, has the current
/// format marker, and matches this exact generated catalogue cardinality.
/// Any mismatch simply rebuilds safely from the source catalogue.
fn load_cached_embeddings(path: &std::path::Path, expected_count: usize) -> Option<Vec<Vec<f32>>> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut magic = [0_u8; INDEX_MAGIC.len()];
    file.read_exact(&mut magic).ok()?;
    if magic != INDEX_MAGIC {
        return None;
    }
    let count = read_u32(&mut file)? as usize;
    let dimensions = read_u32(&mut file)? as usize;
    if count != expected_count || dimensions == 0 || dimensions > 4096 {
        return None;
    }
    let mut embeddings = Vec::with_capacity(count);
    for _ in 0..count {
        let mut embedding = Vec::with_capacity(dimensions);
        for _ in 0..dimensions {
            let mut bytes = [0_u8; 4];
            file.read_exact(&mut bytes).ok()?;
            embedding.push(f32::from_le_bytes(bytes));
        }
        embeddings.push(embedding);
    }
    Some(embeddings)
}

fn persist_embeddings(path: &std::path::Path, embeddings: &[Vec<f32>]) -> Result<(), String> {
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
    let mut file = std::fs::File::create(&temporary_path)
        .map_err(|error| format!("could not create icon vector cache: {error}"))?;
    file.write_all(&INDEX_MAGIC)
        .and_then(|_| file.write_all(&(embeddings.len() as u32).to_le_bytes()))
        .and_then(|_| file.write_all(&(dimensions as u32).to_le_bytes()))
        .map_err(|error| format!("could not write icon vector cache header: {error}"))?;
    for embedding in embeddings {
        for value in embedding {
            file.write_all(&value.to_le_bytes())
                .map_err(|error| format!("could not write icon vector cache: {error}"))?;
        }
    }
    file.sync_all()
        .map_err(|error| format!("could not finalize icon vector cache: {error}"))?;
    std::fs::rename(&temporary_path, path)
        .map_err(|error| format!("could not publish icon vector cache: {error}"))
}

fn read_u32(file: &mut std::fs::File) -> Option<u32> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes).ok()?;
    Some(u32::from_le_bytes(bytes))
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

        persist_embeddings(&cache_path, &embeddings).expect("persist vector matrix");

        assert_eq!(
            load_cached_embeddings(&cache_path, embeddings.len()),
            Some(embeddings)
        );
        assert!(load_cached_embeddings(&cache_path, 3).is_none());
    }
}
