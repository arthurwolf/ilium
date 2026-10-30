//! `ilium plugin registry add|list|remove` (reference-plugin-distribution-cli.md PKG-438).
//! Pinned registries live in `<config>/config.toml` under `[plugins.registry]`.

use super::{offline, CliEnv, CliOutput};
use serde_json::json;

fn load(env: &CliEnv) -> toml::Table {
    std::fs::read_to_string(env.config_dir.join("config.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .unwrap_or_default()
}

fn section(document: &mut toml::Table) -> &mut toml::Table {
    let plugins = document
        .entry("plugins".to_owned())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !plugins.is_table() {
        *plugins = toml::Value::Table(toml::Table::new());
    }
    let registry = plugins
        .as_table_mut()
        .expect("plugins was just made a table")
        .entry("registry".to_owned())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !registry.is_table() {
        *registry = toml::Value::Table(toml::Table::new());
    }
    registry.as_table_mut().expect("registry was just made a table")
}

fn urls_of(registry: &toml::Table) -> Vec<String> {
    registry
        .get("urls")
        .and_then(toml::Value::as_array)
        .map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn save(env: &CliEnv, document: &toml::Table) -> Result<(), CliOutput> {
    let failure = |error: String| offline::failure("registry.config-write-failed", error, 1);
    std::fs::create_dir_all(&env.config_dir).map_err(|e| failure(e.to_string()))?;
    let text = toml::to_string(document).map_err(|e| failure(e.to_string()))?;
    let path = env.config_dir.join("config.toml");
    let temporary = path.with_extension("toml.tmp");
    std::fs::write(&temporary, text).map_err(|e| failure(e.to_string()))?;
    std::fs::rename(&temporary, &path).map_err(|e| failure(e.to_string()))
}

pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let mut positional = Vec::new();
    let mut root_key = None;
    let mut iterator = args.iter();
    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--root-key" => match iterator.next() {
                Some(value) => root_key = Some(value.clone()),
                None => {
                    return offline::failure("usage.missing-argument", "--root-key needs a value", 2)
                }
            },
            flag if flag.starts_with('-') => {
                return offline::failure("usage.unknown-flag", format!("Unknown flag: {flag}"), 2)
            }
            _ => positional.push(argument.clone()),
        }
    }
    let Some((action, rest)) = positional.split_first() else {
        return offline::failure("usage.missing-argument", "registry needs add, list or remove", 2);
    };
    match action.as_str() {
        "list" => {
            let mut document = load(env);
            let registry = section(&mut document);
            let urls = urls_of(registry);
            let mut records: Vec<serde_json::Value> = urls
                .iter()
                .map(|url| {
                    let key = registry
                        .get("root_keys")
                        .and_then(toml::Value::as_table)
                        .and_then(|keys| keys.get(url))
                        .and_then(toml::Value::as_str)
                        .unwrap_or_default();
                    json!({"type":"registry","url":url,"root_key":key})
                })
                .collect();
            if records.is_empty() {
                records.push(json!({"type":"progress","step":"registry","message":"No registries pinned"}));
            }
            offline::finish(records, 0)
        }
        "add" => {
            let Some(url) = rest.first() else {
                return offline::failure("usage.missing-argument", "registry add needs a URL", 2);
            };
            let Some(key) = root_key else {
                return offline::failure(
                    "registry.root-key-missing",
                    "registry add needs --root-key <key id>",
                    2,
                );
            };
            if url.starts_with("http://") {
                return offline::failure("registry.insecure-url", "Registries must use https", 1);
            }
            if !(url.starts_with("https://") || url.starts_with("file://")) {
                return offline::failure(
                    "usage.invalid-value",
                    format!("{url} is not an https:// or file:// URL"),
                    2,
                );
            }
            let mut document = load(env);
            let registry = section(&mut document);
            let mut urls = urls_of(registry);
            if !urls.contains(url) {
                urls.push(url.clone());
            }
            registry.insert(
                "urls".to_owned(),
                toml::Value::Array(urls.into_iter().map(toml::Value::String).collect()),
            );
            let keys = registry
                .entry("root_keys".to_owned())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if let Some(table) = keys.as_table_mut() {
                table.insert(url.clone(), toml::Value::String(key));
            }
            if let Err(output) = save(env, &document) {
                return output;
            }
            offline::finish(vec![json!({"type":"registry","url":url,"action":"added"})], 0)
        }
        "remove" => {
            let Some(url) = rest.first() else {
                return offline::failure("usage.missing-argument", "registry remove needs a URL", 2);
            };
            let mut document = load(env);
            let registry = section(&mut document);
            let mut urls = urls_of(registry);
            let before = urls.len();
            urls.retain(|existing| existing != url);
            if urls.len() == before {
                return offline::failure("registry.unknown", format!("{url} is not pinned"), 1);
            }
            registry.insert(
                "urls".to_owned(),
                toml::Value::Array(urls.into_iter().map(toml::Value::String).collect()),
            );
            if let Some(table) = registry.get_mut("root_keys").and_then(toml::Value::as_table_mut) {
                table.remove(url);
            }
            if let Err(output) = save(env, &document) {
                return output;
            }
            offline::finish(vec![json!({"type":"registry","url":url,"action":"removed"})], 0)
        }
        other => offline::failure(
            "usage.command-unknown",
            format!("Unknown registry command: {other}"),
            2,
        ),
    }
}
