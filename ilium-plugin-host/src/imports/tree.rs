//! Host implementation of `ilium:host/tree`.
//!
//! Hand-owned. This import has no attached session tree yet (HST-020), so every node lookup
//! finds nothing and metadata writes are refused. The session-backed tree replaces the empty
//! view when a session is attached.

use ilium_plugin_api::generated::host::tree::HostTree;
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::NodeId;
use ilium_plugin_api::generated::types::tree::{NodeQuery, NodeView};

/// Host side of the `tree` interface.
#[derive(Debug)]
pub struct TreeImport;

impl HostTree for TreeImport {
    fn get(&mut self, _node: NodeId) -> Option<NodeView> {
        None
    }

    fn children(&mut self, _node: NodeId) -> Vec<NodeId> {
        Vec::new()
    }

    fn roots(&mut self) -> Vec<NodeId> {
        Vec::new()
    }

    fn find(&mut self, _query: NodeQuery) -> Vec<NodeId> {
        Vec::new()
    }

    fn metadata(&mut self, _node: NodeId) -> Option<Vec<u8>> {
        None
    }

    fn set_metadata(&mut self, node: NodeId, _bytes: Vec<u8>) -> Result<(), PluginError> {
        // Without an attached session there is no node to write to. Node 0 stands for the
        // session-less host (a CLI process) and is refused; any other id is simply absent.
        let (code, message) = if node.0 == 0 {
            (ErrorCode::NotPermitted, "no session is attached")
        } else {
            (ErrorCode::NotFound, "the node does not exist")
        };
        Err(PluginError {
            code,
            message: message.to_string(),
            retry_after_ms: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TreeImport;
    use ilium_plugin_api::generated::host::tree::HostTree;
    use ilium_plugin_api::generated::types::errors::ErrorCode;
    use ilium_plugin_api::generated::types::ids::NodeId;
    use ilium_plugin_api::generated::types::tree::NodeQuery;

    /// A node id that no plugin session can hold in practice; used as the "unknown node" case.
    const UNKNOWN_NODE_ID: NodeId = NodeId(9_999_999);

    /// The largest node id the wire format can carry; a boundary value for every id argument.
    const MAXIMUM_NODE_ID: NodeId = NodeId(u64::MAX);

    /// Error codes `set-metadata` may return: the documented `not-found`, `quota-exceeded`,
    /// `not-permitted` and `revoked` cases, plus `internal` for host faults (API-004).
    const SET_METADATA_ERROR_CODES: &[ErrorCode] = &[
        ErrorCode::NotPermitted,
        ErrorCode::NotFound,
        ErrorCode::QuotaExceeded,
        ErrorCode::Revoked,
        ErrorCode::Internal,
    ];

    /// Largest error message the plugin error record may carry, in characters.
    const PLUGIN_ERROR_MESSAGE_LIMIT_CHARS: usize = 256;

    /// REQ: ABI-043
    #[test]
    fn get_returns_none_for_unknown_node() {
        let mut import = TreeImport;
        assert!(import.get(UNKNOWN_NODE_ID).is_none());
    }

    /// REQ: ABI-043
    #[test]
    fn get_returns_none_for_maximum_node_id() {
        let mut import = TreeImport;
        assert!(import.get(MAXIMUM_NODE_ID).is_none());
    }

    /// REQ: HST-020
    #[test]
    fn get_returns_none_when_no_session_is_attached() {
        let mut import = TreeImport;
        assert!(import.get(NodeId(0)).is_none());
    }

    /// REQ: ABI-043
    #[test]
    fn children_returns_empty_list_for_unknown_node() {
        let mut import = TreeImport;
        assert!(import.children(UNKNOWN_NODE_ID).is_empty());
    }

    /// REQ: ABI-043
    #[test]
    fn children_returns_empty_list_for_maximum_node_id() {
        let mut import = TreeImport;
        assert!(import.children(MAXIMUM_NODE_ID).is_empty());
    }

    /// REQ: ABI-043
    #[test]
    fn roots_returns_empty_list_when_tree_has_no_nodes() {
        let mut import = TreeImport;
        assert!(import.roots().is_empty());
    }

    /// REQ: ABI-043
    #[test]
    fn find_with_empty_query_returns_empty_list_when_tree_has_no_nodes() {
        let mut import = TreeImport;
        assert!(import.find(NodeQuery::default()).is_empty());
    }

    /// REQ: ABI-043
    #[test]
    fn find_with_unknown_under_node_returns_empty_list() {
        let mut import = TreeImport;
        let query = NodeQuery {
            under: Some(UNKNOWN_NODE_ID),
            ..NodeQuery::default()
        };
        assert!(import.find(query).is_empty());
    }

    /// REQ: ABI-043
    #[test]
    fn find_with_empty_content_kind_returns_empty_list() {
        let mut import = TreeImport;
        let query = NodeQuery {
            content_kind: Some(String::new()),
            ..NodeQuery::default()
        };
        assert!(import.find(query).is_empty());
    }

    /// REQ: ABI-043
    #[test]
    fn find_with_every_field_set_returns_empty_list_for_absent_tree() {
        let mut import = TreeImport;
        let query = NodeQuery {
            content_kind: Some("ilium.terminal".to_string()),
            signature: Some("claude".to_string()),
            under: Some(MAXIMUM_NODE_ID),
        };
        assert!(import.find(query).is_empty());
    }

    /// REQ: ABI-043
    #[test]
    fn metadata_returns_none_for_unknown_node() {
        let mut import = TreeImport;
        assert!(import.metadata(UNKNOWN_NODE_ID).is_none());
    }

    /// REQ: ABI-043
    #[test]
    fn metadata_returns_none_for_maximum_node_id() {
        let mut import = TreeImport;
        assert!(import.metadata(MAXIMUM_NODE_ID).is_none());
    }

    /// REQ: API-002
    #[test]
    fn set_metadata_on_unknown_node_returns_not_found() {
        let mut import = TreeImport;
        let error = import
            .set_metadata(UNKNOWN_NODE_ID, vec![1])
            .expect_err("writing metadata to an absent node must fail");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: API-002
    #[test]
    fn set_metadata_with_empty_bytes_on_unknown_node_returns_not_found() {
        let mut import = TreeImport;
        let error = import
            .set_metadata(UNKNOWN_NODE_ID, Vec::new())
            .expect_err("an empty write to an absent node must still fail");
        assert_eq!(error.code, ErrorCode::NotFound);
    }

    /// REQ: HST-020
    #[test]
    fn set_metadata_without_session_returns_not_permitted() {
        let mut import = TreeImport;
        let error = import
            .set_metadata(NodeId(0), vec![1])
            .expect_err("a CLI-process host has no session to write metadata into");
        assert_eq!(error.code, ErrorCode::NotPermitted);
    }

    /// REQ: API-004
    #[test]
    fn set_metadata_error_code_is_one_documented_for_the_function() {
        let mut import = TreeImport;
        let error = import
            .set_metadata(UNKNOWN_NODE_ID, vec![1])
            .expect_err("writing metadata to an absent node must fail");
        assert!(SET_METADATA_ERROR_CODES.contains(&error.code));
    }

    /// REQ: API-003
    #[test]
    fn set_metadata_error_carries_no_retry_delay() {
        let mut import = TreeImport;
        let error = import
            .set_metadata(UNKNOWN_NODE_ID, vec![1])
            .expect_err("writing metadata to an absent node must fail");
        assert_eq!(error.retry_after_ms, None);
    }

    /// REQ: API-002
    #[test]
    fn set_metadata_error_message_stays_within_record_limit() {
        let mut import = TreeImport;
        let error = import
            .set_metadata(UNKNOWN_NODE_ID, vec![1])
            .expect_err("writing metadata to an absent node must fail");
        assert!(error.message.chars().count() <= PLUGIN_ERROR_MESSAGE_LIMIT_CHARS);
    }
}
