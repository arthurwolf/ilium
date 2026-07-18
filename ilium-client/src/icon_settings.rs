//! Global icon vocabulary shared by the tree renderer and the Settings UI.
//!
//! This module deliberately stores glyphs as UTF-8 strings rather than a
//! closed enum: the curated quick choices stay useful, while the full picker
//! can legitimately select any catalogued symbol without requiring a client
//! release for each new choice.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Every independently configurable visual role currently shown in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconTarget {
    Group,
    SplitVertical,
    SplitHorizontal,
    Folder,
    Terminal,
    Editor,
    Board,
    Claude,
    Codex,
    Antigravity,
    OtherAgent,
    Working,
    WaitingBackground,
    WaitingApproval,
    Done,
    Idle,
    Goal,
}

impl IconTarget {
    pub const ALL: [Self; 17] = [
        Self::Group,
        Self::SplitVertical,
        Self::SplitHorizontal,
        Self::Folder,
        Self::Terminal,
        Self::Editor,
        Self::Board,
        Self::Claude,
        Self::Codex,
        Self::Antigravity,
        Self::OtherAgent,
        Self::Working,
        Self::WaitingBackground,
        Self::WaitingApproval,
        Self::Done,
        Self::Idle,
        Self::Goal,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Group => "Group",
            Self::SplitVertical => "Vertical split",
            Self::SplitHorizontal => "Horizontal split",
            Self::Folder => "Folder",
            Self::Terminal => "Terminal",
            Self::Editor => "Editor",
            Self::Board => "Board",
            Self::Claude => "Claude agent",
            Self::Codex => "Codex agent",
            Self::Antigravity => "Antigravity agent",
            Self::OtherAgent => "Other agent",
            Self::Working => "Working",
            Self::WaitingBackground => "Background wait",
            Self::WaitingApproval => "Needs approval",
            Self::Done => "Done",
            Self::Idle => "Idle",
            Self::Goal => "Goal attached",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Group => "group",
            Self::SplitVertical => "split_vertical",
            Self::SplitHorizontal => "split_horizontal",
            Self::Folder => "folder",
            Self::Terminal => "terminal",
            Self::Editor => "editor",
            Self::Board => "board",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::OtherAgent => "other_agent",
            Self::Working => "working",
            Self::WaitingBackground => "waiting_background",
            Self::WaitingApproval => "waiting_approval",
            Self::Done => "done",
            Self::Idle => "idle",
            Self::Goal => "goal",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.key() == key)
    }

    pub const fn default_glyph(self) -> &'static str {
        match self {
            Self::Group => "📁",
            Self::SplitVertical => "▥",
            Self::SplitHorizontal => "▤",
            Self::Folder => "📂",
            Self::Terminal => "📟",
            Self::Editor => "🗒️",
            Self::Board => "▦",
            Self::Claude => "🧠",
            Self::Codex => "⚙️",
            Self::Antigravity => "✦",
            Self::OtherAgent => "🤖",
            Self::Working => "⠋",
            Self::WaitingBackground => "◷",
            Self::WaitingApproval => "?",
            Self::Done => "🔔",
            Self::Idle => "●",
            Self::Goal => "🏁",
        }
    }

    pub const fn suggestions(self) -> &'static [&'static str] {
        match self {
            Self::Group | Self::Folder => &["📁", "📂", "🗂️", "🧺"],
            Self::SplitVertical | Self::SplitHorizontal => &["▥", "▤", "⊞", "▦"],
            Self::Terminal => &["📟", "▸", "⌘", "🖥️"],
            Self::Editor => &["📝", "✎", "📄", "✦"],
            Self::Board => &["▦", "▤", "☷", "🗃️"],
            Self::Claude => &["🧠", "🪄", "🧭", "🦀"],
            Self::Codex => &["⚙️", "🛠️", "🧬", "📖"],
            Self::Antigravity => &["✦", "🛰️", "🪐", "⚛️"],
            Self::OtherAgent => &["🤖", "✦", "◈", "◆"],
            Self::Working => &["⠋", "◌", "◒", "✦"],
            Self::WaitingBackground => &["◷", "◴", "⌛", "⏳"],
            Self::WaitingApproval => &["?", "!", "⚑", "✋"],
            Self::Done => &["🔔", "✓", "●", "✦"],
            Self::Idle => &["●", "·", "○", "—"],
            Self::Goal => &["🏁", "⚑", "◆", "✦"],
        }
    }
}

