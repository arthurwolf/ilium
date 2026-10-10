//! Private animation helper. Operational CLI output is JSONL; --ipc explicitly
//! selects the bounded binary protocol on stdin/stdout (never human logging).
fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments == ["--version"] {
        println!("{}", version_record());
        return;
    }
    if arguments == ["--help"] {
        println!(
            "{{\"type\":\"result\",\"usage\":\"ilium-animation-helper --version | --ipc (private bounded binary stdin/stdout protocol)\"}}"
        );
        return;
    }
    if arguments != ["--ipc"] {
        println!(
            "{{\"type\":\"error\",\"message\":\"expected exactly --ipc, --version or --help\"}}"
        );
        std::process::exit(64);
    }
    #[cfg(feature = "v8-runtime")]
    if let Err(error) = ilium_animation_js::helper::run_helper_ipc() {
        // Never place diagnostics in a binary transport, and never attempt an
        // unrestricted fallback when initialization/isolation fails.
        if std::env::var_os("ILIUM_ANIMATION_SANDBOX_DIAGNOSTICS").is_some() {
            eprintln!("animation helper initialization failed: {error}");
        }
        std::process::exit(70);
    }
    #[cfg(not(feature = "v8-runtime"))]
    {
        println!("{{\"type\":\"error\",\"message\":\"helper requires v8-runtime feature\"}}");
        std::process::exit(78);
    }
}

fn version_record() -> String {
    format!(
        "{{\"type\":\"result\",\"command\":\"version\",\"name\":\"ilium-animation-helper\",\"version\":\"{}\"}}",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_identity_is_one_jsonl_result() {
        let output = super::version_record();
        let record: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(record["type"], "result");
        assert_eq!(record["command"], "version");
        assert_eq!(record["name"], "ilium-animation-helper");
        assert_eq!(record["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(output.lines().count(), 1);
    }
}
