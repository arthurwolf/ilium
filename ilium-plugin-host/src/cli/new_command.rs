//! `ilium plugin new`: writes a project from one of the eight built-in templates
//! (reference-project-templates.md, reference-plugin-cli.md DEV-040..045, DEV-260..278, DEV-935).
//! The command needs no network and no toolchain, and every check runs before the first write.

use super::{offline, CliEnv, CliOutput};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const TEMPLATES: [&str; 8] = [
    "server",
    "client",
    "both",
    "scene",
    "pane-kind",
    "core-style",
    "service-provider",
    "extension",
];
const SIDE_ALIASES: [&str; 4] = ["server", "client", "both", "scene"];
const SDK_VERSION: &str = "0.1.0";
const LICENSES: [&str; 10] = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "MPL-2.0",
    "GPL-3.0-only",
    "GPL-3.0-or-later",
    "LGPL-3.0-only",
    "Unlicense",
];

struct Options {
    dir: PathBuf,
    id: String,
    lang: String,
    template: String,
    name: String,
    publisher: String,
    first_party: bool,
    ci: String,
    force: bool,
    license: String,
}

struct Context {
    id: String,
    name: String,
    publisher: String,
    package: String,
    ident: String,
    template: String,
    lang: String,
    license: String,
    grants: BTreeMap<&'static str, Vec<String>>,
}

fn usage(code: &str, message: impl Into<String>) -> CliOutput {
    offline::failure(code, message, 2)
}

