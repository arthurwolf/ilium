//! Verify the production sandbox against an explicitly synthetic provider goal.
//! The probe reports only whether the requested synthetic row matched.

#[cfg(target_os = "linux")]
fn main() {
    use std::{path::Path, time::Duration};

    let arguments: Vec<_> = std::env::args().collect();
    if arguments.len() != 5 || !arguments[3].starts_with("Synthetic ") {
        println!("{{\"type\":\"error\",\"message\":\"invalid synthetic probe arguments\"}}");
        std::process::exit(2);
    }
    let request = serde_json::json!({"thread_id": arguments[2]}).to_string();
    let result =
        ilium_platform::codex_goal_sandbox::CodexGoalSandbox::spawn(Path::new(&arguments[1]))
            .and_then(|sandbox| sandbox.query(request.as_bytes(), Duration::from_secs(10)))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let matched = result
        .as_ref()
        .and_then(|value| value.get("rows"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|rows| {
            rows.len() == 1
                && rows[0][0] == arguments[2]
                && rows[0][1] == arguments[4]
                && rows[0][2] == arguments[3]
        });
    println!(
        "{}",
        serde_json::json!({"type":"result", "matched":matched})
    );
    if !matched {
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("{{\"type\":\"error\",\"message\":\"Linux sandbox unavailable\"}}");
    std::process::exit(1);
}