/// The persisted, globally shared icon choices. All fields remain public so
/// the renderer has a simple, allocation-free lookup path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconSettings {
    pub group: String,
    pub split_vertical: String,
    pub split_horizontal: String,
    pub folder: String,
    pub terminal: String,
    pub editor: String,
    pub board: String,
    pub claude: String,
    pub codex: String,
    pub antigravity: String,
    pub other_agent: String,
    pub working: String,
    pub waiting_background: String,
    pub waiting_approval: String,
    pub done: String,
    pub idle: String,
    pub goal: String,
}

impl Default for IconSettings {
    fn default() -> Self {
        Self::from_target(|target| target.default_glyph().to_string())
    }
}

impl IconSettings {
    pub fn from_target(mut value: impl FnMut(IconTarget) -> String) -> Self {
        Self {
            group: value(IconTarget::Group),
            split_vertical: value(IconTarget::SplitVertical),
            split_horizontal: value(IconTarget::SplitHorizontal),
            folder: value(IconTarget::Folder),
            terminal: value(IconTarget::Terminal),
            editor: value(IconTarget::Editor),
            board: value(IconTarget::Board),
            claude: value(IconTarget::Claude),
            codex: value(IconTarget::Codex),
            antigravity: value(IconTarget::Antigravity),
            other_agent: value(IconTarget::OtherAgent),
            working: value(IconTarget::Working),
            waiting_background: value(IconTarget::WaitingBackground),
            waiting_approval: value(IconTarget::WaitingApproval),
            done: value(IconTarget::Done),
            idle: value(IconTarget::Idle),
            goal: value(IconTarget::Goal),
        }
    }

    pub fn glyph(&self, target: IconTarget) -> &str {
        match target {
            IconTarget::Group => &self.group,
            IconTarget::SplitVertical => &self.split_vertical,
            IconTarget::SplitHorizontal => &self.split_horizontal,
            IconTarget::Folder => &self.folder,
            IconTarget::Terminal => &self.terminal,
            IconTarget::Editor => &self.editor,
            IconTarget::Board => &self.board,
            IconTarget::Claude => &self.claude,
            IconTarget::Codex => &self.codex,
            IconTarget::Antigravity => &self.antigravity,
            IconTarget::OtherAgent => &self.other_agent,
            IconTarget::Working => &self.working,
            IconTarget::WaitingBackground => &self.waiting_background,
            IconTarget::WaitingApproval => &self.waiting_approval,
            IconTarget::Done => &self.done,
            IconTarget::Idle => &self.idle,
            IconTarget::Goal => &self.goal,
        }
    }

    pub fn set(&mut self, target: IconTarget, glyph: String) {
        let slot = match target {
            IconTarget::Group => &mut self.group,
            IconTarget::SplitVertical => &mut self.split_vertical,
            IconTarget::SplitHorizontal => &mut self.split_horizontal,
            IconTarget::Folder => &mut self.folder,
            IconTarget::Terminal => &mut self.terminal,
            IconTarget::Editor => &mut self.editor,
            IconTarget::Board => &mut self.board,
            IconTarget::Claude => &mut self.claude,
            IconTarget::Codex => &mut self.codex,
            IconTarget::Antigravity => &mut self.antigravity,
            IconTarget::OtherAgent => &mut self.other_agent,
            IconTarget::Working => &mut self.working,
            IconTarget::WaitingBackground => &mut self.waiting_background,
            IconTarget::WaitingApproval => &mut self.waiting_approval,
            IconTarget::Done => &mut self.done,
            IconTarget::Idle => &mut self.idle,
            IconTarget::Goal => &mut self.goal,
        };
        *slot = glyph;
    }
}