fn valid_id(id: &str) -> bool {
    let labels: Vec<&str> = id.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn capitalised(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn parse(args: &[String], env: &CliEnv) -> Result<Options, CliOutput> {
    let (mut dir, mut id, mut lang, mut template, mut side) = (None, None, None, None, None);
    let (mut name, mut publisher, mut ci, mut license) = (None, None, None, None);
    let (mut first_party, mut force) = (false, false);
    let mut iterator = args.iter();
    while let Some(argument) = iterator.next() {
        let mut value_of = |flag: &str| -> Result<String, CliOutput> {
            iterator.next().cloned().ok_or_else(|| {
                usage("usage.missing-argument", format!("{flag} needs a value"))
            })
        };
        match argument.as_str() {
            "--json" | "--quiet" | "--no-color" => {}
            "--id" => id = Some(value_of("--id")?),
            "--lang" => lang = Some(value_of("--lang")?),
            "--template" => template = Some(value_of("--template")?),
            "--side" => side = Some(value_of("--side")?),
            "--name" => name = Some(value_of("--name")?),
            "--publisher" => publisher = Some(value_of("--publisher")?),
            "--ci" => ci = Some(value_of("--ci")?),
            "--license" => license = Some(value_of("--license")?),
            "--first-party" => first_party = true,
            "--force" => force = true,
            flag if flag.starts_with('-') => {
                return Err(usage("usage.unknown-flag", format!("Unknown flag: {flag}")))
            }
            _ if dir.is_none() => dir = Some(argument.clone()),
            _ => {
                return Err(usage(
                    "usage.argument-unexpected",
                    "Only one project folder is accepted",
                ))
            }
        }
    }
    let Some(dir) = dir else {
        return Err(usage("usage.missing-argument", "A project folder is required"));
    };
    let Some(id) = id else {
        return Err(usage("usage.missing-argument", "--id is required"));
    };
    let Some(lang) = lang else {
        return Err(usage("usage.missing-argument", "--lang is required"));
    };
    if !matches!(lang.as_str(), "rust" | "typescript") {
        return Err(usage(
            "usage.invalid-value",
            "--lang must be rust or typescript",
        ));
    }
    if let Some(side) = &side {
        if !SIDE_ALIASES.contains(&side.as_str()) {
            return Err(usage(
                "usage.invalid-value",
                "--side accepts server, client, both and scene",
            ));
        }
    }
    let template = match (template, side) {
        (Some(template), Some(side)) if template != side => {
            return Err(usage(
                "usage.conflict",
                "--template and --side must name the same template",
            ))
        }
        (Some(template), _) => template,
        (None, Some(side)) => side,
        (None, None) => {
            return Err(usage(
                "usage.missing-argument",
                "--template or --side is required",
            ))
        }
    };
    let ci = ci.unwrap_or_else(|| "none".to_owned());
    if !matches!(ci.as_str(), "github" | "gitlab" | "none") {
        return Err(usage(
            "usage.invalid-value",
            "--ci must be github, gitlab or none",
        ));
    }
    let last_label = id.rsplit('.').next().unwrap_or_default();
    let publisher = publisher.unwrap_or_else(|| {
        env.env
            .get("USER")
            .cloned()
            .or_else(|| std::env::var("USER").ok())
            .filter(|user| !user.is_empty())
            .unwrap_or_else(|| "unknown".to_owned())
    });
    Ok(Options {
        dir: env.cwd.join(dir),
        name: name.unwrap_or_else(|| capitalised(last_label)),
        id,
        lang,
        template,
        publisher,
        first_party,
        ci,
        force,
        license: license.unwrap_or_else(|| "MIT".to_owned()),
    })
}

fn escape(text: &str) -> String {
    let quoted = Value::String(text.to_owned()).to_string();
    quoted[1..quoted.len() - 1].to_owned()
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Replaces the placeholders (DEV-264, DEV-265, DEV-275). `structured` files get JSON/TOML escaping.
fn fill(text: &str, context: &Context, structured: bool) -> String {
    let wrap = |value: &str| {
        if structured {
            escape(value)
        } else {
            plain(value)
        }
    };
    text.replace("@@package_ident@@", &context.ident)
        .replace("@@package@@", &wrap(&context.package))
        .replace("@@publisher@@", &wrap(&context.publisher))
        .replace("@@name@@", &wrap(&context.name))
        .replace("@@id@@", &wrap(&context.id))
        .replace("@@sdk@@", SDK_VERSION)
}

fn grants_of(template: &str, id: &str) -> BTreeMap<&'static str, Vec<String>> {
    let list = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
    let mut grants = BTreeMap::new();
    match template {
        "server" => {
            grants.insert("server", list(&["tree.read", "tree.decorate"]));
        }
        "client" | "both" => {
            grants.insert("client", list(&["ui.panels"]));
        }
        "pane-kind" => {
            grants.insert("client", list(&["ui.panels", "panes.provide-kind"]));
        }
        "core-style" => {
            grants.insert("server", list(&["tree.read"]));
            grants.insert("client", list(&["ui.panels"]));
        }
        "service-provider" => {
            grants.insert("server", vec![format!("services.provide:{id}/greeter")]);
        }
        "extension" => {
            grants.insert(
                "server",
                list(&["extensions.contribute:ilium.core.smart-copy/after-copy"]),
            );
        }
        _ => {}
    }
    grants
}

fn help_entry(name: &str) -> Value {
    json!([{ "id": "overview", "file": "help/overview.md", "title": name }])
}

fn manifest(context: &Context) -> Value {
    let server = |exports: Value| {
        json!({ "entry": "server.wasm", "world": "ilium:server/base@0.1.0", "exports": exports })
    };
    let client = json!({ "entry": "client.wasm", "world": "ilium:client/base@0.1.0", "exports": ["views"] });
    let mut manifest = json!({
        "api": "ilium:plugin@0.1.0",
        "id": context.id,
        "name": context.name,
        "version": "0.1.0",
        "publisher": context.publisher,
        "description": "",
        "license": context.license,
    });
    let mut contributes = json!({ "help": help_entry(&context.name) });
    let object = manifest.as_object_mut().expect("manifest is an object");
    let command = json!([{ "id": "open", "title": format!("Open {}", context.name), "icon": "layout-sidebar" }]);
    let panel = json!([{ "id": "main", "title": context.name, "icon": "layout-sidebar", "modal": false }]);
    match context.template.as_str() {
        "server" => {
            object.insert("description".into(), json!("Marks agents that start working."));
            object.insert("server".into(), server(json!([])));
            contributes["decoration_slots"] =
                json!([{ "id": "work", "placement": "badge", "priority": 50 }]);
        }
        "client" => {
            object.insert("description".into(), json!("Adds a panel with one view."));
            object.insert("client".into(), client);
            contributes["commands"] = command;
            contributes["panels"] = panel;
        }
        "both" => {
            object.insert(
                "description".into(),
                json!("A server that greets a client view."),
            );
            object.insert("server".into(), server(json!([])));
            object.insert("client".into(), client);
            contributes["commands"] = command;
            contributes["panels"] = panel;
        }
        "scene" => {
            object.insert("description".into(), json!("An ambient background scene."));
            object.insert(
                "scene".into(),
                json!({ "entry": "scene.wasm", "world": "ilium:anim/base@0.1.0", "exports": [] }),
            );
            contributes["scenes"] = json!([{
                "id": "waves",
                "title": "Waves",
                "family": "procedural",
                "modes": ["live"],
                "frame_rate_cap": 30,
                "settings_schema": { "type": "object", "properties": {} }
            }]);
        }
        "pane-kind" => {
            object.insert("description".into(), json!("Adds a new kind of pane."));
            object.insert("client".into(), client);
            contributes["pane_kinds"] = json!([{
                "id": "board", "title": context.name, "icon": "layout-board",
                "primitive": "basic", "wants_keys": true, "persist": "metadata"
            }]);
        }
        "core-style" => {
            object.insert("description".into(), json!("A replaceable first-party plugin."));
            object.insert("publisher".into(), json!("Ilium"));
            object.insert("limits_tier".into(), json!("first-party"));
            object.insert("replaceable".into(), json!(true));
            object.insert("surface".into(), json!([]));
            object.insert("server".into(), server(json!([])));
            object.insert("client".into(), client);
            contributes["extension_points"] = json!([{
                "id": "before-open", "kind": "hook", "mode": "veto", "title": "Before open",
                "deadline_ms": 20, "max_handlers": 8, "version": "0.1.0"
            }]);
        }
        "service-provider" => {
            object.insert("description".into(), json!("Provides a named greeting service."));
            object.insert("server".into(), server(json!(["service-provider"])));
            contributes["services"] = json!([{ "provides": [format!("{}/greeter", context.id)] }]);
        }
        _ => {
            object.insert("description".into(), json!("Fills a hook of a core plugin."));
            object.insert("server".into(), server(json!(["hook-handler"])));
            contributes["extensions"] = json!([{
                "point": "ilium.core.smart-copy/after-copy@0.1.0",
                "id": "strip-keys", "priority": 70, "handler": "strip_keys"
            }]);
        }
    }
    let mut capabilities = serde_json::Map::new();
    for (side, grants) in &context.grants {
        capabilities.insert((*side).to_owned(), json!(grants));
    }
    object.insert("capabilities".into(), Value::Object(capabilities));
    object.insert("contributes".into(), contributes);
    manifest
}

fn sides_of(template: &str) -> Vec<&'static str> {
    match template {
        "client" | "pane-kind" => vec!["client"],
        "both" | "core-style" => vec!["server", "client"],
        "scene" => vec!["scene"],
        _ => vec!["server"],
    }
}

fn readme(context: &Context) -> String {
    let mut text = format!(
        "<!-- written by ilium {} -->\n# @@name@@\n\nA plugin for Ilium. Describe what it does in one paragraph.\n\n## Build and test\n\n    ilium plugin check\n    ilium plugin lint --deny warnings\n    ilium plugin test\n    ilium plugin build\n    ilium plugin dev\n    ilium plugin pack\n\n## Grants\n\n",
        env!("CARGO_PKG_VERSION")
    );
    let mut any = false;
    for (side, grants) in &context.grants {
        for grant in grants {
            any = true;
            text.push_str(&format!("- `{grant}` ({side}): describe why the plugin needs it.\n"));
        }
    }
    if !any {
        text.push_str("This plugin requests no grants.\n");
    }
    fill(&text, context, false)
}

fn cargo_toml(side: Option<&str>, context: &Context, name: &str) -> String {
    let metadata = side
        .map(|side| format!("\n[package.metadata.ilium]\nside = \"{side}\"\n"))
        .unwrap_or_default();
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\nrust-version = \"1.90\"\n{metadata}\n[lib]\ncrate-type = [\"cdylib\", \"rlib\"]\n\n[dependencies]\nilium-plugin-sdk = \"@@sdk@@\"\n\n[dev-dependencies]\nilium-plugin-sdk = {{ version = \"@@sdk@@\", features = [\"testing\"] }}\n"
    )
    .replace("@@package@@", &escape(&context.package))
}

