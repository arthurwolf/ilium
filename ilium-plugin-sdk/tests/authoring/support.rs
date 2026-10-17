//! Shared helpers of the authoring lane: manual page access and small native test plugins.
//!
//! The plugins implement `NativeEntry` by hand (the `ilium_plugin` macro is part of the code under
//! test elsewhere) and use only the returned submissions, so the lane does not depend on host
//! import functions that the skeleton does not expose.

use std::path::PathBuf;

use ilium_plugin_sdk::dedup;
use ilium_plugin_sdk::testing::manual::pages_dir;
use ilium_plugin_sdk::types::{
    Activation, DeactivationReason, Event, Intent, Outcome, PluginError, RequestRef, RequestResult,
    Submission,
};
use ilium_plugin_sdk::NativeEntry;

/// Text of one manual page (`name` without `.md`).
pub fn page(name: &str) -> String {
    let path: PathBuf = pages_dir().join(format!("{name}.md"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("manual page {}: {error}", path.display()))
}

/// Every page of the manual with its name.
pub fn all_pages() -> Vec<(String, String)> {
    let mut pages = Vec::new();
    let entries = std::fs::read_dir(pages_dir()).expect("manual pages folder");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        if let Ok(text) = std::fs::read_to_string(&path) {
            pages.push((name, text));
        }
    }
    pages.sort();
    pages
}

/// The row of the requirement table for `id`.
pub fn table_row(id: &str) -> String {
    let table = page("reference-requirements-author-guides");
    let prefix = format!("| {id} |");
    table
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("requirement row {id} missing"))
        .to_string()
}

/// Lines of the guide pages (not the requirement tables) that carry the marker `[id]`.
pub fn rule_lines(id: &str) -> Vec<String> {
    let marker = format!("[{id}]");
    let mut found = Vec::new();
    for (name, text) in all_pages() {
        if name.starts_with("reference-requirements-") {
            continue;
        }
        found.extend(
            text.lines()
                .filter(|line| line.contains(&marker))
                .map(str::to_string),
        );
    }
    found
}

/// Asserts that the table row and one page line of `id` contain every needle.
pub fn assert_rule(id: &str, keyword: &str, needles: &[&str]) {
    let row = table_row(id);
    assert!(
        row.contains(keyword),
        "{id}: row lacks keyword {keyword}: {row}"
    );
    let lines = rule_lines(id);
    assert!(!lines.is_empty(), "{id}: no page line carries the marker");
    for needle in needles {
        assert!(row.contains(needle), "{id}: table row lacks {needle:?}");
        assert!(
            lines.iter().any(|line| line.contains(needle)),
            "{id}: page line lacks {needle:?}"
        );
    }
}

/// `host.deliver` batch used by the plugins below: a unit event that carries no payload.
pub fn tick() -> Event {
    Event::ThemeChanged
}

/// Sound id of the manual's catalogue example (api-intents: `ding`).
pub const SOUND: &str = "ding";

/// Submits the same dedup key with different content on every event (AUT-016).
#[derive(Default)]
pub struct SameKeyEmitter {
    sent: u32,
}

impl NativeEntry for SameKeyEmitter {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for _ in batch {
            let intent = if self.sent == 0 {
                Intent::PlaySound(SOUND.to_string())
            } else {
                Intent::DismissToast(format!("toast-{}", self.sent))
            };
            out.push(Submission::keyed(intent, "finished:1:1"));
            self.sent += 1;
        }
        out
    }
}

/// Builds the key from an id and a revision and bumps the revision per event (AUT-015).
#[derive(Default)]
pub struct RevisionKeyEmitter {
    revision: u32,
}

impl NativeEntry for RevisionKeyEmitter {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for _ in batch {
            self.revision += 1;
            let key = dedup::key_for(&["finished", "1", &self.revision.to_string()]);
            out.push(Submission::keyed(Intent::PlaySound(SOUND.to_string()), key));
        }
        out
    }
}

/// Restore bytes with a leading format byte; format 2 is current, format 1 the previous release
/// (AUT-017, AUT-206).
#[derive(Default)]
pub struct Versioned {
    pub value: u32,
}

impl Versioned {
    pub const FORMAT: u8 = 2;
    pub const PREVIOUS_FORMAT: u8 = 1;

