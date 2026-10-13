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
    fn embed(&mut self, texts: Vec<String>, purpose: String) -> Result<RequestId, PluginError> {
        unimplemented!("host::imports::vector::embed")
    }

    fn take_vectors(&mut self, request: RequestId) -> Option<Vec<Vec<f32>>> {
        unimplemented!("host::imports::vector::take_vectors")
    }

    fn create_index(&mut self, name: String, dimensions: u16) -> Result<IndexId, PluginError> {
        unimplemented!("host::imports::vector::create_index")
    }

    fn upsert(
        &mut self,
        index: IndexId,
        entries: Vec<(String, Vec<f32>)>,
    ) -> Result<(), PluginError> {
        unimplemented!("host::imports::vector::upsert")
    }

    fn remove_entries(&mut self, index: IndexId, ids: Vec<String>) -> Result<(), PluginError> {
        unimplemented!("host::imports::vector::remove_entries")
    }

    fn top_k(
        &mut self,
        index: IndexId,
        query: Vec<f32>,
        k: u16,
    ) -> Result<Vec<(String, f32)>, PluginError> {
        unimplemented!("host::imports::vector::top_k")
    }

    fn drop_index(&mut self, index: IndexId) {
        unimplemented!("host::imports::vector::drop_index")
    }
}