const SERVER_LIB: &str = r#"use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::intents::SetDecorationSpec;
use ilium_plugin_sdk::types::tree::AgentActivity;

#[ilium_plugin(side = "server")]
#[derive(Default)]
pub struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activate: {} ready", context.plugin));
        Ok(())
    }
}

impl Handler for Plugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            let Event::AgentActivityChanged(AgentActivityEvent { node, current, .. }) = event
            else {
                continue;
            };
            if current != AgentActivity::Working {
                continue;
            }
            submissions.push(Submission::new(Intent::SetDecoration(SetDecorationSpec {
                node,
                slot: "work".into(),
                content: vec![Span::plain("work")],
                icon: None,
                sort_key: None,
                persist: false,
            })));
        }
        submissions
    }
}
"#;

const SERVER_TEST: &str = r#"use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::testing::FakeHost;
use ilium_plugin_sdk::types::events::AgentActivityEvent;
use ilium_plugin_sdk::types::tree::AgentActivity;
use @@package_ident@@::Plugin;

#[test]
fn working_agent_gets_a_badge() {
    let mut host = FakeHost::for_manifest("manifest.json").expect("manifest");
    host.activate::<Plugin>();
    let submissions = host.deliver(vec![Event::AgentActivityChanged(AgentActivityEvent {
        node: 42,
        previous: AgentActivity::Idle,
        current: AgentActivity::Working,
        revision: 1,
    })]);
    assert_eq!(submissions.len(), 1);
}
"#;

const CLIENT_LIB: &str = r#"use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::ui::{Area, Tree, UiTree, ViewTarget};

#[ilium_plugin(side = "client")]
#[derive(Default)]
pub struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activate: {} ready", context.plugin));
        Ok(())
    }
}

impl Views for Plugin {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut builder = Tree::new();
        let line = builder.paragraph(["Hello from the plugin"]).key("hello");
        let root = builder.block().title("Example").child(line);
        builder.finish(root)
    }
}
"#;

const CLIENT_TEST: &str = r#"use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::ui::ViewTarget;
use @@package_ident@@::Plugin;

#[test]
fn overview_matches_the_golden_render() {
    let mut host = FakeHost::for_manifest("manifest.json").expect("manifest");
    host.activate::<Plugin>();
    let rendered = host.render_text::<Plugin>(ViewTarget::Panel("main".into()), 40, 4);
    assert_golden("tests/golden/overview.txt", &rendered);
}
"#;

const GOLDEN_OVERVIEW: &str = "╭  ≡ ● · Example ────────────────────────╮\n│ Hello from the plugin                  │\n│                                        │\n╰────────────────────────────────────────╯\n";

const SCENE_LIB: &str = r#"use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::scene::{FrameInput, FrameSurface, Scene};

#[ilium_plugin(side = "scene")]
#[derive(Default)]
pub struct Waves;

impl Scene for Waves {
    fn render(&mut self, frame: FrameInput, surface: &FrameSurface) {
        for row in 0..surface.height() {
            for column in 0..surface.width() {
                let phase = (column + row + frame.index as u32) % 10;
                surface.write_tone(column, row, phase as u8);
            }
        }
    }
}
"#;

const SCENE_TEST: &str = r#"use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use @@package_ident@@::Waves;

#[test]
fn frame_three_matches_the_golden_tones() {
    let mut host = FakeHost::for_manifest("manifest.json").expect("manifest");
    let tones = host.render_scene_tones::<Waves>(16, 4, 3);
    assert_golden("tests/golden/frame-tones.txt", &tones);
}
"#;

const SCENE_GOLDEN: &str = "3456789012345678\n4567890123456789\n5678901234567890\n6789012345678901\n";

const PANE_LIB: &str = r#"use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::ui::{Area, Tree, UiTree, ViewTarget};

#[ilium_plugin(side = "client")]
#[derive(Default)]
pub struct Plugin {
    presses: u32,
}

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activate: {} ready", context.plugin));
        Ok(())
    }
}

