//! Application prompt sources embedded at compilation, rendered without file I/O.

mod catalog;
pub use catalog::{agent, catalog, conversion, naming, voice};

use std::sync::OnceLock;

use handlebars::Handlebars;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("invalid embedded prompt: {0}")]
    Template(#[from] handlebars::TemplateError),
    #[error("could not render prompt: {0}")]
    Render(#[from] handlebars::RenderError),
}

fn new_registry() -> Result<Handlebars<'static>, PromptError> {
    let mut registry = Handlebars::new();
    // Prompts are plain text; user values must retain punctuation and markup.
    registry.register_escape_fn(handlebars::no_escape);
    for &(name, source) in catalog() {
        registry.register_template_string(name, source)?;
    }
    Ok(registry)
}

fn registry() -> &'static Handlebars<'static> {
    static REGISTRY: OnceLock<Handlebars<'static>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        // build.rs registers these exact embedded sources with the same engine.
        new_registry().expect("embedded catalog was validated during compilation")
    })
}

pub fn render<T: Serialize>(name: &str, context: &T) -> Result<String, PromptError> {
    Ok(registry().render(name, context)?)
}

/// Renders catalog-owned templates with infallibly serializable JSON values.
/// Names are fixed call-site catalog identifiers; build validation and caller
/// regression tests establish this invariant. No user value is parsed as a template.
pub fn render_value(name: &str, context: &Value) -> String {
    render(name, context).expect("validated catalog name and JSON value render infallibly")
}

/// Shared renderer for callers that already carry a catalog source reference.
/// Tests can supply synthetic sources; production callers use embedded constants.
pub fn render_source<T: Serialize>(
    name: &str,
    source: &str,
    context: &T,
) -> Result<String, PromptError> {
    if let Some(&(catalog_name, _)) =
        catalog().find(|&&(_, catalog_source)| catalog_source == source)
    {
        // Existing callers use human-readable labels such as "session-title".
        // The embedded source identifies the cached entry independently of that
        // diagnostic label; custom sources retain their supplied name below.
        return render(catalog_name, context);
    }
    let mut registry = new_registry()?;
    registry.register_template_string(name, source)?;
    Ok(registry.render(name, context)?)
}
