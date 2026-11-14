//! Parse component-model text into actual WebAssembly component bytes for tests.

/// Test fixture parsing. Invalid WAT is a fixture error, not a host runtime result.
pub fn wat_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).unwrap_or_else(|error| panic!("invalid component fixture: {error}"))
}

/// A genuine component for install tests: it exports the required `lifecycle` and `handler`
/// interfaces plus the given optional exports, and carries the `ilium:api-version` custom
/// section for API 0.1.0 (reference-manifest.md, PKG-042 and PKG-043).
pub fn plugin_component(optional_exports: &[&str]) -> Vec<u8> {
    let mut text = String::from("(component\n");
    for (index, name) in ["lifecycle", "handler"]
        .iter()
        .chain(optional_exports.iter())
        .enumerate()
    {
        text.push_str(&format!(
            "  (instance $e{index})\n  (export \"ilium:exports/{name}@0.1.0\" (instance $e{index}))\n"
        ));
    }
    text.push(')');
    let mut bytes = wat_component(&text);
    // Custom section: id 0, size, name length, name, then major.minor.patch as three big-endian u16.
    let name = b"ilium:api-version";
    let payload = [0u8, 0, 0, 1, 0, 0];
    bytes.push(0);
    bytes.push((1 + name.len() + payload.len()) as u8);
    bytes.push(name.len() as u8);
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(&payload);
    bytes
}

/// The `files` inventory of a package: length and SHA-256 of every entry except `manifest.json`
/// and `signature`, bytewise sorted by path as the manifest reference requires.
pub fn files_inventory(files: &[(String, Vec<u8>)]) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let mut listed: Vec<_> = files
        .iter()
        .filter(|(path, _)| path != "manifest.json" && path != "signature")
        .collect();
    listed.sort_by(|left, right| left.0.cmp(&right.0));
    serde_json::Value::Array(
        listed
            .into_iter()
            .map(|(path, bytes)| {
                serde_json::json!({
                    "path": path,
                    "length": bytes.len(),
                    "sha256": format!("{:x}", Sha256::digest(bytes)),
                })
            })
            .collect(),
    )
}

/// A genuine `ilium:anim` scene component: it imports `frame-surface` (size, write-rows,
/// present) and exports `ilium:exports/renderer@0.1.0`. Each `render` checks that the
/// surface size equals the frame size, fills the whole field with grey `ink` through
/// `write-rows` in `gray8`, presents and drops its borrowed surface. It asks for 7 frames per
/// second, wants neither pointer nor occupancy, writes no cell colours, is always ready,
/// accepts every reconfigure and has no status.
pub fn scene_component_text(ink: u8) -> String {
    SCENE_COMPONENT.replace(
        "(i32.const 2048) (i32.const 200)",
        &format!("(i32.const 2048) (i32.const {ink})"),
    )
}

/// The scene of [`scene_component_text`] (ink 200) that also imports
/// `ilium:host/location@0.1.0` and whose `status` reports what `location.current` answered:
/// `located` for a position, `unlocated` for none. Instantiating it needs `location.observer`.
pub fn scene_component_with_location_text() -> String {
    let replacements = [
        (
            r#"  (alias export $surface "[method]frame-surface.present" (func $present))"#,
            r#"  (alias export $surface "[method]frame-surface.present" (func $present))
  (import "ilium:host/location@0.1.0" (instance $location
    (export "current" (func (result (option (tuple f64 f64)))))))
  (alias export $location "current" (func $location-current))"#,
        ),
        (
            r#"  (core func $surface-drop (canon resource.drop $fs))"#,
            r#"  (core func $surface-drop (canon resource.drop $fs))
  (core func $location-current-core (canon lower (func $location-current) (memory $libc "memory")))"#,
        ),
        (
            r#"    (import "host" "renderer-new" (func $renderer-new (param i32) (result i32)))"#,
            r#"    (import "host" "renderer-new" (func $renderer-new (param i32) (result i32)))
    (import "host" "location-current" (func $location-current (param i32)))
    (data (i32.const 1300) "located")
    (data (i32.const 1310) "unlocated")"#,
        ),
        (
            r#"    (func (export "status") (param i32) (result i32)
      (i32.store8 (i32.const 1100) (i32.const 0))
      (i32.const 1100)))"#,
            r#"    (func (export "status") (param i32) (result i32)
      (call $location-current (i32.const 1200))
      (i32.store8 (i32.const 1100) (i32.const 1))
      (if (i32.load8_u (i32.const 1200))
        (then
          (i32.store (i32.const 1104) (i32.const 1300))
          (i32.store (i32.const 1108) (i32.const 7)))
        (else
          (i32.store (i32.const 1104) (i32.const 1310))
          (i32.store (i32.const 1108) (i32.const 9))))
      (i32.const 1100)))"#,
        ),
        (
            r#"(export "renderer-new" (func $renderer-new))"#,
            r#"(export "renderer-new" (func $renderer-new))
      (export "location-current" (func $location-current-core))"#,
        ),
    ];
    let mut text = SCENE_COMPONENT.to_owned();
    for (anchor, replacement) in replacements {
        debug_assert_eq!(text.matches(anchor).count(), 1, "anchor {anchor}");
        text = text.replacen(anchor, replacement, 1);
    }
    text
}