impl Handler for Plugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        for event in batch {
            if let Event::KeyPressed(_) = event {
                self.presses += 1;
            }
        }
        Vec::new()
    }
}

impl Views for Plugin {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut builder = Tree::new();
        let line = builder.paragraph([format!("Keys pressed: {}", self.presses)]).key("keys");
        let root = builder.block().title("Board").child(line);
        builder.finish(root)
    }
}
"#;

const PANE_TEST: &str = r#"use ilium_plugin_sdk::testing::{assert_golden, FakeHost};
use ilium_plugin_sdk::ui::ViewTarget;
use @@package_ident@@::Plugin;

#[test]
fn the_pane_renders_and_counts_keys() {
    let mut host = FakeHost::for_manifest("manifest.json").expect("manifest");
    host.activate::<Plugin>();
    let rendered = host.render_text::<Plugin>(ViewTarget::Pane(1), 40, 6);
    assert_golden("tests/golden/overview.txt", &rendered);
}
"#;

const SERVICE_LIB: &str = r##"use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
pub struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activate: {} ready", context.plugin));
        Ok(())
    }
}

impl ServiceProvider for Plugin {
    fn handle(&mut self, _call: ServiceCall) -> ServiceReply {
        ServiceReply::json(r#"{"greeting":"hello"}"#)
    }
}
"##;

const SERVICE_TEST: &str = r##"use ilium_plugin_sdk::testing::FakeSession;
use @@package_ident@@::Plugin;

#[test]
fn the_greeter_replies_with_a_greeting() {
    let mut session = FakeSession::for_manifest("manifest.json").expect("manifest");
    session.start_server::<Plugin>();
    let reply = session.call_service("@@id@@/greeter", "{}");
    assert_eq!(reply, r#"{"greeting":"hello"}"#);
}
"##;

const EXTENSION_LIB: &str = r#"use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
pub struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activate: {} ready", context.plugin));
        Ok(())
    }
}

impl HookHandler for Plugin {
    fn strip_keys(&mut self, call: HookCall) -> HookReply {
        let cleaned = call.payload.replace("sk-secret", "");
        HookReply::Replace(cleaned)
    }
}
"#;

const EXTENSION_TEST: &str = r##"use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::testing::FakeHost;
use @@package_ident@@::Plugin;

#[test]
fn the_secret_is_stripped_from_the_payload() {
    let mut host = FakeHost::for_manifest("manifest.json").expect("manifest");
    host.activate::<Plugin>();
    let reply = host.run_hook("strip-keys", HookCall::new(r#"{"text":"sk-secret"}"#));
    assert!(matches!(reply, HookReply::Replace(_)));
}
"##;

const CORE_LIB_SERVER: &str = r#"use ilium_plugin_sdk::host::log::{self, Level};
use ilium_plugin_sdk::prelude::*;

#[ilium_plugin(side = "server")]
#[derive(Default)]
pub struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, context: Activation) -> Result<(), PluginError> {
        log::log(Level::Info, &format!("activate: {} ready", context.plugin));
        Ok(())
    }
}
"#;

const BOTH_SHARED_LIB: &str = r#"/// The message the server sends to the client view, encoded by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub text: String,
}

impl Hello {
    pub fn encode(&self) -> Vec<u8> {
        self.text.as_bytes().to_vec()
    }

    pub fn decode(bytes: &[u8]) -> Option<Hello> {
        String::from_utf8(bytes.to_vec()).ok().map(|text| Hello { text })
    }
}

#[cfg(test)]
mod tests {
    use super::Hello;

    #[test]
    fn a_message_survives_a_round_trip() {
        let hello = Hello { text: "hello".into() };
        assert_eq!(Hello::decode(&hello.encode()), Some(hello));
    }
}
"#;

const BOTH_SERVER_LIB: &str = r#"use ilium_plugin_sdk::prelude::*;
use shared::Hello;

#[ilium_plugin(side = "server")]
#[derive(Default)]
pub struct Plugin;

impl Lifecycle for Plugin {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for Plugin {
    fn on_events(&mut self, batch: Vec<Event>) -> Vec<Submission> {
        let mut submissions = Vec::new();
        for event in batch {
            if let Event::ClientAttached(_) = event {
                let hello = Hello { text: "hello from the server".into() };
                submissions.push(Submission::new(Intent::message("hello", hello.encode())));
            }
        }
        submissions
    }
}
"#;

const BOTH_CLIENT_LIB: &str = r#"use ilium_plugin_sdk::prelude::*;
use ilium_plugin_sdk::ui::{Area, Tree, UiTree, ViewTarget};
use shared::Hello;

#[ilium_plugin(side = "client")]
#[derive(Default)]
pub struct Plugin {
    text: String,
}

impl Lifecycle for Plugin {
    fn activate(&mut self, _context: Activation) -> Result<(), PluginError> {
        Ok(())
    }
}

impl Handler for Plugin {
    fn on_message(&mut self, channel: &str, bytes: &[u8]) {
        if channel == "hello" {
            if let Some(hello) = Hello::decode(bytes) {
                self.text = hello.text;
            }
        }
    }
}

impl Views for Plugin {
    fn render(&mut self, _view: ViewTarget, _area: Area) -> UiTree {
        let mut builder = Tree::new();
        let line = builder.paragraph([self.text.as_str()]).key("hello");
        let root = builder.block().title("Hello").child(line);
        builder.finish(root)
    }
}
"#;

const BOTH_TEST: &str = r#"use ilium_plugin_sdk::testing::FakeSession;

#[test]
fn the_client_view_shows_the_message_after_client_attached() {
    let mut session = FakeSession::for_manifest("manifest.json").expect("manifest");
    session.start_server::<server::Plugin>();
    session.attach_client::<client::Plugin>();
    session.deliver_to_server("client-attached");
    assert!(session.render_client_text(40, 4).contains("hello from the server"));
}
"#;

const SAMPLE_FIXTURE: &str = r#"{
  "schema": 1,
  "events": [
    { "tag": "agent-activity-changed", "val": { "node": 42, "previous": "idle", "current": "working", "revision": 1 } }
  ]
}
"#;

