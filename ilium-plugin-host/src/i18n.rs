//! Catalogues, language choice, plurals and number formats (guide-internationalisation,
//! USR-310 .. USR-317).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use ilium_plugin_api::{ErrorCode, PluginError};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

/// Largest catalogue file the loader accepts (USR-310).
const MAX_CATALOGUE_BYTES: usize = 256 * 1024;

/// One message of a catalogue: a plain text or a set of plural forms keyed by CLDR category.
#[derive(Debug, Clone, PartialEq)]
enum Message {
    Plain(String),
    Plural(BTreeMap<String, String>),
}

/// A loaded plugin catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalogue {
    tag: String,
    messages: BTreeMap<String, Message>,
}

impl Catalogue {
    /// The language tag of the catalogue.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The plural form of a message for `count` in `tag`.
    pub fn plural_text(&self, tag: &str, message_id: &str, count: u64) -> String {
        match self.messages.get(message_id) {
            Some(Message::Plain(text)) => text.clone(),
            Some(Message::Plural(forms)) => {
                let category = category_name(plural_category(tag, count));
                forms
                    .get(category)
                    .or_else(|| forms.get("other"))
                    .cloned()
                    .unwrap_or_else(|| message_id.to_string())
            }
            None => message_id.to_string(),
        }
    }
}

/// The key/value pairs of a JSON object in file order, so duplicate ids can be seen.
struct OrderedObject(Vec<(String, serde_json::Value)>);

impl<'de> Deserialize<'de> for OrderedObject {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = OrderedObject;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, serde_json::Value>()? {
                    entries.push(entry);
                }
                Ok(OrderedObject(entries))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

fn invalid_catalogue(file: &str, detail: impl std::fmt::Display) -> PluginError {
    PluginError::new(
        ErrorCode::InvalidArgument,
        format!("i18n.catalogue-invalid {file}: {detail}"),
    )
}

/// Loads a plugin catalogue file.
pub fn load_plugin_catalogue(path: &Path) -> Result<Catalogue, PluginError> {
    let file = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|error| invalid_catalogue(&file, error))?;
    if bytes.len() > MAX_CATALOGUE_BYTES {
        return Err(invalid_catalogue(&file, "larger than 256 KiB"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid_catalogue(&file, "invalid UTF-8"))?;
    let object: OrderedObject =
        serde_json::from_str(text).map_err(|error| invalid_catalogue(&file, error))?;
    let tag = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut messages = BTreeMap::new();
    for (message_id, value) in object.0 {
        let message = match value {
            serde_json::Value::String(text) => Message::Plain(text),
            serde_json::Value::Object(forms) => {
                let mut plural = BTreeMap::new();
                for (category, form) in forms {
                    let serde_json::Value::String(form) = form else {
                        return Err(invalid_catalogue(
                            &file,
                            format!("{message_id}: plural form {category} is not a string"),
                        ));
                    };
                    plural.insert(category, form);
                }
                if !plural.contains_key("other") {
                    return Err(invalid_catalogue(
                        &file,
                        format!("{message_id}: the plural message has no `other` form"),
                    ));
                }
                Message::Plural(plural)
            }
            _ => {
                return Err(invalid_catalogue(
                    &file,
                    format!("{message_id}: the value is not a string"),
                ))
            }
        };
        if messages.insert(message_id.clone(), message).is_some() {
            return Err(invalid_catalogue(
                &file,
                format!("{message_id}: duplicate message id"),
            ));
        }
    }
    Ok(Catalogue { tag, messages })
}

/// The environment variables that choose the UI language.
#[derive(Debug, Clone, PartialEq)]
pub struct LangEnv {
    variables: BTreeMap<String, String>,
}

impl LangEnv {
    /// Builds an environment from name and value pairs.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> LangEnv {
        LangEnv {
            variables: pairs
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        }
    }
}

/// The chosen UI language and where the choice came from.
#[derive(Debug, Clone, PartialEq)]
pub struct LanguageChoice {
    /// The language tag.
    pub tag: String,
    /// The source of the choice, for example `ILIUM_LANG`.
    pub source: String,
}

/// Reduces a locale such as `de_DE.UTF-8` to its language, `de`; `C` and `POSIX` name no language.
fn language_of_locale(locale: &str) -> Option<String> {
    let base = locale.split(['.', '@']).next().unwrap_or("");
    let language = base.split(['_', '-']).next().unwrap_or("");
    if language.is_empty() || language == "C" || language == "POSIX" {
        return None;
    }
    Some(language.to_ascii_lowercase())
}

/// Chooses the UI language from the setting and the environment.
pub fn choose_ui_language(env: &LangEnv, setting: Option<&str>) -> LanguageChoice {
    if let Some(tag) = setting.filter(|tag| !tag.is_empty()) {
        return LanguageChoice {
            tag: tag.to_string(),
            source: "[ui] language".to_string(),
        };
    }
    if let Some(tag) = env
        .variables
        .get("ILIUM_LANG")
        .filter(|tag| !tag.is_empty())
    {
        return LanguageChoice {
            tag: tag.clone(),
            source: "ILIUM_LANG".to_string(),
        };
    }
    for variable in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        let language = env
            .variables
            .get(variable)
            .and_then(|value| language_of_locale(value));
        if let Some(tag) = language {
            return LanguageChoice {
                tag,
                source: variable.to_string(),
            };
        }
    }
    LanguageChoice {
        tag: "en".to_string(),
        source: "default".to_string(),
    }
}

