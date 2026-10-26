//! Compile one trusted release component with the same Wasmtime settings as the host.
//! The release builder must call this for every bundled component, then embed both
//! the source `.wasm` and generated `.cwasm` in the host's static catalogue.

use std::path::Path;

use ilium_plugin_host::engine::{config_fingerprint, runtime_config};
use wasmtime::Engine;

fn run() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let source = args.next().ok_or_else(|| anyhow::anyhow!("usage: ilium-plugin-precompile INPUT.wasm OUTPUT.cwasm"))?;
    let output = args.next().ok_or_else(|| anyhow::anyhow!("usage: ilium-plugin-precompile INPUT.wasm OUTPUT.cwasm"))?;
    if args.next().is_some() { return Err(anyhow::anyhow!("expected exactly two paths")); }
    let source = Path::new(&source);
    let output = Path::new(&output);
    let wasm = std::fs::read(source)?;
    let config = runtime_config();
    let engine = Engine::new(&config).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let cwasm = engine.precompile_component(&wasm).map_err(|error| anyhow::anyhow!(error.to_string()))?;
    if let Some(parent) = output.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::write(output, &cwasm)?;
    let fingerprint = config_fingerprint();
    let fingerprint_path = output.with_extension("fingerprint");
    std::fs::write(&fingerprint_path, &fingerprint)?;
    println!("{}", serde_json::json!({
        "type": "artifact",
        "input": source.display().to_string(),
        "output": output.display().to_string(),
        "fingerprint": fingerprint,
        "fingerprint_file": fingerprint_path.display().to_string(),
        "wasm_bytes": wasm.len(),
        "cwasm_bytes": cwasm.len(),
    }));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{}", serde_json::json!({"type": "error", "message": error.to_string()}));
        std::process::exit(1);
    }
}