const SAMPLE_EXPECT: &str = r#"{
  "schema": 1,
  "submissions": { "match": "ordered", "items": [ { "intent": "set-decoration", "node": 42, "slot": "work" } ] }
}
"#;

const SAMPLE_EXPECT_EMPTY: &str = r#"{
  "schema": 1,
  "submissions": { "match": "ordered", "items": [] }
}
"#;

const SAMPLE_FIXTURE_EMPTY: &str = "{\n  \"schema\": 1,\n  \"events\": []\n}\n";

const TS_SERVER: &str = r#"import { definePlugin, type Event, type Submission } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { span } from '@ilium/plugin-sdk/types';

let nextCorrelation = 1n;

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate(context) {
    log('info', `activate: ${context.plugin} ready`);
  },
  onEvents(batch: Event[]): Submission[] {
    const submissions: Submission[] = [];
    for (const event of batch) {
      if (event.tag !== 'agent-activity-changed') continue;
      if (event.val.current !== 'working') continue;
      submissions.push({
        intent: {
          tag: 'set-decoration',
          val: {
            node: event.val.node,
            slot: 'work',
            content: [span('work')],
            icon: undefined,
            sortKey: undefined,
            persist: false,
          },
        },
        correlation: nextCorrelation++,
        dedupKey: undefined,
      });
    }
    return submissions;
  },
});
"#;

const TS_SERVER_TEST: &str = r#"import { expect, test } from 'bun:test';
import { createFakeHost } from '@ilium/plugin-sdk/testing';

test('working agent gets a badge', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/server');
  const submissions = host.deliver([
    { tag: 'agent-activity-changed', val: { node: 42n, previous: 'idle', current: 'working', revision: 1n } },
  ]);
  expect(submissions.length).toBe(1);
});
"#;

const TS_CLIENT: &str = r#"import { definePlugin } from '@ilium/plugin-sdk';
import { log } from '@ilium/plugin-sdk/host/log';
import { Tree } from '@ilium/plugin-sdk/ui';

export const { lifecycle, views } = definePlugin({
  side: 'client',
  activate(context) {
    log('info', `activate: ${context.plugin} ready`);
  },
  render() {
    const builder = new Tree();
    const line = builder.paragraph(['Hello from the plugin']).key('hello');
    return builder.finish(builder.block().title('Example').child(line));
  },
});
"#;

const TS_CLIENT_TEST: &str = r#"import { expect, test } from 'bun:test';
import { assertGolden, createFakeHost } from '@ilium/plugin-sdk/testing';

test('the overview matches the golden render', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/client');
  const rendered = host.renderText({ panel: 'main' }, 40, 4);
  assertGolden('tests/golden/overview.txt', rendered);
  expect(rendered.length).toBeGreaterThan(0);
});
"#;

const TS_SCENE: &str = r#"import { definePlugin } from '@ilium/plugin-sdk';

export const { scene } = definePlugin({
  side: 'scene',
  render(frame, surface) {
    for (let row = 0; row < surface.height; row++) {
      for (let column = 0; column < surface.width; column++) {
        surface.writeTone(column, row, (column + row + Number(frame.index)) % 10);
      }
    }
  },
});
"#;

const TS_SCENE_TEST: &str = r#"import { expect, test } from 'bun:test';
import { assertGolden, createFakeHost } from '@ilium/plugin-sdk/testing';

test('frame three matches the golden tones', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/scene');
  const tones = host.renderSceneTones(16, 4, 3);
  assertGolden('tests/golden/frame-tones.txt', tones);
  expect(tones.length).toBeGreaterThan(0);
});
"#;

const TS_SHARED: &str = r#"export interface Hello {
  text: string;
}

export function encodeHello(hello: Hello): Uint8Array {
  return new TextEncoder().encode(hello.text);
}

export function decodeHello(bytes: Uint8Array): Hello {
  return { text: new TextDecoder().decode(bytes) };
}
"#;

const TS_BOTH_SERVER: &str = r#"import { definePlugin } from '@ilium/plugin-sdk';
import { encodeHello } from './shared';

export const { lifecycle, handler } = definePlugin({
  side: 'server',
  activate() {},
  onEvents(batch) {
    return batch
      .filter((event) => event.tag === 'client-attached')
      .map((_, index) => ({
        intent: { tag: 'message', val: { channel: 'hello', bytes: encodeHello({ text: 'hello from the server' }) } },
        correlation: BigInt(index + 1),
        dedupKey: undefined,
      }));
  },
});
"#;

const TS_BOTH_CLIENT: &str = r#"import { definePlugin } from '@ilium/plugin-sdk';
import { Tree } from '@ilium/plugin-sdk/ui';
import { decodeHello } from './shared';

let text = '';

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onMessage(channel: string, bytes: Uint8Array) {
    if (channel === 'hello') text = decodeHello(bytes).text;
  },
  render() {
    const builder = new Tree();
    const line = builder.paragraph([text]).key('hello');
    return builder.finish(builder.block().title('Hello').child(line));
  },
});
"#;

