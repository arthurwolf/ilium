//! Setup policy only: canonical manifest parsing, grant declarations and deterministic defaults.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use ilium_plugin_api::grants::Grant;
use ilium_plugin_api::manifest::{parse_manifest, Manifest};
use ilium_plugin_api::platform::Platform;
use ilium_plugin_api::side::Side;
use ilium_plugin_api::tier::Tier;
use ilium_plugin_sdk_core::types::{Activation, PluginId};

use super::Mode;
use crate::errors::{codes, HarnessError};

#[derive(Debug)]
struct DeclaredGrants {
    server: Option<BTreeSet<String>>,
    client: Option<BTreeSet<String>>,
    scene: Option<BTreeSet<String>>,
}

impl DeclaredGrants {
    fn from_manifest(manifest: &Manifest) -> Self {
        let names = |grants: &[Grant]| -> BTreeSet<String> {
            grants
                .iter()
                .map(|grant| grant.as_str().to_owned())
                .collect()
        };
        Self {
            server: manifest
                .server
                .as_ref()
                .map(|_| names(&manifest.capabilities.server)),
            client: manifest
                .client
                .as_ref()
                .map(|_| names(&manifest.capabilities.client)),
            scene: manifest
                .scene
                .as_ref()
                .map(|_| names(&manifest.capabilities.scene)),
        }
    }

    fn get(&self, side: Side) -> Option<&BTreeSet<String>> {
        match side {
            Side::Server => self.server.as_ref(),
            Side::Client => self.client.as_ref(),
            Side::Scene => self.scene.as_ref(),
        }
    }
}

pub(super) struct Setup {
    pub(super) plugin: String,
    pub(super) side: Side,
    pub(super) granted: BTreeSet<String>,
    tier: Tier,
    platform: Platform,
    mode: Mode,
    seed: u64,
    declared: Option<DeclaredGrants>,
    closed: bool,
    seed_error: Option<HarnessError>,
    mode_error: Option<HarnessError>,
}

impl fmt::Debug for Setup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Setup")
            .field("plugin", &self.plugin)
            .field("side", &self.side)
            .field("granted", &self.granted)
            .field("tier", &self.tier)
            .field("platform", &self.platform)
            .field("mode", &self.mode)
            .field("seed", &self.seed)
            .field("closed", &self.closed)
            .field("seed_error", &self.seed_error)
            .field("mode_error", &self.mode_error)
            .finish_non_exhaustive()
    }
}

fn invalid(message: impl Into<String>) -> HarnessError {
    HarnessError::new(codes::CONFIG_INVALID, message)
}

fn environment(name: &str) -> Result<Option<String>, HarnessError> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(invalid(format!("{name} must be UTF-8"))),
    }
}

