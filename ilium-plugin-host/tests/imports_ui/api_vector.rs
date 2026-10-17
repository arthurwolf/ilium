//! `ilium:host/vector` (api-vector.md): embeddings and nearest-neighbour indexes.

use ilium_plugin_api::generated::host::vector::HostVector;
use ilium_plugin_api::generated::types::ids::{IndexId, RequestId};
use ilium_plugin_api::ErrorCode;

use crate::common::*;

const GRANTS: &[&str] = &["inference.embed"];

fn seeded() -> Rig {
    let rig = rig(GRANTS);
    rig.kernel.set_embedder(Some(3));
    rig
}

fn texts(count: usize) -> Vec<String> {
    (0..count).map(|n| format!("t{n}")).collect()
}

/// REQ: WIT:ilium:host/vector.embed
#[test]
fn wit_vector_embed_returns_a_request_id_that_completes_with_vectors() {
    let mut rig = seeded();
    let id = rig
        .host
        .vector()
        .embed(texts(2), "search".to_owned())
        .expect("accepted");
    rig.host.pump(8);
    let vectors = rig.host.vector().take_vectors(id).expect("vectors");
    assert_eq!(vectors.len(), 2);
    assert!(vectors.iter().all(|v| v.len() == 3));
}

/// REQ: WIT:ilium:host/vector.embed
#[test]
fn wit_vector_embed_text_count_boundaries_invalid_argument() {
    let mut rig = seeded();
    assert_eq!(
        code_of(rig.host.vector().embed(Vec::new(), "p".to_owned())),
        ErrorCode::InvalidArgument
    );
    assert!(rig.host.vector().embed(texts(256), "p".to_owned()).is_ok());
    assert_eq!(
        code_of(rig.host.vector().embed(texts(257), "p".to_owned())),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/vector.embed
#[test]
fn wit_vector_embed_text_size_boundaries_invalid_argument() {
    let mut rig = seeded();
    assert!(rig
        .host
        .vector()
        .embed(vec![text_of(8 * KIB - 1)], "p".to_owned())
        .is_ok());
    assert!(rig
        .host
        .vector()
        .embed(vec![text_of(8 * KIB)], "p".to_owned())
        .is_ok());
    assert_eq!(
        code_of(
            rig.host
                .vector()
                .embed(vec![text_of(8 * KIB + 1)], "p".to_owned())
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/vector.embed
#[test]
fn wit_vector_embed_without_a_provider_unsupported() {
    let mut rig = rig(GRANTS);
    rig.kernel.set_embedder(None);
    assert_eq!(
        code_of(rig.host.vector().embed(texts(1), "p".to_owned())),
        ErrorCode::Unsupported
    );
}

/// REQ: WIT:ilium:host/vector.embed
#[test]
fn wit_vector_embed_declined_grant_not_permitted() {
    let mut rig = declined_rig(&["inference.embed"]);
    assert_eq!(
        code_of(rig.host.vector().embed(texts(1), "p".to_owned())),
        ErrorCode::NotPermitted
    );
}

/// REQ: WIT:ilium:host/vector.take-vectors
#[test]
fn wit_vector_take_vectors_returns_once() {
    let mut rig = seeded();
    let id = rig
        .host
        .vector()
        .embed(texts(1), "p".to_owned())
        .expect("accepted");
    rig.host.pump(8);
    assert!(rig.host.vector().take_vectors(id).is_some());
    assert!(rig.host.vector().take_vectors(id).is_none());
}

/// REQ: WIT:ilium:host/vector.take-vectors
#[test]
fn wit_vector_take_vectors_unknown_request_is_none() {
    let mut rig = seeded();
    assert!(rig
        .host
        .vector()
        .take_vectors(RequestId(4_040_404))
        .is_none());
}

/// REQ: WIT:ilium:host/vector.create-index
#[test]
fn wit_vector_create_index_returns_distinct_ids() {
    let mut rig = seeded();
    let first = rig
        .host
        .vector()
        .create_index("a".to_owned(), 3)
        .expect("a");
    let second = rig
        .host
        .vector()
        .create_index("b".to_owned(), 3)
        .expect("b");
    assert_ne!(first, second);
}

/// REQ: WIT:ilium:host/vector.create-index
#[test]
fn wit_vector_create_index_dimension_boundaries_invalid_argument() {
    let mut rig = seeded();
    assert_eq!(
        code_of(rig.host.vector().create_index("z".to_owned(), 0)),
        ErrorCode::InvalidArgument
    );
    let one = rig
        .host
        .vector()
        .create_index("one".to_owned(), 1)
        .expect("1");
    rig.host.vector().drop_index(one);
    let max = rig
        .host
        .vector()
        .create_index("max".to_owned(), 4096)
        .expect("4096");
    rig.host.vector().drop_index(max);
    assert_eq!(
        code_of(rig.host.vector().create_index("big".to_owned(), 4097)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/vector.create-index
#[test]
fn wit_vector_create_index_fifth_index_quota_exceeded() {
    let mut rig = seeded();
    for n in 0..4 {
        rig.host
            .vector()
            .create_index(format!("i{n}"), 3)
            .expect("within quota");
    }
    assert_eq!(
        code_of(rig.host.vector().create_index("i4".to_owned(), 3)),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/vector.upsert
#[test]
fn wit_vector_upsert_inserts_then_replaces_by_id() {
    let mut rig = seeded();
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 2)
        .expect("index");
    rig.host
        .vector()
        .upsert(index, vec![("x".to_owned(), vec![1.0, 0.0])])
        .expect("insert");
    rig.host
        .vector()
        .upsert(index, vec![("x".to_owned(), vec![0.0, 1.0])])
        .expect("replace");
    let hits = rig
        .host
        .vector()
        .top_k(index, vec![0.0, 1.0], 5)
        .expect("hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "x");
}

/// REQ: WIT:ilium:host/vector.upsert
#[test]
fn wit_vector_upsert_length_mismatch_invalid_argument() {
    let mut rig = seeded();
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 2)
        .expect("index");
    assert_eq!(
        code_of(
            rig.host
                .vector()
                .upsert(index, vec![("x".to_owned(), vec![1.0, 2.0, 3.0])])
        ),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/vector.upsert
#[test]
fn wit_vector_upsert_unknown_index_not_found() {
    let mut rig = seeded();
    assert_eq!(
        code_of(
            rig.host
                .vector()
                .upsert(IndexId(999), vec![("x".to_owned(), vec![1.0])])
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/vector.upsert
#[test]
fn wit_vector_upsert_entry_quota_boundary_quota_exceeded() {
    let mut rig = seeded();
    rig.kernel.set_vector_entry_limit(2);
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 1)
        .expect("index");
    let entries = |ids: &[&str]| {
        ids.iter()
            .map(|id| ((*id).to_owned(), vec![1.0f32]))
            .collect::<Vec<_>>()
    };
    assert!(rig
        .host
        .vector()
        .upsert(index, entries(&["a", "b"]))
        .is_ok());
    assert_eq!(
        code_of(rig.host.vector().upsert(index, entries(&["c"]))),
        ErrorCode::QuotaExceeded
    );
}

/// REQ: WIT:ilium:host/vector.remove-entries
#[test]
fn wit_vector_remove_entries_drops_the_named_entries() {
    let mut rig = seeded();
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 1)
        .expect("index");
    rig.host
        .vector()
        .upsert(
            index,
            vec![("x".to_owned(), vec![1.0]), ("y".to_owned(), vec![0.5])],
        )
        .expect("insert");
    rig.host
        .vector()
        .remove_entries(index, vec!["x".to_owned()])
        .expect("removed");
    let hits = rig.host.vector().top_k(index, vec![1.0], 5).expect("hits");
    assert_eq!(
        hits.iter().map(|h| h.0.as_str()).collect::<Vec<_>>(),
        vec!["y"]
    );
}

/// REQ: WIT:ilium:host/vector.remove-entries
#[test]
fn wit_vector_remove_entries_unknown_ids_are_ignored() {
    let mut rig = seeded();
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 1)
        .expect("index");
    assert!(rig
        .host
        .vector()
        .remove_entries(index, vec!["ghost".to_owned()])
        .is_ok());
}

/// REQ: WIT:ilium:host/vector.remove-entries
#[test]
fn wit_vector_remove_entries_unknown_index_not_found() {
    let mut rig = seeded();
    assert_eq!(
        code_of(
            rig.host
                .vector()
                .remove_entries(IndexId(999), vec!["x".to_owned()])
        ),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/vector.top-k
#[test]
fn wit_vector_top_k_orders_by_similarity_and_limits_to_k() {
    let mut rig = seeded();
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 2)
        .expect("index");
    rig.host
        .vector()
        .upsert(
            index,
            vec![
                ("near".to_owned(), vec![1.0, 0.1]),
                ("far".to_owned(), vec![0.0, 1.0]),
                ("mid".to_owned(), vec![0.7, 0.7]),
            ],
        )
        .expect("insert");
    let hits = rig
        .host
        .vector()
        .top_k(index, vec![1.0, 0.0], 2)
        .expect("hits");
    assert_eq!(
        hits.iter().map(|h| h.0.as_str()).collect::<Vec<_>>(),
        vec!["near", "mid"]
    );
    assert!(hits[0].1 >= hits[1].1);
}

/// REQ: WIT:ilium:host/vector.top-k
#[test]
fn wit_vector_top_k_k_boundaries_invalid_argument() {
    let mut rig = seeded();
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 1)
        .expect("index");
    assert_eq!(
        code_of(rig.host.vector().top_k(index, vec![1.0], 0)),
        ErrorCode::InvalidArgument
    );
    assert!(rig.host.vector().top_k(index, vec![1.0], 1).is_ok());
    assert!(rig.host.vector().top_k(index, vec![1.0], 100).is_ok());
    assert_eq!(
        code_of(rig.host.vector().top_k(index, vec![1.0], 101)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/vector.top-k
#[test]
fn wit_vector_top_k_query_length_mismatch_invalid_argument() {
    let mut rig = seeded();
    let index = rig
        .host
        .vector()
        .create_index("a".to_owned(), 2)
        .expect("index");
    assert_eq!(
        code_of(rig.host.vector().top_k(index, vec![1.0], 3)),
        ErrorCode::InvalidArgument
    );
}

/// REQ: WIT:ilium:host/vector.drop-index
#[test]
fn wit_vector_drop_index_frees_the_quota_slot_and_invalidates_the_id() {
    let mut rig = seeded();
    let ids: Vec<_> = (0..4)
        .map(|n| {
            rig.host
                .vector()
                .create_index(format!("i{n}"), 1)
                .expect("index")
        })
        .collect();
    rig.host.vector().drop_index(ids[0]);
    assert!(rig
        .host
        .vector()
        .create_index("again".to_owned(), 1)
        .is_ok());
    assert_eq!(
        code_of(rig.host.vector().top_k(ids[0], vec![1.0], 1)),
        ErrorCode::NotFound
    );
}

/// REQ: WIT:ilium:host/vector.drop-index
#[test]
fn wit_vector_drop_index_unknown_id_is_a_no_op() {
    let mut rig = seeded();
    rig.host.vector().drop_index(IndexId(31_337));
    assert!(rig.host.failures().is_empty());
}