const SCENE_COMPONENT: &str = r#"(component
  (import "ilium:anim/surface@0.1.0" (instance $surface
    (export "frame-surface" (type $fs (sub resource)))
    (type $pf-def (enum "mask8" "mono1" "gray8" "rgb8" "rgba8"))
    (export "pixel-format" (type $pf (eq $pf-def)))
    (type $code-def (enum "not-permitted" "not-found" "invalid-argument" "conflict" "quota-exceeded" "rate-limited" "busy" "timed-out" "cancelled" "unsupported" "pane-not-ready" "revoked" "io" "network" "internal"))
    (export "error-code" (type $code (eq $code-def)))
    (type $err-def (record (field "code" $code) (field "message" string) (field "retry-after-ms" (option u32))))
    (export "plugin-error" (type $err (eq $err-def)))
    (export "[method]frame-surface.size" (func (param "self" (borrow $fs)) (result (tuple u32 u32))))
    (export "[method]frame-surface.write-rows" (func (param "self" (borrow $fs)) (param "first-row" u32) (param "format" $pf) (param "data" (list u8)) (result (result (error $err)))))
    (export "[method]frame-surface.present" (func (param "self" (borrow $fs))))))
  (alias export $surface "frame-surface" (type $fs))
  (alias export $surface "[method]frame-surface.size" (func $size))
  (alias export $surface "[method]frame-surface.write-rows" (func $write-rows))
  (alias export $surface "[method]frame-surface.present" (func $present))
  (core module $libc
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 8192))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap))
  (core instance $libc (instantiate $libc))
  (core func $size-core (canon lower (func $size) (memory $libc "memory")))
  (core func $write-rows-core (canon lower (func $write-rows) (memory $libc "memory") (realloc (func $libc "realloc"))))
  (core func $present-core (canon lower (func $present)))
  (core func $surface-drop (canon resource.drop $fs))
  (type $sr (resource (rep i32)))
  (core func $renderer-new (canon resource.new $sr))
  (core module $scene
    (import "libc" "memory" (memory 1))
    (import "host" "size" (func $size (param i32 i32)))
    (import "host" "write-rows" (func $write-rows (param i32 i32 i32 i32 i32 i32)))
    (import "host" "present" (func $present (param i32)))
    (import "host" "surface-drop" (func $surface-drop (param i32)))
    (import "host" "renderer-new" (func $renderer-new (param i32) (result i32)))
    (func (export "new") (param i32 i32) (result i32)
      (call $renderer-new (i32.const 0)))
    (func (export "readiness") (param i32) (result i32) (i32.const 0))
    (func (export "frames-per-second") (param i32) (result i32) (i32.const 7))
    (func (export "no") (param i32) (result i32) (i32.const 0))
    (func (export "render") (param $self i32) (param i64 i64 i64) (param $width i32) (param $height i32) (param i32 i32) (param $surface i32)
      (local $length i32)
      (call $size (local.get $surface) (i32.const 1024))
      (if (i32.ne (i32.load (i32.const 1024)) (local.get $width)) (then unreachable))
      (if (i32.ne (i32.load (i32.const 1028)) (local.get $height)) (then unreachable))
      (local.set $length (i32.mul (local.get $width) (local.get $height)))
      (memory.fill (i32.const 2048) (i32.const 200) (local.get $length))
      (call $write-rows (local.get $surface) (i32.const 0) (i32.const 2) (i32.const 2048) (local.get $length) (i32.const 1040))
      (if (i32.load8_u (i32.const 1040)) (then unreachable))
      (call $present (local.get $surface))
      (call $surface-drop (local.get $surface)))
    (func (export "set-palette") (param i32 i32 i32))
    (func (export "reconfigure") (param i32 i32 i32) (result i32) (i32.const 1))
    (func (export "status") (param i32) (result i32)
      (i32.store8 (i32.const 1100) (i32.const 0))
      (i32.const 1100)))
  (core instance $scene (instantiate $scene
    (with "libc" (instance $libc))
    (with "host" (instance
      (export "size" (func $size-core))
      (export "write-rows" (func $write-rows-core))
      (export "present" (func $present-core))
      (export "surface-drop" (func $surface-drop))
      (export "renderer-new" (func $renderer-new))))))
  (type $readiness (enum "ready" "preparing" "failed"))
  (type $frame-input (record (field "time-millis" u64) (field "wall-millis" u64) (field "now-unix-millis" u64) (field "width" u32) (field "height" u32) (field "palette-rgb" (list u32))))
  (func $new (param "spec-json" string) (result (own $sr))
    (canon lift (core func $scene "new") (memory $libc "memory") (realloc (func $libc "realloc"))))
  (func $readiness-f (param "self" (borrow $sr)) (result $readiness)
    (canon lift (core func $scene "readiness")))
  (func $frames-per-second (param "self" (borrow $sr)) (result u32)
    (canon lift (core func $scene "frames-per-second")))
  (func $no (param "self" (borrow $sr)) (result bool)
    (canon lift (core func $scene "no")))
  (func $render (param "self" (borrow $sr)) (param "frame" $frame-input) (param "surface" (borrow $fs))
    (canon lift (core func $scene "render") (memory $libc "memory") (realloc (func $libc "realloc"))))
  (func $set-palette (param "self" (borrow $sr)) (param "palette-rgb" (list u32))
    (canon lift (core func $scene "set-palette") (memory $libc "memory") (realloc (func $libc "realloc"))))
  (func $reconfigure (param "self" (borrow $sr)) (param "settings-json" string) (result bool)
    (canon lift (core func $scene "reconfigure") (memory $libc "memory") (realloc (func $libc "realloc"))))
  (func $status (param "self" (borrow $sr)) (result (option string))
    (canon lift (core func $scene "status") (memory $libc "memory") (realloc (func $libc "realloc"))))
  (instance $renderer
    (export "scene-renderer" (type $sr))
    (export "readiness" (type $readiness))
    (export "frame-input" (type $frame-input))
    (export "[constructor]scene-renderer" (func $new))
    (export "[method]scene-renderer.readiness" (func $readiness-f))
    (export "[method]scene-renderer.frames-per-second" (func $frames-per-second))
    (export "[method]scene-renderer.wants-pointer" (func $no))
    (export "[method]scene-renderer.wants-occupancy" (func $no))
    (export "[method]scene-renderer.uses-cell-colors" (func $no))
    (export "[method]scene-renderer.render" (func $render))
    (export "[method]scene-renderer.set-palette" (func $set-palette))
    (export "[method]scene-renderer.reconfigure" (func $reconfigure))
    (export "[method]scene-renderer.status" (func $status)))
  (export "ilium:exports/renderer@0.1.0" (instance $renderer))
)"#;

