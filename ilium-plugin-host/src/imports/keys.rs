//! Host implementation of `ilium:host/keys`, and the chord grammar every key contribution uses.
//!
//! Grabs live in one [`KeyGrabs`] table per runtime (a session), capped at 32 grabs across all of
//! its plugins. Every grab remembers its owner, so the runtime frees a plugin's grabs when the
//! plugin stops. Nothing is process-wide: two runtimes in one process have two tables.
//!
//! The client asks the table which grab, if any, takes a pressed key ([`KeyGrabs::route`]); the
//! matching plugin then hears `key-pressed` and the key does not reach the pane (gap G1).
//!
//! [`parse_chord`] reads the grammar of `api-keys.md` and `reference-manifest.md`
//! ("Keybinding chords"): `*(modifier "+") key` in lower case. A grab and a manifest binding
//! compare chords in their canonical form ([`Chord::canonical`]), so `shift+ctrl+k` and
//! `ctrl+shift+k` are the same chord.

use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
use ilium_plugin_api::generated::types::errors::{ErrorCode, PluginError};
use ilium_plugin_api::generated::types::ids::GrabId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Session-wide cap on key grabs (api-host, API-070).
const SESSION_KEY_GRAB_CAP: usize = 32;

/// Chords the kernel keeps for itself; plugins cannot grab them.
const RESERVED_CHORDS: [&str; 2] = ["ctrl+shift+escape", "ctrl+alt+escape"];

/// Most chords one grab may name (api-keys.md, `GrabSpec.chords`).
const CHORDS_PER_GRAB: usize = 32;

/// The key of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChordKey {
    /// One printable character, lower case; `+` is written `plus`.
    Character(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    /// `f1` to `f24`.
    Function(u8),
}

/// The named keys and their chord names.
const NAMED_KEYS: [(&str, ChordKey); 14] = [
    ("enter", ChordKey::Enter),
    ("escape", ChordKey::Escape),
    ("tab", ChordKey::Tab),
    ("backspace", ChordKey::Backspace),
    ("delete", ChordKey::Delete),
    ("insert", ChordKey::Insert),
    ("home", ChordKey::Home),
    ("end", ChordKey::End),
    ("page-up", ChordKey::PageUp),
    ("page-down", ChordKey::PageDown),
    ("up", ChordKey::Up),
    ("down", ChordKey::Down),
    ("left", ChordKey::Left),
    ("right", ChordKey::Right),
];

/// One chord: modifiers and a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: ChordKey,
}

impl Chord {
    /// A chord of a bare key.
    pub fn of_key(key: ChordKey) -> Self {
        Self {
            ctrl: false,
            alt: false,
            shift: false,
            super_key: false,
            key,
        }
    }

    /// Whether the chord has any modifier.
    pub fn has_modifiers(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.super_key
    }

    /// The canonical text: modifiers in the order `ctrl`, `alt`, `shift`, `super`, then the key.
    pub fn canonical(&self) -> String {
        let mut text = String::new();
        for (held, name) in [
            (self.ctrl, "ctrl+"),
            (self.alt, "alt+"),
            (self.shift, "shift+"),
            (self.super_key, "super+"),
        ] {
            if held {
                text.push_str(name);
            }
        }
        match self.key {
            ChordKey::Character('+') => text.push_str("plus"),
            ChordKey::Character(character) => text.push(character),
            ChordKey::Function(number) => text.push_str(&format!("f{number}")),
            named => {
                if let Some((name, _)) = NAMED_KEYS.iter().find(|(_, key)| *key == named) {
                    text.push_str(name);
                }
            }
        }
        text
    }
}

/// The key of one chord's last segment.
fn parse_key(text: &str) -> Option<ChordKey> {
    if text == "plus" {
        return Some(ChordKey::Character('+'));
    }
    if let Some((_, key)) = NAMED_KEYS.iter().find(|(name, _)| *name == text) {
        return Some(*key);
    }
    if let Some(number) = text
        .strip_prefix('f')
        .and_then(|digits| digits.parse::<u8>().ok())
    {
        if (1..=24).contains(&number) && text.len() > 1 {
            return Some(ChordKey::Function(number));
        }
    }
    let mut characters = text.chars();
    let character = characters.next()?;
    let is_printable = !character.is_control() && !character.is_whitespace();
    (characters.next().is_none() && is_printable && !character.is_uppercase() && character != '+')
        .then_some(ChordKey::Character(character))
}

