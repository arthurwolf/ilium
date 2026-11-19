//! Minimal pane state and verified message, without prompts or screen contents.
use ilium_core::NodeId;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Brief {
    pub pane: NodeId,
    pub state: String,
    pub last_message: Option<String>,
    pub state_age_seconds: Option<u64>,
}

pub fn pane_brief(
    pane: NodeId,
    state: &str,
    message: Option<&str>,
    since: Option<u64>,
    now: u64,
) -> Brief {
    let last_message = message
        .map(|text| {
            text.chars()
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .collect::<String>()
        })
        .map(|text| text.trim().chars().take(300).collect::<String>())
        .filter(|text| !text.is_empty());
    Brief {
        pane,
        state: state.to_owned(),
        last_message,
        state_age_seconds: since.map(|since| now.saturating_sub(since) / 1000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brief_is_bounded_and_age_is_unknown_without_observed_state() {
        let result = pane_brief(
            NodeId(7),
            "working",
            Some(&format!("\n{}", "é".repeat(400))),
            Some(1000),
            4500,
        );
        assert_eq!(result.state_age_seconds, Some(3));
        let text = result.last_message.unwrap();
        assert_eq!(text.chars().count(), 300);
        assert!(!text.chars().any(char::is_control));
        let unknown = pane_brief(NodeId(7), "shell", None, None, 4500);
        assert!(unknown.last_message.is_none() && unknown.state_age_seconds.is_none());
        assert_eq!(
            pane_brief(NodeId(7), "done", None, Some(5000), 4500).state_age_seconds,
            Some(0)
        );
    }
}