/// One selectable glyph and the words used to find it in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IconCatalogEntry {
    pub glyph: &'static str,
    pub name: &'static str,
}

/// A detailed Unicode CLDR subgroup. The official data gives the picker more
/// than a hundred useful categories (for example Animals · Birds, Computer,
/// Mathematics, Warning and Country flags), rather than one unwieldy list.
#[derive(Debug)]
pub struct IconCategory {
    pub label: &'static str,
    pub family: IconCatalogFamily,
    pub entries: Vec<IconCatalogEntry>,
}

/// Rendering contract for the two intentionally separate catalogue sections.
/// Official glyphs are portable UTF-8; Nerd Font glyphs live in Unicode's
/// Private Use Area and require a Nerd Font terminal to render as designed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconCatalogFamily {
    OfficialUtf8,
    NerdFont,
}

impl IconCatalogFamily {
    pub const fn label(self) -> &'static str {
        match self {
            Self::OfficialUtf8 => "Official UTF-8",
            Self::NerdFont => "Nerd Font",
        }
    }
}

/// Returns the complete selectable catalogue. It combines the full English
/// Unicode emoji data set, the compact terminal symbols used by ilium, and
/// the complete Nerd Font icon ranges installed on the developer machine.
///
/// The `emoji` crate owns the Unicode data and its CLDR group/subgroup/name
/// metadata. Nerd Font ranges are Unicode Private Use Area contracts, so we
/// generate their glyphs once here instead of embedding a brittle 10,000-line
/// source fixture. A Nerd Font terminal renders those categories as the
/// intended developer icons; every standard Unicode choice remains portable.
pub fn icon_categories() -> &'static [IconCategory] {
    static CATALOGUE: OnceLock<Vec<IconCategory>> = OnceLock::new();
    CATALOGUE.get_or_init(build_icon_categories)
}

/// One visible chapter in the picker document. The picker owns no duplicate
/// category state: a query simply removes chapters that have no matching
/// entries, preserving the catalogue's official-first ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPickerChapter {
    pub label: &'static str,
    pub family: IconCatalogFamily,
    pub entries: Vec<IconCatalogEntry>,
}

/// Cached search result owned by an open picker. Filtering 12,000 choices is
/// intentionally done only when the query changes, never while a user moves
/// through the viewport or the terminal redraws an animation frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPickerSearchResults {
    pub chapters: Vec<IconPickerChapter>,
    pub entry_count: usize,
}

impl IconPickerSearchResults {
    pub fn entry(&self, selected_entry: usize) -> Option<IconCatalogEntry> {
        self.chapters
            .iter()
            .flat_map(|chapter| chapter.entries.iter().copied())
            .nth(selected_entry)
    }
}

/// Returns the picker as a category-preserving document. A category-name
/// match deliberately keeps that complete chapter, while an icon-name match
/// keeps only the matching icons inside its original chapter.
pub fn picker_chapters(search_query: &str) -> Vec<IconPickerChapter> {
    picker_search_results(search_query).chapters
}

/// Computes one immutable filtered document. Query normalization happens
/// once; every matching test uses allocation-free ASCII case comparison.
pub fn picker_search_results(search_query: &str) -> IconPickerSearchResults {
    let normalized_query = search_query.trim().to_ascii_lowercase();
    let chapters = icon_categories()
        .iter()
        .filter_map(|category| {
            let category_matches =
                ascii_case_insensitive_contains(category.label, &normalized_query);
            let entries = category
                .entries
                .iter()
                .copied()
                .filter(|entry| {
                    normalized_query.is_empty()
                        || category_matches
                        || ascii_case_insensitive_contains(entry.name, &normalized_query)
                })
                .collect::<Vec<_>>();
            (!entries.is_empty()).then_some(IconPickerChapter {
                label: category.label,
                family: category.family,
                entries,
            })
        })
        .collect::<Vec<_>>();
    let entry_count = chapters.iter().map(|chapter| chapter.entries.len()).sum();
    IconPickerSearchResults {
        chapters,
        entry_count,
    }
}