    pub fn load(bytes: &[u8]) -> Versioned {
        match bytes {
            [2, a, b, c, d] => Versioned {
                value: u32::from_le_bytes([*a, *b, *c, *d]),
            },
            [1, a] => Versioned {
                value: u32::from(*a),
            },
            _ => Versioned::default(),
        }
    }

    pub fn save(&self) -> Vec<u8> {
        let mut out = vec![Self::FORMAT];
        out.extend_from_slice(&self.value.to_le_bytes());
        out
    }
}

impl NativeEntry for Versioned {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle"];
    fn native_activate(&mut self, context: Activation) -> Result<(), PluginError> {
        self.value = context
            .restored
            .as_deref()
            .map(|bytes| Versioned::load(bytes).value)
            .unwrap_or(0);
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        Some(self.save())
    }
    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// Submits one `play-sound` per event, named by the position in the batch (AUT-020).
#[derive(Default)]
pub struct InOrder;

impl NativeEntry for InOrder {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        batch
            .iter()
            .enumerate()
            .map(|(index, _)| Submission::new(Intent::DismissToast(format!("event-{index}"))))
            .collect()
    }
}

/// Rebuilds its picture after `events-dropped` and ignores unit events otherwise (AUT-021).
#[derive(Default)]
pub struct Resync;

impl NativeEntry for Resync {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            if let Event::EventsDropped(_) = event {
                out.push(Submission::new(Intent::DismissToast("resync".to_string())));
            }
        }
        out
    }
}

/// Generation guard of recipe 8: a start on every `theme-changed`, an apply only for the newest
/// completion (AUT-019).
#[derive(Default)]
pub struct Latest {
    generation: u64,
    by_request: std::collections::HashMap<u64, u64>,
}

impl Latest {
    pub fn start(&mut self, request: u64) {
        self.generation += 1;
        self.by_request.insert(request, self.generation);
    }
    pub fn accept(&mut self, request: u64) -> bool {
        self.by_request.remove(&request) == Some(self.generation)
    }
}

/// The start submission carries a sound intent; the apply submission a toast dismissal named
/// `apply`.
#[derive(Default)]
pub struct StaleGuard {
    latest: Latest,
}

impl NativeEntry for StaleGuard {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            match event {
                Event::ThemeChanged => {
                    let submission = Submission::new(Intent::PlaySound(SOUND.to_string()));
                    self.latest.start(submission.correlation);
                    out.push(submission);
                }
                Event::RequestCompleted(RequestResult {
                    request: RequestRef::Submission(correlation),
                    ..
                }) => {
                    if self.latest.accept(correlation) {
                        out.push(Submission::new(Intent::DismissToast("apply".to_string())));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// Plays a sound; when the host refuses with `not-permitted` it falls back to a toast dismissal
/// and keeps working (AUT-205, AUT-405).
#[derive(Default)]
pub struct OptionalGrant {
    pending: Option<u64>,
}

impl NativeEntry for OptionalGrant {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle", "handler"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        None
    }
    fn native_on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut out = Vec::new();
        for event in batch {
            match event {
                Event::ThemeChanged => {
                    let submission = Submission::new(Intent::PlaySound(SOUND.to_string()));
                    self.pending = Some(submission.correlation);
                    out.push(submission);
                }
                Event::RequestCompleted(RequestResult {
                    request: RequestRef::Submission(correlation),
                    outcome,
                }) => {
                    if self.pending == Some(correlation) {
                        self.pending = None;
                        if let Outcome::Refused(error) = outcome {
                            if error.code == ilium_plugin_sdk::types::ErrorCode::NotPermitted {
                                out.push(Submission::new(Intent::DismissToast(
                                    "fallback".to_string(),
                                )));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// Returns `N` restore bytes from `deactivate` (performance.md budget "Restore bytes": 1 MiB).
#[derive(Default)]
pub struct SizedRestore<const N: usize>;

impl<const N: usize> NativeEntry for SizedRestore<N> {
    const SIDE: &'static str = "server";
    const EXPORTS: &'static [&'static str] = &["lifecycle"];
    fn native_activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
    fn native_deactivate(&mut self, _reason: DeactivationReason) -> Option<Vec<u8>> {
        Some(vec![7u8; N])
    }
    fn native_on_events(&mut self, _batch: Vec<Event>) -> Vec<Submission> {
        Vec::new()
    }
}

/// One MiB in bytes.
pub const MIB: usize = 1024 * 1024;