const TS_BOTH_TEST: &str = r#"import { expect, test } from 'bun:test';
import { createFakeSession } from '@ilium/plugin-sdk/testing';

test('the client view shows the message after client-attached', async () => {
  const session = createFakeSession().forManifest('manifest.json');
  await session.startServer('../src/server');
  await session.attachClient('../src/client');
  session.deliverToServer('client-attached');
  expect(session.renderClientText(40, 4)).toContain('hello from the server');
});
"#;

const TS_SERVICE: &str = r#"import { definePlugin } from '@ilium/plugin-sdk';

export const { lifecycle, serviceProvider } = definePlugin({
  side: 'server',
  activate() {},
  handle() {
    return { json: '{"greeting":"hello"}' };
  },
});
"#;

const TS_SERVICE_TEST: &str = r#"import { expect, test } from 'bun:test';
import { createFakeSession } from '@ilium/plugin-sdk/testing';

test('the greeter replies with a greeting', async () => {
  const session = createFakeSession().forManifest('manifest.json');
  await session.startServer('../src/server');
  expect(session.callService('@@id@@/greeter', '{}')).toBe('{"greeting":"hello"}');
});
"#;

const TS_EXTENSION: &str = r#"import { definePlugin } from '@ilium/plugin-sdk';

export const { lifecycle, hookHandler } = definePlugin({
  side: 'server',
  activate() {},
  strip_keys(call: { payload: string }) {
    return { tag: 'replace', val: call.payload.replace('sk-secret', '') };
  },
});
"#;

const TS_EXTENSION_TEST: &str = r#"import { expect, test } from 'bun:test';
import { createFakeHost } from '@ilium/plugin-sdk/testing';

test('the secret is stripped from the payload', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/server');
  const reply = host.runHook('strip-keys', { payload: '{"text":"sk-secret"}' });
  expect(reply.tag).toBe('replace');
});
"#;

const TS_PANE: &str = r#"import { definePlugin } from '@ilium/plugin-sdk';
import { Tree } from '@ilium/plugin-sdk/ui';

let presses = 0;

export const { lifecycle, handler, views } = definePlugin({
  side: 'client',
  activate() {},
  onEvents(batch) {
    for (const event of batch) if (event.tag === 'key-pressed') presses++;
    return [];
  },
  render() {
    const builder = new Tree();
    const line = builder.paragraph([`Keys pressed: ${presses}`]).key('keys');
    return builder.finish(builder.block().title('Board').child(line));
  },
});
"#;

const TS_PANE_TEST: &str = r#"import { expect, test } from 'bun:test';
import { assertGolden, createFakeHost } from '@ilium/plugin-sdk/testing';

test('the pane renders and counts keys', async () => {
  const host = createFakeHost().forManifest('manifest.json');
  await host.activateModule('../src/client');
  const rendered = host.renderText({ pane: 1 }, 40, 6);
  assertGolden('tests/golden/overview.txt', rendered);
  expect(rendered.length).toBeGreaterThan(0);
});
"#;

const PANE_GOLDEN: &str = "╭ Board ─────────────────────────────────╮\n│ Keys pressed: 0                        │\n│                                        │\n│                                        │\n│                                        │\n╰────────────────────────────────────────╯\n";

const GITIGNORE: &str = "dist/\n.ilium/\ntarget/\nnode_modules/\npublisher.key\n*.tmp\n";

const CHANGELOG: &str = "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- First version.\n";

const HELP: &str = "# @@name@@\n\nOne sentence that says what the plugin adds.\n\nName each command, setting and view the plugin adds, one line each.\n";

const GITHUB_CI: &str = "name: plugin\non: [push, pull_request]\njobs:\n  plugin:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: ilium plugin check\n      - run: ilium plugin lint --deny warnings\n      - run: ilium plugin build\n      - run: ilium plugin test\n";

const GITLAB_CI: &str = "plugin:\n  script:\n    - ilium plugin check\n    - ilium plugin lint --deny warnings\n    - ilium plugin build\n    - ilium plugin test\n";

const TS_TSCONFIG: &str = "{\n  \"compilerOptions\": {\n    \"target\": \"ES2022\",\n    \"module\": \"ESNext\",\n    \"moduleResolution\": \"bundler\",\n    \"strict\": true,\n    \"noEmit\": true,\n    \"types\": [\"bun-types\"]\n  },\n  \"include\": [\"src\", \"tests\"]\n}\n";

const TS_BUNFIG: &str = "[test]\npreload = [\"@ilium/plugin-sdk/testing/setup\"]\n";

const TS_PACKAGE: &str = "{\n  \"name\": \"@@package@@\",\n  \"version\": \"0.1.0\",\n  \"private\": true,\n  \"type\": \"module\",\n  \"dependencies\": { \"@ilium/plugin-sdk\": \"@@sdk@@\" },\n  \"scripts\": { \"test\": \"bun test\" }\n}\n";

fn extending(context: &Context) -> String {
    format!(
        "# Extending {}\n\nPlugins extend this plugin through its extension points.\n\n| Point | Kind | Mode | Deadline | Version |\n|---|---|---|---|---|\n| `before-open` | hook | veto | 20 ms | 0.1.0 |\n",
        plain(&context.name)
    )
}