fn ascii_case_insensitive_contains(haystack: &str, query: &str) -> bool {
    query.is_empty()
        || haystack
            .as_bytes()
            .windows(query.len())
            .any(|candidate| candidate.eq_ignore_ascii_case(query.as_bytes()))
}

/// Compatibility lookup for callers that need a flat sequence. New picker
/// rendering uses [`picker_chapters`] so category structure stays visible.
pub fn picker_entries(category_index: usize, search_query: &str) -> Vec<IconCatalogEntry> {
    let chapters = picker_chapters(search_query);
    if search_query.trim().is_empty() {
        return icon_categories()
            .get(category_index)
            .map(|category| category.entries.clone())
            .unwrap_or_default();
    }
    chapters
        .into_iter()
        .flat_map(|chapter| chapter.entries)
        .collect()
}

/// Resolves a selected document ordinal to its glyph. Keyboard and mouse
/// selection use this shared lookup so neither has to recreate filtering.
pub fn picker_entry(search_query: &str, selected_entry: usize) -> Option<IconCatalogEntry> {
    picker_chapters(search_query)
        .into_iter()
        .flat_map(|chapter| chapter.entries)
        .nth(selected_entry)
}

/// Counts the currently visible document entries without exposing a second
/// representation of the category-preserving picker.
pub fn picker_entry_count(search_query: &str) -> usize {
    picker_chapters(search_query)
        .iter()
        .map(|chapter| chapter.entries.len())
        .sum()
}

/// Counts actual choices, not categories, for an honest picker status line.
pub fn catalogue_icon_count() -> usize {
    icon_categories()
        .iter()
        .map(|category| category.entries.len())
        .sum()
}

/// Counts the selectable choices in one deliberate family boundary.
pub fn catalogue_icon_count_for(family: IconCatalogFamily) -> usize {
    icon_categories()
        .iter()
        .filter(|category| category.family == family)
        .map(|category| category.entries.len())
        .sum()
}

/// Builds the category list from the official Unicode CLDR classifications.
fn build_icon_categories() -> Vec<IconCategory> {
    let mut official_grouped = BTreeMap::<&'static str, Vec<IconCatalogEntry>>::new();
    for emoji in emoji::lookup_by_name::iter_emoji() {
        if !matches!(emoji.status, emoji::Status::FullyQualified) {
            continue;
        }
        official_grouped
            .entry(category_label(emoji.subgroup))
            .or_default()
            .push(IconCatalogEntry {
                glyph: emoji.glyph,
                name: emoji.name,
            });
    }
    let mut categories = vec![IconCategory {
        label: "Quick picks",
        family: IconCatalogFamily::OfficialUtf8,
        entries: QUICK_PICK_GLYPHS
            .iter()
            .map(|(glyph, name)| IconCatalogEntry { glyph, name })
            .collect(),
    }];
    categories.extend(official_grouped.into_iter().map(|(label, mut entries)| {
        entries.sort_unstable_by_key(|entry| entry.name);
        IconCategory {
            label,
            family: IconCatalogFamily::OfficialUtf8,
            entries,
        }
    }));
    categories.extend(nerd_font_icon_categories());
    categories
}