/// Reads one chord of the grammar `*(modifier "+") key`. `None` when it is not one: an upper
/// case letter, an unknown key or modifier, a repeated modifier or an empty segment.
pub fn parse_chord(text: &str) -> Option<Chord> {
    let mut segments: Vec<&str> = text.split('+').collect();
    let key = parse_key(segments.pop()?)?;
    let mut chord = Chord::of_key(key);
    for modifier in segments {
        let slot = match modifier {
            "ctrl" => &mut chord.ctrl,
            "alt" => &mut chord.alt,
            "shift" => &mut chord.shift,
            "super" => &mut chord.super_key,
            _ => return None,
        };
        if *slot {
            return None;
        }
        *slot = true;
    }
    Some(chord)
}

/// One grab: its owner, its chords (as written and canonical), its pane and its end rule.
struct GrabRecord {
    owner: String,
    chords: Vec<(String, String)>,
    pane: Option<u64>,
    until_release: bool,
}

/// A grab that takes a pressed key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrabMatch {
    pub plugin: String,
    pub grab: u64,
    /// The chord as the grab wrote it.
    pub chord: String,
    pub pane: Option<u64>,
}

#[derive(Default)]
struct GrabTable {
    next_id: u64,
    grabs: HashMap<u64, GrabRecord>,
}

/// The key grabs of one runtime, shared by the imports of its plugins. Cloning shares the table.
#[derive(Clone, Default)]
pub struct KeyGrabs {
    table: Arc<Mutex<GrabTable>>,
}

impl std::fmt::Debug for KeyGrabs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyGrabs")
            .field("held", &self.held())
            .finish()
    }
}

impl KeyGrabs {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, GrabTable> {
        // Every writer does one insert or remove, so the table is consistent after a panic.
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many grabs are held in all.
    pub fn held(&self) -> usize {
        self.lock().grabs.len()
    }

    /// Frees every grab of a plugin that stopped.
    pub fn forget_plugin(&self, plugin: &str) {
        self.lock().grabs.retain(|_, grab| grab.owner != plugin);
    }

    /// Ends the grabs of panes that closed: `is_live` says which pane ids still exist.
    pub fn forget_closed_panes(&self, is_live: impl Fn(u64) -> bool) {
        self.lock()
            .grabs
            .retain(|_, grab| grab.pane.is_none_or(&is_live));
    }

