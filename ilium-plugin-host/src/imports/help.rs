//! Host implementation of `ilium:host/help` over a fixed table of topics.
//!
//! The runtime equips `imports::backed::KernelHelp`, which reads the kernel's live help
//! catalogue; this import serves a table handed to it at construction, for hosts that have no
//! kernel. Its table belongs to the import value, never to a thread or the process.

use ilium_plugin_api::generated::host::help::HostHelp;

use std::collections::HashMap;
use std::sync::Arc;

/// True when `id` has the global form `<prefix>/<local>` with both parts non-empty.
fn is_global_topic_id(id: &str) -> bool {
    matches!(id.split_once('/'), Some((prefix, local)) if !prefix.is_empty() && !local.is_empty())
}

/// Host side of the `help` interface: topics keyed by global id `<plugin>/<local-id>` (or
/// `ilium/<id>`) to their markdown.
#[derive(Debug, Clone, Default)]
pub struct HelpImport {
    topics: Arc<HashMap<String, String>>,
}

impl HelpImport {
    /// An import answering from `topics` (global id to markdown).
    pub fn with_topics(topics: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            topics: Arc::new(topics.into_iter().collect()),
        }
    }
}

impl HostHelp for HelpImport {
    /// Unknown, malformed or local ids give `None`, never a failure (case-sensitive match).
    fn topic(&mut self, id: String) -> Option<String> {
        if !is_global_topic_id(&id) {
            return None;
        }
        self.topics.get(&id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::calls::Trap;
    use crate::testing::{FakeKernel, HarnessHandle, HostHarness, NativeGuest};
    use ilium_plugin_api::generated::host::help::HostHelp;
    use ilium_plugin_api::generated::types::events::Event;
    use ilium_plugin_api::generated::types::intents::Submission;
    use ilium_plugin_api::{ProcessKind, Side, Tier};

    /// Global id of the notes plugin's search topic, as the manual writes it.
    const SEARCH_TOPIC_ID: &str = "com.example.notes/notes.search";
    /// File text of the search topic, exactly as the manual's example ships it.
    const SEARCH_TOPIC_TEXT: &str =
        "# Searching notes\n\nType a word in the search box to find it in every note.\n";

    /// A guest with no behaviour; the help import is called directly through the rig.
    struct IdleGuest;

    impl NativeGuest for IdleGuest {
        fn on_events(&mut self, _batch: Vec<Event>) -> Result<Vec<Submission>, Trap> {
            Ok(Vec::new())
        }
    }

    /// A client-side rig whose plugin holds `grants`; the kernel is returned so topics can be set.
    fn help_rig(grants: &[&str]) -> (Arc<FakeKernel>, HarnessHandle) {
        let kernel = Arc::new(FakeKernel::new(ProcessKind::Client));
        let host = HostHarness::new()
            .tier(Tier::T3)
            .side(Side::Client)
            .grants(grants)
            .kernel(kernel.clone())
            .with_native_guest(Box::new(IdleGuest))
            .build()
            .expect("the harness builds a host for a native guest");
        (kernel, host)
    }

    /// A client-side rig where the user declined `requested`; the kernel holds the topics.
    fn declined_help_rig(requested: &[&str]) -> (Arc<FakeKernel>, HarnessHandle) {
        let kernel = Arc::new(FakeKernel::new(ProcessKind::Client));
        let host = HostHarness::new()
            .tier(Tier::T3)
            .side(Side::Client)
            .declined(requested)
            .kernel(kernel.clone())
            .with_native_guest(Box::new(IdleGuest))
            .build()
            .expect("the harness builds a host for a native guest");
        (kernel, host)
    }

    /// Calls `topic(id)` through the rig's `help` accessor.
    fn topic_of(host: &HarnessHandle, id: &str) -> Option<String> {
        let mut help = host.help();
        help.topic(id.to_owned())
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn valid_global_id_returns_the_topic_markdown() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        assert_eq!(
            topic_of(&host, SEARCH_TOPIC_ID),
            Some(SEARCH_TOPIC_TEXT.to_owned())
        );
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn topic_text_is_returned_unchanged_byte_for_byte() {
        let (kernel, host) = help_rig(&["help.read"]);
        let text = "# Title  \n\n  indented line\n\ttabbed\n\n";
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, text)]);
        assert_eq!(topic_of(&host, SEARCH_TOPIC_ID), Some(text.to_owned()));
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn topic_of_another_plugin_is_readable() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[("com.example.other/other.intro", "# Other\n")]);
        assert_eq!(
            topic_of(&host, "com.example.other/other.intro"),
            Some("# Other\n".to_owned())
        );
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn kernel_topic_with_ilium_prefix_is_returned() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[("ilium/keys.reference", "# Keys\n")]);
        assert_eq!(
            topic_of(&host, "ilium/keys.reference"),
            Some("# Keys\n".to_owned())
        );
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn unknown_global_id_is_none_without_a_failure() {
        let (_kernel, host) = help_rig(&["help.read"]);
        assert_eq!(topic_of(&host, "com.example.notes/no-such-topic"), None);
        assert!(host.failures().is_empty());
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn empty_id_is_none() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        assert_eq!(topic_of(&host, ""), None);
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn local_id_without_plugin_prefix_is_none() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        assert_eq!(topic_of(&host, "notes.search"), None);
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn misspelt_plugin_prefix_is_none() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        assert_eq!(topic_of(&host, "com.example.note/notes.search"), None);
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn plugin_prefix_with_empty_local_id_is_none() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        assert_eq!(topic_of(&host, "com.example.notes/"), None);
    }

    /// REQ: WIT:ilium:host/help.topic
    #[test]
    fn id_differing_only_in_case_is_none() {
        let (kernel, host) = help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        assert_eq!(topic_of(&host, "com.example.notes/Notes.Search"), None);
    }

    /// REQ: CAP-013
    #[test]
    fn declined_help_read_grant_returns_none_for_a_real_topic() {
        let (kernel, host) = declined_help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        assert_eq!(topic_of(&host, SEARCH_TOPIC_ID), None);
    }

    /// REQ: CAP-013
    #[test]
    fn declined_help_read_grant_never_fails_the_call() {
        let (kernel, host) = declined_help_rig(&["help.read"]);
        kernel.set_help_topics(&[(SEARCH_TOPIC_ID, SEARCH_TOPIC_TEXT)]);
        let _ = topic_of(&host, SEARCH_TOPIC_ID);
        assert!(host.failures().is_empty());
    }

    /// REQ: CAP-013
    #[test]
    fn declined_grant_with_empty_id_is_none() {
        let (_kernel, host) = declined_help_rig(&["help.read"]);
        assert_eq!(topic_of(&host, ""), None);
    }

    #[test]
    fn a_fixed_table_answers_global_ids_and_is_its_own() {
        let mut help = super::HelpImport::with_topics([(
            SEARCH_TOPIC_ID.to_owned(),
            SEARCH_TOPIC_TEXT.to_owned(),
        )]);
        assert_eq!(
            help.topic(SEARCH_TOPIC_ID.to_owned()),
            Some(SEARCH_TOPIC_TEXT.to_owned())
        );
        assert_eq!(help.topic("notes.search".to_owned()), None);
        let mut empty = super::HelpImport::default();
        assert_eq!(empty.topic(SEARCH_TOPIC_ID.to_owned()), None);
    }
}
