//! `ilium plugin search [query] [--category c] [--sort relevance|name|updated] [--limit n]
//! [--offset n] [--registry url] [--offline]` (reference-registry.md PKG-379..PKG-384).

use super::{offline, registry_access, CliEnv, CliOutput};
use serde_json::{json, Value};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// The closed category list (PKG-384).
pub const CATEGORIES: &[&str] = &[
    "agents",
    "appearance",
    "scenes",
    "editors",
    "git",
    "inference",
    "notifications",
    "search",
    "terminal",
    "tools",
    "voice",
];

const DEFAULT_LIMIT: usize = 20;
const MAX_QUERY_CHARS: usize = 200;

/// Case and accent folding: NFKD, combining marks removed, lower case (PKG-379).
pub fn fold(text: &str) -> String {
    text.nfkd()
        .filter(|character| !is_combining_mark(*character))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `term` is a prefix of a word of `field` (a word starts after a non-alphanumeric
/// character or at the start of the field).
fn word_prefix(field: &str, term: &str) -> bool {
    let mut previous_alphanumeric = false;
    for (index, character) in field.char_indices() {
        let starts_word = character.is_alphanumeric() && !previous_alphanumeric;
        if starts_word && field[index..].starts_with(term) {
            return true;
        }
        previous_alphanumeric = character.is_alphanumeric();
    }
    false
}

struct Fields {
    id: String,
    name: String,
    keywords: String,
    publisher: String,
    description: String,
}

impl Fields {
    fn of(entry: &Value) -> Self {
        let text = |key: &str| fold(entry[key].as_str().unwrap_or_default());
        let keywords = entry["keywords"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        Fields {
            id: text("id"),
            name: text("name"),
            keywords: fold(&keywords),
            publisher: text("publisher"),
            description: text("description"),
        }
    }

    /// The best field weight for one term, 0 when it matches nowhere (PKG-380).
    fn weight(&self, term: &str) -> u32 {
        [
            (&self.id, 8),
            (&self.name, 6),
            (&self.keywords, 4),
            (&self.publisher, 2),
            (&self.description, 1),
        ]
        .iter()
        .filter(|(field, _)| word_prefix(field, term))
        .map(|(_, weight)| *weight)
        .max()
        .unwrap_or(0)
    }
}

/// The relevance score of an entry, `None` when a term matches no field.
fn score(entry: &Value, terms: &[String]) -> Option<u32> {
    let fields = Fields::of(entry);
    let mut total = 0;
    for term in terms {
        match fields.weight(term) {
            0 => return None,
            weight => total += weight,
        }
    }
    Some(total)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    Relevance,
    Name,
    Updated,
}

struct Options {
    query: Vec<String>,
    category: Option<String>,
    order: Option<Order>,
    limit: usize,
    offset: usize,
    registry: Option<String>,
    offline: bool,
}

fn invalid(message: String) -> CliOutput {
    offline::failure("usage.invalid-value", message, 2)
}

fn parse(args: &[String]) -> Result<Options, CliOutput> {
    let mut options = Options {
        query: Vec::new(),
        category: None,
        order: None,
        limit: DEFAULT_LIMIT,
        offset: 0,
        registry: None,
        offline: false,
    };
    let mut words: Vec<String> = Vec::new();
    let mut iterator = args.iter();
    while let Some(argument) = iterator.next() {
        let mut value = |name: &str| {
            iterator.next().cloned().ok_or_else(|| {
                offline::failure("usage.missing-argument", format!("{name} needs a value"), 2)
            })
        };
        match argument.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--offline" => options.offline = true,
            "--category" => {
                let category = value("--category")?;
                if !CATEGORIES.contains(&category.as_str()) {
                    return Err(invalid(format!(
                        "{category} is not a category; use one of: {}",
                        CATEGORIES.join(", ")
                    )));
                }
                options.category = Some(category);
            }
            "--sort" => {
                options.order = Some(match value("--sort")?.as_str() {
                    "relevance" => Order::Relevance,
                    "name" => Order::Name,
                    "updated" => Order::Updated,
                    other => {
                        return Err(invalid(format!(
                            "{other} is not a sort order; use relevance, name or updated"
                        )))
                    }
                });
            }
            "--limit" => {
                let text = value("--limit")?;
                options.limit = text
                    .parse::<usize>()
                    .ok()
                    .filter(|limit| (1..=100).contains(limit))
                    .ok_or_else(|| invalid(format!("--limit must be 1 to 100, not {text}")))?;
            }
            "--offset" => {
                let text = value("--offset")?;
                options.offset = text
                    .parse::<usize>()
                    .map_err(|_| invalid(format!("--offset must be a number, not {text}")))?;
            }
            "--registry" => options.registry = Some(value("--registry")?),
            flag if flag.starts_with('-') => {
                return Err(offline::failure(
                    "usage.unknown-flag",
                    format!("Unknown flag: {flag}"),
                    2,
                ))
            }
            _ => words.push(argument.clone()),
        }
    }
    if words.join(" ").chars().count() > MAX_QUERY_CHARS {
        return Err(invalid(format!("A query is at most {MAX_QUERY_CHARS} characters")));
    }
    options.query = words
        .iter()
        .flat_map(|word| word.split_whitespace())
        .map(fold)
        .collect();
    Ok(options)
}

fn plugin_record(entry: &Value) -> Value {
    let version = entry["latest_stable"]
        .as_str()
        .or_else(|| entry["latest_beta"].as_str());
    json!({
        "type": "plugin",
        "id": entry["id"],
        "version": version,
        "name": entry["name"],
        "publisher": entry["publisher"],
        "description": entry["description"],
        "categories": entry["categories"],
        "updated": entry["updated"],
        "deprecated": !entry["deprecated"].is_null(),
    })
}

pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let options = match parse(args) {
        Ok(options) => options,
        Err(output) => return output,
    };
    let snapshot = match registry_access::open(env, options.registry.as_deref(), options.offline) {
        Ok(snapshot) => snapshot,
        Err(output) => return output,
    };
    let mut matches: Vec<(u32, Value)> = snapshot
        .index_entries()
        .into_iter()
        .filter(|entry| {
            options.category.as_deref().is_none_or(|wanted| {
                entry["categories"]
                    .as_array()
                    .is_some_and(|list| list.iter().any(|c| c.as_str() == Some(wanted)))
            })
        })
        .filter_map(|entry| score(&entry, &options.query).map(|score| (score, entry)))
        .collect();
    let order = options.order.unwrap_or(if options.query.is_empty() {
        Order::Name
    } else {
        Order::Relevance
    });
    let text = |entry: &Value, key: &str| entry[key].as_str().unwrap_or_default().to_owned();
    matches.sort_by(|(left_score, left), (right_score, right)| {
        let by_updated = text(right, "updated").cmp(&text(left, "updated"));
        let by_id = text(left, "id").cmp(&text(right, "id"));
        match order {
            Order::Relevance => right_score
                .cmp(left_score)
                .then(by_updated)
                .then(by_id),
            Order::Name => fold(&text(left, "name"))
                .cmp(&fold(&text(right, "name")))
                .then(by_id),
            Order::Updated => by_updated.then(by_id),
        }
    });
    let mut records: Vec<Value> = snapshot
        .warnings
        .iter()
        .map(|message| json!({"type": "warning", "code": "registry.stale", "message": message}))
        .collect();
    records.extend(
        matches
            .iter()
            .skip(options.offset)
            .take(options.limit)
            .map(|(_, entry)| plugin_record(entry)),
    );
    offline::finish(records, 0)
}