fn license_text(license: &str, publisher: &str) -> String {
    if license == "MIT" {
        format!(
            "MIT License\n\nCopyright (c) {}\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the \"Software\"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software.\n\nThe above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND.\n",
            plain(publisher)
        )
    } else {
        format!(
            "{license}\n\nCopyright (c) {}\n\nSee https://spdx.org/licenses/{license}.html for the full licence text.\n",
            plain(publisher)
        )
    }
}

/// Every file the template writes: relative path and final content, sorted by path.
fn project_files(context: &Context, options: &Options) -> BTreeMap<String, String> {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let rust = context.lang == "rust";
    let template = context.template.as_str();
    let mut put = |path: &str, text: String| {
        files.insert(path.to_owned(), text);
    };
    put(
        "manifest.json",
        format!(
            "{}\n",
            serde_json::to_string_pretty(&manifest(context)).expect("manifest serializes")
        ),
    );
    put(".gitignore", GITIGNORE.to_owned());
    put("README.md", readme(context));
    put("CHANGELOG.md", CHANGELOG.to_owned());
    put("help/overview.md", fill(HELP, context, false));
    put("LICENSE", license_text(&context.license, &context.publisher));
    let sides = sides_of(template);
    let has_view = matches!(template, "client" | "pane-kind");
    if sides.contains(&"server") && !matches!(template, "both" | "core-style") {
        put("tests/fixtures/sample.json", SAMPLE_FIXTURE.to_owned());
        put("tests/fixtures/sample.expect.json", SAMPLE_EXPECT.to_owned());
    } else {
        put("tests/fixtures/sample.json", SAMPLE_FIXTURE_EMPTY.to_owned());
        put(
            "tests/fixtures/sample.expect.json",
            SAMPLE_EXPECT_EMPTY.to_owned(),
        );
    }
    if has_view {
        let golden = if template == "client" { GOLDEN_OVERVIEW } else { PANE_GOLDEN };
        put("tests/golden/overview.txt", golden.to_owned());
    }
    if template == "scene" {
        put("tests/golden/frame-tones.txt", SCENE_GOLDEN.to_owned());
    }
    if template == "core-style" {
        put("docs/EXTENDING.md", extending(context));
    }
    match options.ci.as_str() {
        "github" => put(".github/workflows/plugin.yml", GITHUB_CI.to_owned()),
        "gitlab" => put(".gitlab-ci.yml", GITLAB_CI.to_owned()),
        _ => {}
    }
    let two_sided = matches!(template, "both" | "core-style");
    if rust {
        if two_sided {
            put(
                "Cargo.toml",
                "[workspace]\nmembers = [\"shared\", \"server\", \"client\"]\nresolver = \"2\"\n".to_owned(),
            );
            put(
                "shared/Cargo.toml",
                "[package]\nname = \"shared\"\nversion = \"0.1.0\"\nedition = \"2021\"\nrust-version = \"1.90\"\n".to_owned(),
            );
            put(
                "shared/src/lib.rs",
                if template == "both" {
                    BOTH_SHARED_LIB.to_owned()
                } else {
                    "//! Types shared by the server and client sides.\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn the_crate_builds() {}\n}\n".to_owned()
                },
            );
            for side in ["server", "client"] {
                let toml = cargo_toml(Some(side), context, &format!("{}-{side}", context.package))
                    .replace(
                        "[dependencies]\n",
                        "[dependencies]\nshared = { path = \"../shared\" }\n",
                    );
                put(&format!("{side}/Cargo.toml"), toml);
            }
            let (server_lib, client_lib) = if template == "both" {
                (BOTH_SERVER_LIB, BOTH_CLIENT_LIB)
            } else {
                (CORE_LIB_SERVER, CLIENT_LIB)
            };
            put("server/src/lib.rs", server_lib.to_owned());
            put("client/src/lib.rs", client_lib.to_owned());
            put(
                "tests/session.rs",
                if template == "both" {
                    BOTH_TEST.to_owned()
                } else {
                    "use ilium_plugin_sdk::testing::FakeSession;\n\n#[test]\nfn before_open_runs_against_two_stub_contributors() {\n    let mut session = FakeSession::for_manifest(\"manifest.json\").expect(\"manifest\");\n    let verdicts = session.run_hook_with_stubs(\"before-open\", 2);\n    assert_eq!(verdicts.len(), 2);\n}\n".to_owned()
                },
            );
        } else {
            let side = sides[0];
            put("Cargo.toml", cargo_toml(Some(side), context, &context.package));
            let (lib, test_path, test) = match template {
                "client" => (CLIENT_LIB, "tests/view.rs", CLIENT_TEST),
                "scene" => (SCENE_LIB, "tests/frame.rs", SCENE_TEST),
                "pane-kind" => (PANE_LIB, "tests/view.rs", PANE_TEST),
                "service-provider" => (SERVICE_LIB, "tests/service.rs", SERVICE_TEST),
                "extension" => (EXTENSION_LIB, "tests/hook.rs", EXTENSION_TEST),
                _ => (SERVER_LIB, "tests/handler.rs", SERVER_TEST),
            };
            put("src/lib.rs", lib.to_owned());
            put(test_path, test.to_owned());
        }
    } else {
        put("package.json", fill(TS_PACKAGE, context, true));
        put("tsconfig.json", TS_TSCONFIG.to_owned());
        put("bunfig.toml", TS_BUNFIG.to_owned());
        match template {
            "client" => {
                put("src/client.ts", TS_CLIENT.to_owned());
                put("tests/view.test.ts", TS_CLIENT_TEST.to_owned());
            }
            "scene" => {
                put("src/scene.ts", TS_SCENE.to_owned());
                put("tests/frame.test.ts", TS_SCENE_TEST.to_owned());
            }
            "pane-kind" => {
                put("src/client.ts", TS_PANE.to_owned());
                put("tests/view.test.ts", TS_PANE_TEST.to_owned());
            }
            "service-provider" => {
                put("src/server.ts", TS_SERVICE.to_owned());
                put("tests/service.test.ts", fill(TS_SERVICE_TEST, context, true));
            }
            "extension" => {
                put("src/server.ts", TS_EXTENSION.to_owned());
                put("tests/hook.test.ts", TS_EXTENSION_TEST.to_owned());
            }
            "both" => {
                put("src/shared.ts", TS_SHARED.to_owned());
                put("src/server.ts", TS_BOTH_SERVER.to_owned());
                put("src/client.ts", TS_BOTH_CLIENT.to_owned());
                put("tests/session.test.ts", TS_BOTH_TEST.to_owned());
            }
            "core-style" => {
                put("src/shared.ts", "export const PLUGIN_NAME = 'core';\n".to_owned());
                put(
                    "src/server.ts",
                    "import { definePlugin } from '@ilium/plugin-sdk';\n\nexport const { lifecycle } = definePlugin({ side: 'server', activate() {} });\n".to_owned(),
                );
                put("src/client.ts", TS_CLIENT.to_owned());
                put(
                    "tests/session.test.ts",
                    "import { expect, test } from 'bun:test';\nimport { createFakeSession } from '@ilium/plugin-sdk/testing';\n\ntest('before-open runs against two stub contributors', async () => {\n  const session = createFakeSession().forManifest('manifest.json');\n  expect(session.runHookWithStubs('before-open', 2).length).toBe(2);\n});\n".to_owned(),
                );
            }
            _ => {
                put("src/server.ts", TS_SERVER.to_owned());
                put("tests/handler.test.ts", TS_SERVER_TEST.to_owned());
            }
        }
    }
    if rust && !two_sided && template != "client" && template != "scene" {
        // Single-crate templates keep their fixtures; nothing else to add.
    }
    // Placeholders in the remaining text files.
    let structured = |path: &str| path.ends_with(".json") || path.ends_with(".toml");
    let keys: Vec<String> = files.keys().cloned().collect();
    for key in keys {
        if matches!(key.as_str(), "manifest.json" | "README.md" | "package.json") {
            continue;
        }
        let text = files.get(&key).cloned().unwrap_or_default();
        files.insert(key.clone(), fill(&text, context, structured(&key)));
    }
    let manifest_text = files.get("manifest.json").cloned().unwrap_or_default();
    files.insert("manifest.json".to_owned(), manifest_text);
    files
}

