//! Catalogues, language choice, plurals and number formats (guide-internationalisation,
//! USR-310 .. USR-317).

use std::path::Path;

use ilium_plugin_api::PluginError;

/// A loaded plugin catalogue.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalogue {
    _private: (),
}

impl Catalogue {
    /// The language tag of the catalogue.
    pub fn tag(&self) -> &str {
        unimplemented!("host::i18n::Catalogue::tag")
    }

    /// The plural form of a message for `count` in `tag`.
    pub fn plural_text(&self, _tag: &str, _message_id: &str, _count: u64) -> String {
        unimplemented!("host::i18n::Catalogue::plural_text")
    }
}

/// Loads a plugin catalogue file.
pub fn load_plugin_catalogue(_path: &Path) -> Result<Catalogue, PluginError> {
    unimplemented!("host::i18n::load_plugin_catalogue")
}

/// The environment variables that choose the UI language.
#[derive(Debug, Clone, PartialEq)]
pub struct LangEnv {
    _private: (),
}

impl LangEnv {
    /// Builds an environment from name and value pairs.
    pub fn from_pairs(_pairs: &[(&str, &str)]) -> LangEnv {
        unimplemented!("host::i18n::LangEnv::from_pairs")
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

/// Chooses the UI language from the setting and the environment.
pub fn choose_ui_language(_env: &LangEnv, _setting: Option<&str>) -> LanguageChoice {
    unimplemented!("host::i18n::choose_ui_language")
}

/// Message lookup with fallback from region to language to English.
#[derive(Debug)]
pub struct Lookup {
    _private: (),
}

impl Lookup {
    /// Builds a lookup from `(tag, [(message id, text)])` entries.
    pub fn from_entries(_entries: &[(&str, &[(&str, &str)])]) -> Lookup {
        unimplemented!("host::i18n::Lookup::from_entries")
    }

    /// The text of a message in a language.
    pub fn text(&self, _tag: &str, _message_id: &str) -> String {
        unimplemented!("host::i18n::Lookup::text")
    }

    /// The log codes written by lookups so far.
    pub fn log_codes(&self) -> Vec<String> {
        unimplemented!("host::i18n::Lookup::log_codes")
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

/// The plural category of `count` in `tag`.
pub fn plural_category(_tag: &str, _count: u64) -> PluralCategory {
    unimplemented!("host::i18n::plural_category")
}

/// Formats a number for a language.
pub fn format_number(_value: f64, _tag: &str) -> String {
    unimplemented!("host::i18n::format_number")
}

/// Formats a log timestamp for a language.
pub fn format_log_time(_unix_ms: u64, _tag: &str) -> String {
    unimplemented!("host::i18n::format_log_time")
}

/// The host text of a message in a language.
pub fn text_for(_tag: &str, _message_id: &str) -> String {
    unimplemented!("host::i18n::text_for")
}