    /// The grab that takes a pressed key while `focused` has focus, the oldest first. A grab
    /// naming a pane is active only while that pane has focus. Without release reports, an
    /// active `until-release` grab ends at the first key it does not take.
    pub fn route(&self, chord: &Chord, focused: Option<u64>) -> Option<GrabMatch> {
        let canonical = chord.canonical();
        let mut table = self.lock();
        let mut ids: Vec<u64> = table.grabs.keys().copied().collect();
        ids.sort_unstable();
        let mut found = None;
        let mut ended = Vec::new();
        for id in ids {
            let Some(grab) = table.grabs.get(&id) else {
                continue;
            };
            if grab.pane.is_some_and(|pane| Some(pane) != focused) {
                continue;
            }
            let written = grab
                .chords
                .iter()
                .find(|(_, held)| *held == canonical)
                .map(|(written, _)| written.clone());
            match written {
                Some(chord) if found.is_none() => {
                    found = Some(GrabMatch {
                        plugin: grab.owner.clone(),
                        grab: id,
                        chord,
                        pane: grab.pane,
                    });
                }
                Some(_) => {}
                None if grab.until_release => ended.push(id),
                None => {}
            }
        }
        for id in ended {
            table.grabs.remove(&id);
        }
        found
    }
}

fn refuse(code: ErrorCode, message: &str) -> PluginError {
    PluginError {
        code,
        message: message.to_string(),
        retry_after_ms: None,
    }
}

/// Host side of the `keys` interface of one plugin. Cloning shares the table and the owner.
#[derive(Debug, Clone)]
pub struct KeysImport {
    grabs: KeyGrabs,
    plugin: String,
}

impl Default for KeysImport {
    fn default() -> Self {
        Self::new(KeyGrabs::new(), "ilium.host.standalone")
    }
}

impl KeysImport {
    /// The import of `plugin` over the runtime's table.
    pub fn new(grabs: KeyGrabs, plugin: &str) -> Self {
        Self {
            grabs,
            plugin: plugin.to_owned(),
        }
    }
}

impl HostKeys for KeysImport {
    fn grab(&mut self, spec: GrabSpec) -> Result<GrabId, PluginError> {
        if spec.chords.is_empty() || spec.chords.iter().any(|chord| chord.is_empty()) {
            return Err(refuse(ErrorCode::InvalidArgument, "grab names no chord"));
        }
        if spec.chords.len() > CHORDS_PER_GRAB {
            return Err(refuse(
                ErrorCode::InvalidArgument,
                "a grab names at most 32 chords",
            ));
        }
        let mut chords = Vec::with_capacity(spec.chords.len());
        for written in spec.chords {
            let Some(chord) = parse_chord(&written) else {
                return Err(refuse(
                    ErrorCode::InvalidArgument,
                    "a chord is not in the chord grammar",
                ));
            };
            chords.push((written, chord.canonical()));
        }
        if chords
            .iter()
            .any(|(_, canonical)| RESERVED_CHORDS.contains(&canonical.as_str()))
        {
            return Err(refuse(
                ErrorCode::Conflict,
                "chord is reserved by the kernel",
            ));
        }
        let mut table = self.grabs.lock();
        if table.grabs.len() >= SESSION_KEY_GRAB_CAP {
            return Err(refuse(ErrorCode::QuotaExceeded, "key grab limit reached"));
        }
        // Two grabs may hold one chord only for two different panes.
        let already_held = table.grabs.values().any(|grab| {
            let overlaps = grab.pane.is_none()
                || spec.pane.is_none()
                || grab.pane == spec.pane.map(|pane| pane.0);
            overlaps
                && grab
                    .chords
                    .iter()
                    .any(|(_, held)| chords.iter().any(|(_, wanted)| wanted == held))
        });
        if already_held {
            return Err(refuse(ErrorCode::Conflict, "chord is already grabbed"));
        }
        table.next_id += 1;
        let id = table.next_id;
        table.grabs.insert(
            id,
            GrabRecord {
                owner: self.plugin.clone(),
                chords,
                pane: spec.pane.map(|pane| pane.0),
                until_release: spec.until_release,
            },
        );
        Ok(GrabId(id))
    }