/// Adds the major Nerd Font v3 icon families. These are intentionally kept as
/// separate categories: a user looking for a coding glyph should not have to
/// sift through Material symbols, weather icons, or Powerline separators.
fn nerd_font_icon_categories() -> Vec<IconCategory> {
    const RANGES: &[(&str, &str, u32, u32)] = &[
        ("Nerd Font · Codicons", "Codicon", 0xEA60, 0xEC1E),
        ("Nerd Font · Devicons", "Devicon", 0xE700, 0xE7C5),
        ("Nerd Font · Font Awesome", "Font Awesome", 0xF000, 0xF2FF),
        (
            "Nerd Font · Material Design",
            "Material Design",
            0xF0001,
            0xF1AF0,
        ),
        ("Nerd Font · Octicons", "Octicon", 0xF400, 0xF4A8),
        ("Nerd Font · Powerline", "Powerline", 0xE0A0, 0xE0D7),
        ("Nerd Font · Seti UI", "Seti UI", 0xE5FA, 0xE62B),
        ("Nerd Font · Weather", "Weather", 0xE300, 0xE3E3),
    ];

    RANGES
        .iter()
        .map(|(label, family, start, end)| {
            let mut entries = Vec::with_capacity((end - start + 1) as usize);
            for codepoint in *start..=*end {
                let Some(glyph) = char::from_u32(codepoint) else {
                    continue;
                };
                entries.push(IconCatalogEntry {
                    glyph: Box::leak(glyph.to_string().into_boxed_str()),
                    name: Box::leak(format!("{family} icon U+{codepoint:04X}").into_boxed_str()),
                });
            }
            IconCategory {
                label,
                family: IconCatalogFamily::NerdFont,
                entries,
            }
        })
        .collect()
}

/// Existing carefully chosen terminal-friendly symbols remain available even
/// though they are not all fully-qualified emoji.
const QUICK_PICK_GLYPHS: &[(&str, &str)] = &[
    ("📁", "folder"),
    ("📂", "open folder"),
    ("🗂️", "card index"),
    ("▥", "vertical split"),
    ("▤", "horizontal split"),
    ("⊞", "squared plus"),
    ("▦", "grid"),
    ("☷", "trigram"),
    ("📟", "pager"),
    ("🖥️", "computer"),
    ("⌘", "command key"),
    ("▸", "right triangle"),
    ("📝", "memo"),
    ("✎", "pencil"),
    ("📄", "document"),
    ("🗃️", "card file"),
    ("🧠", "brain"),
    ("🪄", "magic wand"),
    ("🧭", "compass"),
    ("🦀", "crab"),
    ("🦞", "lobster"),
    ("⚙️", "gear"),
    ("🛠️", "tools"),
    ("🧬", "DNA"),
    ("📖", "open book"),
    ("🤖", "robot"),
    ("🛰️", "satellite"),
    ("🪐", "ringed planet"),
    ("⚛️", "atom"),
    ("⠋", "braille spinner"),
    ("◌", "dotted circle"),
    ("◒", "half circle"),
    ("✦", "four point star"),
    ("◷", "seven clock"),
    ("◴", "three clock"),
    ("⌛", "hourglass"),
    ("⏳", "flowing hourglass"),
    ("?", "question mark"),
    ("!", "exclamation mark"),
    ("⚑", "outlined flag"),
    ("✋", "raised hand"),
    ("🔔", "bell"),
    ("✓", "check mark"),
    ("●", "filled circle"),
    ("○", "empty circle"),
    ("·", "middle dot"),
    ("◆", "filled diamond"),
    ("◇", "empty diamond"),
    ("◈", "diamond dot"),
    ("⬡", "hexagon"),
    ("⬢", "filled hexagon"),
    ("✚", "cross"),
    ("✿", "flower"),
    ("☀", "sun"),
    ("☁", "cloud"),
    ("☾", "moon"),
    ("⚡", "lightning"),
    ("∞", "infinity"),
];