/// Message lookup with fallback from region to language to English.
#[derive(Debug)]
pub struct Lookup {
    tables: BTreeMap<String, BTreeMap<String, String>>,
    logged: Mutex<(BTreeSet<(String, String)>, Vec<String>)>,
}

/// Number of `{n}` placeholders in a message.
fn placeholder_count(text: &str) -> usize {
    let mut count = 0;
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if !after[..end].is_empty() && after[..end].bytes().all(|b| b.is_ascii_digit()) => {
                count += 1;
                rest = &after[end + 1..];
            }
            _ => rest = after,
        }
    }
    count
}

impl Lookup {
    /// Builds a lookup from `(tag, [(message id, text)])` entries.
    pub fn from_entries(entries: &[(&str, &[(&str, &str)])]) -> Lookup {
        let mut tables: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (tag, messages) in entries {
            let table = tables.entry(tag.to_string()).or_default();
            for (message_id, text) in *messages {
                table.insert(message_id.to_string(), text.to_string());
            }
        }
        Lookup {
            tables,
            logged: Mutex::new((BTreeSet::new(), Vec::new())),
        }
    }

    fn find(&self, tag: &str, message_id: &str) -> Option<&String> {
        self.tables.get(tag).and_then(|table| table.get(message_id))
    }

    /// The text of a message in a language.
    pub fn text(&self, tag: &str, message_id: &str) -> String {
        let english = self.find("en", message_id);
        let language = tag.split('-').next().unwrap_or(tag);
        let candidate = self
            .find(tag, message_id)
            .or_else(|| self.find(language, message_id));
        if let Some(text) = candidate {
            let matches_english = english
                .map(|english| placeholder_count(english) == placeholder_count(text))
                .unwrap_or(true);
            if matches_english {
                return text.clone();
            }
            self.log_once(tag, message_id, "i18n.placeholder-mismatch");
        }
        english
            .cloned()
            .unwrap_or_else(|| message_id.to_string())
    }

    fn log_once(&self, tag: &str, message_id: &str, code: &str) {
        // A poisoned lock only means another lookup panicked; the log state is still usable.
        let mut guard = match self.logged.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.0.insert((tag.to_string(), message_id.to_string())) {
            guard.1.push(code.to_string());
        }
    }

    /// The log codes written by lookups so far.
    pub fn log_codes(&self) -> Vec<String> {
        match self.logged.lock() {
            Ok(guard) => guard.1.clone(),
            Err(poisoned) => poisoned.into_inner().1.clone(),
        }
    }
}

/// A CLDR plural category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluralCategory {
    /// Zero.
    Zero,
    /// One.
    One,
    /// Two.
    Two,
    /// Few.
    Few,
    /// Many.
    Many,
    /// Other.
    Other,
}

fn category_name(category: PluralCategory) -> &'static str {
    match category {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}

fn language_part(tag: &str) -> String {
    tag.split(['-', '_']).next().unwrap_or(tag).to_ascii_lowercase()
}