    fn release(&mut self, grab: GrabId) {
        let mut table = self.grabs.lock();
        // A plugin releases only its own grabs.
        if table
            .grabs
            .get(&grab.0)
            .is_some_and(|grab| grab.owner == self.plugin)
        {
            table.grabs.remove(&grab.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyGrabs, KeysImport};
    use ilium_plugin_api::generated::host::keys::{GrabSpec, HostKeys};
    use ilium_plugin_api::generated::types::errors::ErrorCode;

    /// Session-wide cap on key grabs across all plugins of one session (api-host, API-070).
    const SESSION_KEY_GRAB_CAP: usize = 32;

    fn spec_for_chord(chord: &str) -> GrabSpec {
        GrabSpec {
            chords: vec![chord.to_string()],
            pane: None,
            until_release: false,
        }
    }

    /// REQ: API-070
    #[test]
    fn grab_refuses_with_quota_exceeded_once_the_key_grab_limit_is_reached() {
        let mut import = KeysImport::default();
        let chords: Vec<String> = ('a'..='z')
            .map(|letter| format!("alt+{letter}"))
            .chain(('a'..='z').map(|letter| format!("ctrl+{letter}")))
            .collect();

        let mut granted_count = 0usize;
        let mut refusal_code: Option<ErrorCode> = None;
        for chord in &chords {
            match import.grab(spec_for_chord(chord)) {
                Ok(_) => granted_count += 1,
                Err(error) => {
                    refusal_code = Some(error.code);
                    break;
                }
            }
        }

        assert_eq!(
            refusal_code,
            Some(ErrorCode::QuotaExceeded),
            "the grab past the key grab limit must return quota-exceeded"
        );
        assert!(
            granted_count <= SESSION_KEY_GRAB_CAP,
            "grants before the refusal ({granted_count}) must not exceed the cap of {SESSION_KEY_GRAB_CAP}"
        );
    }

    #[test]
    fn two_runtimes_have_two_grab_tables() {
        let mut first = KeysImport::new(KeyGrabs::new(), "a.one");
        let mut second = KeysImport::new(KeyGrabs::new(), "a.one");
        first.grab(spec_for_chord("alt+q")).expect("first grabs");
        second
            .grab(spec_for_chord("alt+q"))
            .expect("the other table does not know the chord");
    }

    #[test]
    fn plugins_of_one_table_conflict_and_release_only_their_own_grabs() {
        let grabs = KeyGrabs::new();
        let mut owner = KeysImport::new(grabs.clone(), "a.owner");
        let mut other = KeysImport::new(grabs.clone(), "a.other");
        let held = owner.grab(spec_for_chord("alt+q")).expect("grabbed");
        let error = other.grab(spec_for_chord("alt+q")).expect_err("conflict");
        assert_eq!(error.code, ErrorCode::Conflict);
        other.release(held);
        assert_eq!(grabs.held(), 1, "a foreign release changes nothing");
        owner.release(held);
        assert_eq!(grabs.held(), 0);
    }

    #[test]
    fn chords_follow_the_grammar_and_compare_canonically() {
        use super::{parse_chord, ChordKey};
        let chord = parse_chord("shift+ctrl+k").expect("a chord");
        assert_eq!(chord.canonical(), "ctrl+shift+k");
        assert_eq!(
            parse_chord("plus").map(|chord| chord.key),
            Some(ChordKey::Character('+'))
        );
        assert_eq!(
            parse_chord("f12").map(|chord| chord.key),
            Some(ChordKey::Function(12))
        );
        assert_eq!(
            parse_chord("page-up")
                .map(|chord| chord.canonical())
                .as_deref(),
            Some("page-up")
        );
        for invalid in [
            "",
            "ctrl+",
            "ctrl+ctrl+a",
            "hyper+a",
            "A",
            "f25",
            "f0",
            "ab",
            "ctrl++",
        ] {
            assert!(parse_chord(invalid).is_none(), "{invalid} is refused");
        }
    }

    #[test]
    fn a_grab_routes_its_chords_to_its_owner_only_while_its_pane_has_focus() {
        use super::parse_chord;
        use ilium_plugin_api::generated::types::ids::NodeId;
        let grabs = KeyGrabs::new();
        let mut owner = KeysImport::new(grabs.clone(), "a.owner");
        let spec = GrabSpec {
            chords: vec!["up".to_owned(), "shift+ctrl+j".to_owned()],
            pane: Some(NodeId(7)),
            until_release: false,
        };
        let grab = owner.grab(spec).expect("grabbed");
        let up = parse_chord("up").expect("a chord");
        let routed = grabs.route(&up, Some(7)).expect("taken");
        assert_eq!((routed.plugin.as_str(), routed.grab), ("a.owner", grab.0));
        assert!(
            grabs.route(&up, Some(8)).is_none(),
            "another pane keeps the key"
        );
        let written = grabs
            .route(&parse_chord("ctrl+shift+j").expect("a chord"), Some(7))
            .expect("taken");
        assert_eq!(written.chord, "shift+ctrl+j");
        grabs.forget_closed_panes(|pane| pane != 7);
        assert_eq!(grabs.held(), 0);
    }

    #[test]
    fn an_until_release_grab_ends_at_the_first_key_it_does_not_take() {
        use super::parse_chord;
        let grabs = KeyGrabs::new();
        let mut owner = KeysImport::new(grabs.clone(), "a.owner");
        owner
            .grab(GrabSpec {
                chords: vec!["j".to_owned()],
                pane: None,
                until_release: true,
            })
            .expect("grabbed");
        let j = parse_chord("j").expect("a chord");
        assert!(grabs.route(&j, None).is_some());
        assert!(grabs
            .route(&parse_chord("k").expect("a chord"), None)
            .is_none());
        assert!(grabs.route(&j, None).is_none(), "the grab ended");
    }

    #[test]
    fn a_chord_outside_the_grammar_is_refused() {
        let mut import = KeysImport::default();
        let error = import.grab(spec_for_chord("Ctrl+K")).expect_err("refused");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn a_stopped_plugin_gives_its_grabs_back() {
        let grabs = KeyGrabs::new();
        let mut owner = KeysImport::new(grabs.clone(), "a.owner");
        owner.grab(spec_for_chord("alt+q")).expect("grabbed");
        owner.grab(spec_for_chord("alt+w")).expect("grabbed");
        grabs.forget_plugin("a.owner");
        assert_eq!(grabs.held(), 0);
    }
}