/// Turns CLDR's terse subgroup keys into readable terminal labels while
/// preserving all its fine-grained categories.
fn category_label(subgroup: &'static str) -> &'static str {
    match subgroup {
        "animal-mammal" => "Animals · Mammals",
        "animal-bird" => "Animals · Birds",
        "animal-amphibian" => "Animals · Amphibians",
        "animal-reptile" => "Animals · Reptiles",
        "animal-marine" => "Animals · Marine",
        "animal-bug" => "Animals · Insects",
        "computer" => "Computer",
        "math" => "Mathematics",
        "other-symbol" => "Signs & symbols",
        "transport-sign" => "Transport signs",
        "geometric" => "Geometric shapes",
        "arrow" => "Arrows",
        "warning" => "Warnings",
        "currency" => "Currency",
        "keycap" => "Keycaps",
        "alphanum" => "Letters & numbers",
        "food-fruit" => "Food · Fruit",
        "food-vegetable" => "Food · Vegetables",
        "food-marine" => "Food · Seafood",
        "food-sweet" => "Food · Sweets",
        "food-prepared" => "Food · Prepared",
        "sky & weather" => "Sky & weather",
        "transport-air" => "Travel · Air",
        "transport-ground" => "Travel · Ground",
        "transport-water" => "Travel · Water",
        "place-map" => "Places · Maps",
        "place-geographic" => "Places · Geographic",
        "place-building" => "Places · Buildings",
        "place-religious" => "Places · Religious",
        "time" => "Time",
        "tool" => "Tools",
        "book-paper" => "Books & paper",
        "clothing" => "Clothing",
        "game" => "Games",
        "arts & crafts" => "Arts & crafts",
        _ => subgroup,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        catalogue_icon_count, catalogue_icon_count_for, icon_categories, picker_chapters,
        picker_entries, IconCatalogFamily,
    };

    #[test]
    fn catalogue_contains_thousands_of_named_icons_in_detailed_categories() {
        assert!(catalogue_icon_count() >= 10_680);
        assert!(icon_categories().len() >= 50);
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Animals · Mammals"));
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Computer"));
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Mathematics"));
        assert!(icon_categories()
            .iter()
            .any(|category| category.label == "Nerd Font · Material Design"));
        assert!(catalogue_icon_count_for(IconCatalogFamily::OfficialUtf8) >= 3_500);
        assert!(catalogue_icon_count_for(IconCatalogFamily::NerdFont) >= 8_000);
        let nerd_font_start = icon_categories()
            .iter()
            .position(|category| category.family == IconCatalogFamily::NerdFont)
            .expect("the catalogue has a Nerd Font section");
        assert!(nerd_font_start > 0);
        assert!(icon_categories()[..nerd_font_start]
            .iter()
            .all(|category| category.family == IconCatalogFamily::OfficialUtf8));
        assert!(icon_categories()[nerd_font_start..]
            .iter()
            .all(|category| category.family == IconCatalogFamily::NerdFont));
    }

    #[test]
    fn search_finds_icons_by_official_name_and_category() {
        assert!(picker_entries(0, "rocket")
            .iter()
            .any(|entry| entry.glyph == "🚀"));
        assert!(!picker_entries(0, "mathematics").is_empty());
    }

    #[test]
    fn chapter_search_preserves_family_order_and_removes_empty_chapters() {
        let chapters = picker_chapters("rocket");
        assert!(!chapters.is_empty());
        assert!(chapters.iter().all(|chapter| !chapter.entries.is_empty()));
        assert!(chapters.iter().any(|chapter| {
            chapter
                .entries
                .iter()
                .any(|entry| entry.glyph == "🚀" && entry.name.contains("rocket"))
        }));

        let all_chapters = picker_chapters("");
        let first_nerd_font = all_chapters
            .iter()
            .position(|chapter| chapter.family == IconCatalogFamily::NerdFont)
            .expect("the picker has Nerd Font chapters");
        assert!(all_chapters[..first_nerd_font]
            .iter()
            .all(|chapter| chapter.family == IconCatalogFamily::OfficialUtf8));
    }
}