impl Setup {
    pub(super) fn new() -> Self {
        let seed = environment("ILIUM_PLUGIN_TEST_SEED").and_then(|value| match value {
            None => Ok(1),
            Some(value) => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(invalid(
                        "ILIUM_PLUGIN_TEST_SEED must be unsigned decimal u64 text",
                    ));
                }
                value
                    .parse::<u64>()
                    .map_err(|_| invalid("ILIUM_PLUGIN_TEST_SEED exceeds u64"))
            }
        });
        let mode = environment("ILIUM_PLUGIN_TEST_MODE").and_then(|value| match value.as_deref() {
            None | Some("native") => Ok(Mode::Native),
            Some("component") => Ok(Mode::Component),
            Some(_) => Err(invalid(
                "ILIUM_PLUGIN_TEST_MODE must be native or component",
            )),
        });
        let (seed, seed_error) = match seed {
            Ok(value) => (value, None),
            Err(error) => (1, Some(error)),
        };
        let (mode, mode_error) = match mode {
            Ok(value) => (value, None),
            Err(error) => (Mode::Native, Some(error)),
        };
        let platform: Platform = std::env::consts::OS
            .parse()
            .unwrap_or_else(|error| panic!("{}", invalid(format!("host platform: {error}"))));
        Self {
            plugin: "test.plugin".to_owned(),
            side: Side::Server,
            granted: BTreeSet::new(),
            tier: Tier::T1,
            platform,
            mode,
            seed,
            declared: None,
            closed: false,
            seed_error,
            mode_error,
        }
    }

    pub(super) fn for_manifest(path: &Path) -> Result<Self, HarnessError> {
        let bytes = std::fs::read(path).map_err(|error| {
            HarnessError::new(codes::MANIFEST_READ, format!("{}: {error}", path.display()))
        })?;
        let text = String::from_utf8(bytes).map_err(|error| {
            HarnessError::new(
                "manifest.json-invalid",
                format!("{}: {error}", path.display()),
            )
        })?;
        let manifest = parse_manifest(&text).map_err(|errors| {
            let code = errors
                .first()
                .map_or("manifest.schema", |error| error.code.as_str());
            let details = errors
                .iter()
                .map(|error| format!("{} {}: {}", error.code, error.pointer, error.message))
                .collect::<Vec<_>>()
                .join("\n");
            HarnessError::new(code, format!("{}: {details}", path.display()))
        })?;
        let declared = DeclaredGrants::from_manifest(&manifest);
        let side = [Side::Server, Side::Client, Side::Scene]
            .into_iter()
            .find(|side| declared.get(*side).is_some())
            .ok_or_else(|| invalid("manifest declares no component side"))?;
        let mut setup = Self::new();
        setup.plugin = manifest.id;
        setup.side = side;
        setup.granted = declared.get(side).cloned().unwrap_or_default();
        setup.declared = Some(declared);
        // limits_tier is a resource class, not package trust. Keep the documented T1 default.
        Ok(setup)
    }

    pub(super) fn ensure_open(&self) {
        if self.closed {
            panic!(
                "{}",
                HarnessError::new(codes::SETUP_CLOSED, "setup ended at build or activation")
            );
        }
    }

    pub(super) fn close(&mut self) {
        self.closed = true;
        if let Some(error) = self.mode_error.as_ref().or(self.seed_error.as_ref()) {
            panic!("{error}");
        }
    }

    pub(super) fn mode(&self) -> Mode {
        if let Some(error) = &self.mode_error {
            panic!("{error}");
        }
        self.mode
    }

    pub(super) fn seed(&self) -> u64 {
        if let Some(error) = &self.seed_error {
            panic!("{error}");
        }
        self.seed
    }

    pub(super) fn set_mode(&mut self, mode: Mode) {
        self.ensure_open();
        self.mode = mode;
        self.mode_error = None;
    }

    pub(super) fn set_seed(&mut self, seed: u64) {
        self.ensure_open();
        self.seed = seed;
        self.seed_error = None;
    }

    pub(super) fn set_tier(&mut self, tier: &str) {
        self.ensure_open();
        self.tier = tier
            .parse()
            .unwrap_or_else(|error| panic!("{}", invalid(format!("tier: {error}"))));
    }

    pub(super) fn set_platform(&mut self, platform: &str) {
        self.ensure_open();
        self.platform = platform
            .parse()
            .unwrap_or_else(|error| panic!("{}", invalid(format!("platform: {error}"))));
    }

    pub(super) fn set_side(&mut self, side: &str) {
        self.ensure_open();
        let side: Side = side
            .parse()
            .unwrap_or_else(|error| panic!("{}", invalid(format!("side: {error}"))));
        if let Some(declared) = &self.declared {
            let names = declared.get(side).unwrap_or_else(|| {
                panic!("{}", invalid(format!("manifest has no {side} component")))
            });
            if side != self.side {
                self.granted = names.clone();
            }
        }
        self.side = side;
    }

    pub(super) fn validate_grants(&self, names: &[&str]) -> Result<BTreeSet<String>, HarnessError> {
        // Parse the entire list before checking declarations or changing any state.
        let parsed = names
            .iter()
            .map(|name| {
                Grant::parse(name)
                    .map(|grant| grant.as_str().to_owned())
                    .map_err(|error| {
                        HarnessError::new(
                            codes::GRANT_UNKNOWN,
                            format!("{name}: {}", error.message),
                        )
                    })
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if let Some(declared) = &self.declared {
            let allowed = declared
                .get(self.side)
                .ok_or_else(|| invalid("selected component side is absent"))?;
            if let Some(name) = parsed.iter().find(|name| !allowed.contains(*name)) {
                return Err(HarnessError::new(
                    codes::GRANT_NOT_DECLARED,
                    format!("{} did not request {name} on {}", self.plugin, self.side),
                ));
            }
        }
        Ok(parsed)
    }

    pub(super) fn add_grants(&mut self, names: &[&str]) {
        self.ensure_open();
        let names = self
            .validate_grants(names)
            .unwrap_or_else(|error| panic!("{error}"));
        self.granted.extend(names);
    }

    pub(super) fn activation(&self, instance: u64, restored: Option<Vec<u8>>) -> Activation {
        Activation {
            plugin: PluginId(self.plugin.clone()),
            api: "ilium:plugin@0.1.0".to_owned(),
            instance,
            side: self.side.as_str().to_owned(),
            settings_json: "{}".to_owned(),
            granted: self.granted.iter().cloned().collect(),
            restored,
        }
    }
}