fn template_record(files: &BTreeMap<String, String>, context: &Context) -> String {
    let hashes: BTreeMap<&String, String> = files
        .iter()
        .map(|(path, text)| {
            (
                path,
                format!("sha256:{}", hex(&Sha256::digest(text.as_bytes()))),
            )
        })
        .collect();
    let record = json!({
        "schema": 1,
        "origin": "builtin",
        "location": context.template,
        "name": context.template,
        "version": env!("CARGO_PKG_VERSION"),
        "lang": context.lang,
        "files": hashes,
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&record).expect("record serializes")
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp-new");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path)
}

pub fn run(args: &[String], env: &CliEnv) -> CliOutput {
    let options = match parse(args, env) {
        Ok(options) => options,
        Err(output) => return output,
    };
    if !valid_id(&options.id) {
        return offline::failure(
            "new.id-invalid",
            format!("The id {:?} does not match [a-z0-9-]+(.[a-z0-9-]+)+", options.id),
            1,
        );
    }
    let is_ilium = options.id.starts_with("ilium.");
    let core_style = options.template == "core-style";
    if (core_style && !(is_ilium && options.first_party)) || (!core_style && is_ilium) {
        return offline::failure(
            "new.id-reserved",
            "Ids that start with ilium. are reserved for first-party core-style plugins",
            1,
        );
    }
    if !TEMPLATES.contains(&options.template.as_str()) {
        return offline::failure(
            "new.template-unknown",
            format!("Unknown template: {}", options.template),
            1,
        );
    }
    if !LICENSES.contains(&options.license.as_str()) {
        return offline::failure(
            "new.license-unknown",
            format!("Unknown SPDX license id: {}", options.license),
            1,
        );
    }
    if !options.force && std::fs::read_dir(&options.dir).is_ok_and(|mut entries| entries.next().is_some()) {
        return offline::failure(
            "new.dir-not-empty",
            format!("{} is not empty", options.dir.display()),
            1,
        );
    }
    let package = options.id.rsplit('.').next().unwrap_or_default().to_owned();
    let context = Context {
        ident: package.replace('-', "_"),
        package,
        name: options.name.clone(),
        publisher: options.publisher.clone(),
        template: options.template.clone(),
        lang: options.lang.clone(),
        license: options.license.clone(),
        grants: grants_of(&options.template, &options.id),
        id: options.id.clone(),
    };
    let mut files = project_files(&context, &options);
    let record = template_record(&files, &context);
    files.insert(".ilium-template.json".to_owned(), record);
    let mut records = Vec::new();
    for (path, text) in &files {
        if let Err(error) = write_atomic(&options.dir.join(path), text) {
            records.push(json!({"type":"error","code":"new.write-failed","message":format!("{path}: {error}")}));
            return offline::finish(records, 1);
        }
        records.push(json!({"type":"progress","step":"inventory","message":path}));
    }
    offline::finish(records, 0)
}