#[cfg(test)]
mod tests {
    use super::wat_component;

    /// The 8-byte component preamble: magic `\0asm`, then layer 1 (component) with version 0x0d.
    const COMPONENT_PREAMBLE: [u8; 8] = [0x00, 0x61, 0x73, 0x6D, 0x0D, 0x00, 0x01, 0x00];

    /// REQ: PKG-141
    #[test]
    fn empty_component_text_compiles_to_component_preamble() {
        let bytes = wat_component("(component)");
        assert_eq!(bytes, COMPONENT_PREAMBLE.to_vec());
    }

    /// REQ: PKG-141
    #[test]
    fn output_starts_with_layer_one_component_magic() {
        let bytes = wat_component("(component)");
        assert!(
            bytes.len() >= 8,
            "output shorter than the preamble: {} bytes",
            bytes.len()
        );
        assert_eq!(&bytes[0..4], &[0x00, 0x61, 0x73, 0x6D]);
        assert_eq!(bytes[4], 0x0D, "layer field must mark a component");
    }

    #[test]
    fn same_text_compiles_to_identical_bytes() {
        let text = "(component (core module $m (func (export \"f\"))))";
        let first = wat_component(text);
        let second = wat_component(text);
        assert_eq!(first, second);
    }

    #[test]
    fn component_with_core_module_is_longer_than_empty_component() {
        let empty = wat_component("(component)");
        let with_module = wat_component("(component (core module $m (func (export \"f\"))))");
        assert!(
            with_module.len() > empty.len(),
            "module-bearing component ({} bytes) must exceed empty component ({} bytes)",
            with_module.len(),
            empty.len()
        );
    }

    #[test]
    fn whitespace_around_component_text_does_not_change_output() {
        let compact = wat_component("(component)");
        let padded = wat_component("  \n(component)\n  ");
        assert_eq!(compact, padded);
    }

    #[test]
    fn empty_input_does_not_yield_a_bare_component_preamble() {
        // The doc comment defines the input as `(component ...)` text. Empty text is not that
        // form, so it must not compile to the bare 8-byte empty component.
        let bytes = wat_component("");
        assert_ne!(bytes, COMPONENT_PREAMBLE.to_vec());
    }
}