/// The plural category of `count` in `tag`.
pub fn plural_category(tag: &str, count: u64) -> PluralCategory {
    let (mod10, mod100) = (count % 10, count % 100);
    match language_part(tag).as_str() {
        "ja" | "zh" | "ko" | "vi" | "th" | "id" => PluralCategory::Other,
        "fr" | "pt" => {
            if count <= 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
        "ru" | "uk" | "be" => {
            if mod10 == 1 && mod100 != 11 {
                PluralCategory::One
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                PluralCategory::Few
            } else {
                PluralCategory::Many
            }
        }
        "pl" => {
            if count == 1 {
                PluralCategory::One
            } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                PluralCategory::Few
            } else {
                PluralCategory::Many
            }
        }
        "cs" | "sk" => match count {
            1 => PluralCategory::One,
            2..=4 => PluralCategory::Few,
            _ => PluralCategory::Other,
        },
        "ar" => match count {
            0 => PluralCategory::Zero,
            1 => PluralCategory::One,
            2 => PluralCategory::Two,
            _ if (3..=10).contains(&mod100) => PluralCategory::Few,
            _ if (11..=99).contains(&mod100) => PluralCategory::Many,
            _ => PluralCategory::Other,
        },
        _ => {
            if count == 1 {
                PluralCategory::One
            } else {
                PluralCategory::Other
            }
        }
    }
}

/// Group and decimal separators of a language.
fn separators(tag: &str) -> (&'static str, &'static str) {
    match language_part(tag).as_str() {
        "de" | "es" | "it" | "pt" | "nl" | "id" | "tr" | "da" => (".", ","),
        "fr" => ("\u{202f}", ","),
        "ru" | "sv" | "pl" | "cs" | "nb" | "fi" | "uk" => ("\u{a0}", ","),
        _ => (",", "."),
    }
}

/// Formats a number for a language.
pub fn format_number(value: f64, tag: &str) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let (group, decimal) = separators(tag);
    let plain = value.abs().to_string();
    let (integer, fraction) = match plain.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (plain.as_str(), None),
    };
    let mut grouped = String::new();
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push_str(group);
        }
        grouped.push(digit);
    }
    let sign = if value < 0.0 { "-" } else { "" };
    match fraction {
        Some(fraction) => format!("{sign}{grouped}{decimal}{fraction}"),
        None => format!("{sign}{grouped}"),
    }
}

/// Formats a log timestamp for a language. Logs are language independent: the result is always
/// `YYYY-MM-DD HH:mm:ss` in UTC (USR-314).
pub fn format_log_time(unix_ms: u64, _tag: &str) -> String {
    let seconds = unix_ms / 1000;
    let days = (seconds / 86_400) as i64;
    let second_of_day = seconds % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60
    )
}

/// English host texts, the source of every other language and of the `en-XA` pseudo-locale.
fn english_host_text(message_id: &str) -> Option<&'static str> {
    Some(match message_id {
        "host.perm.fs.read" => "Read files in the folders you allow",
        "host.perm.fs.write" => "Create and change files in the folders you allow",
        "host.perm.net" => "Connect to the network hosts you allow",
        "host.perm.process" => "Run programs on your computer",
        "host.perm.clipboard" => "Read and change the clipboard",
        "host.perm.terminal.read" => "Read what a terminal pane shows",
        "host.perm.terminal.write" => "Type into a terminal pane",
        _ => return None,
    })
}

/// Expands `text` by about 50 percent with accented letters inside brackets.
fn pseudo_localise(text: &str) -> String {
    let accented: String = text
        .chars()
        .map(|letter| match letter {
            'a' => '\u{e1}',
            'e' => '\u{e9}',
            'i' => '\u{ed}',
            'o' => '\u{f3}',
            'u' => '\u{fa}',
            'A' => '\u{c1}',
            'E' => '\u{c9}',
            'I' => '\u{cd}',
            'O' => '\u{d3}',
            'U' => '\u{da}',
            other => other,
        })
        .collect();
    let original = text.chars().count();
    let padding = (original * 3).div_ceil(5) + 1;
    format!("[{accented}{}]", "~".repeat(padding))
}

/// The host text of a message in a language.
pub fn text_for(tag: &str, message_id: &str) -> String {
    let english = english_host_text(message_id).unwrap_or(message_id);
    if tag.eq_ignore_ascii_case("en-XA") {
        return pseudo_localise(english);
    }
    english.to_string()
}
