//! Pure returned-submission admission shared by native test hosts and runtime adapters.
//! Typed wire decoding establishes variant/field shapes. This module owns the documented
//! batch-level dedup/scoped-name checks, then grant checks, without applying effects.

use crate::abi::{self, BatchCheck, BatchVerdict};
use crate::dedup::DedupKey;
use crate::generated::types::errors::{ErrorCode, PluginError};
use crate::generated::types::intents::{Intent, Submission};
use crate::grants::{self, Grant};

fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::new(ErrorCode::InvalidArgument, message)
}

/// A typed submission's batch-level shape checks. Per-target/value/rate/quota checks belong
/// to the effect adapter; they must not accidentally turn into whole-batch refusals here.
pub fn check_shape(submission: &Submission) -> Result<(), PluginError> {
    match &submission.dedup_key {
        Some(key) => {
            DedupKey::parse(key)
                .map_err(|error| invalid(format!("dedup-key-invalid: {}", error.message)))?;
        }
        None if matches!(
            &submission.intent,
            Intent::SendInput(_) | Intent::AgentAction(_)
        ) =>
        {
            return Err(invalid("dedup-key-invalid: this intent requires a key"));
        }
        None => {}
    }
    // Scoped target syntax is shape, while possession/own-contribution checks are grants.
    if let Err(error) = grants::required_by_intent(&submission.intent, "") {
        if error.code == ErrorCode::InvalidArgument {
            return Err(error);
        }
    }
    Ok(())
}

/// Checks only authorization against the canonical catalogue, including scoped wildcard
/// coverage and conditional input grants. The caller supplies the authenticated origin id.
pub fn check_grants(
    submission: &Submission,
    origin_plugin: &str,
    held: &[Grant],
) -> Result<(), PluginError> {
    let required = grants::required_by_intent(&submission.intent, origin_plugin)?;
    if let Some(missing) = required
        .iter()
        .find(|required| !held.iter().any(|grant| grant.covers(required)))
    {
        return Err(PluginError::new(
            ErrorCode::NotPermitted,
            format!("missing grant {}", missing.as_str()),
        ));
    }
    Ok(())
}

/// Atomic batch admission: all shapes first, then all grants, using abi::batch_verdicts.
/// None means the effect adapter may consider that submission; Some is its exact refusal.
/// This deliberately does not reserve dedup keys or perform per-submission effects.
pub fn batch_refusals(
    submissions: &[Submission],
    origin_plugin: &str,
    held: &[Grant],
) -> Vec<Option<PluginError>> {
    let shape_errors: Vec<_> = submissions
        .iter()
        .map(|item| check_shape(item).err())
        .collect();
    let grant_errors: Vec<_> = if shape_errors.iter().all(Option::is_none) {
        submissions
            .iter()
            .map(|item| check_grants(item, origin_plugin, held).err())
            .collect()
    } else {
        vec![None; submissions.len()]
    };
    let checks: Vec<_> = shape_errors
        .iter()
        .zip(&grant_errors)
        .map(|(shape, grant)| BatchCheck {
            shape_ok: shape.is_none(),
            grant_ok: grant.is_none(),
        })
        .collect();
    abi::batch_verdicts(&checks)
        .into_iter()
        .enumerate()
        .map(|(index, verdict)| match verdict {
            BatchVerdict::Proceed => None,
            BatchVerdict::Cancelled(message) => {
                Some(PluginError::new(ErrorCode::Cancelled, message))
            }
            BatchVerdict::Refused(code) => {
                let mut error = shape_errors[index]
                    .clone()
                    .or_else(|| grant_errors[index].clone())
                    .unwrap_or_else(|| PluginError::new(code, "batch refused"));
                // The harness dedup diagnostic is a specifically required prefix (DEV-435).
                // Other offenders and every sibling use the normative batch refusal text.
                if !error.message.starts_with("dedup-key-invalid:") {
                    error.message = format!("{}{}", abi::BATCH_REFUSED_PREFIX, index);
                }
                error.code = code;
                Some(error)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::types::ids::NodeId;
    use crate::generated::types::intents::{
        AgentActionSpec, CreatePaneSpec, InvokeCommandSpec, SendInputSpec,
    };

    fn item(intent: Intent, key: Option<&str>) -> Submission {
        Submission {
            intent,
            correlation: 1,
            dedup_key: key.map(str::to_owned),
        }
    }

    fn grants(names: &[&str]) -> Vec<Grant> {
        names
            .iter()
            .map(|name| Grant::parse(name).expect("literal grant"))
            .collect()
    }

    #[test]
    fn later_bad_shape_wins_over_earlier_missing_permission() {
        let batch = vec![
            item(Intent::CloseNode(NodeId(1)), None),
            item(Intent::PlaySound("ding".into()), Some("bad key")),
        ];
        let results = batch_refusals(&batch, "test.plugin", &grants(&["sound.play"]));
        assert_eq!(
            results[0].as_ref().expect("sibling").code,
            ErrorCode::Cancelled
        );
        assert_eq!(
            results[0].as_ref().expect("sibling").message,
            "batch refused: submission 1"
        );
        assert_eq!(
            results[1].as_ref().expect("offender").code,
            ErrorCode::InvalidArgument
        );
        assert!(results[1]
            .as_ref()
            .expect("offender")
            .message
            .starts_with("dedup-key-invalid:"));
    }

    #[test]
    fn missing_mandatory_key_is_malformed_before_permission_check() {
        let submission = item(Intent::AgentAction(AgentActionSpec::default()), None);
        assert_eq!(
            check_shape(&submission).expect_err("missing key").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn create_pane_initial_input_needs_both_grants() {
        let submission = item(
            Intent::CreatePane(CreatePaneSpec {
                initial_input: Some("hello".into()),
                ..CreatePaneSpec::default()
            }),
            None,
        );
        assert_eq!(
            check_grants(&submission, "test.plugin", &grants(&["panes.create"]))
                .expect_err("input permission")
                .code,
            ErrorCode::NotPermitted
        );
        assert!(check_grants(
            &submission,
            "test.plugin",
            &grants(&["panes.create", "terminal.write"])
        )
        .is_ok());
    }

    #[test]
    fn raw_input_and_lock_require_every_conditional_grant() {
        let submission = item(
            Intent::SendInput(SendInputSpec {
                pane: NodeId(1),
                text: String::new(),
                delivery: crate::generated::types::intents::Delivery::Typed,
                prompt_epoch: None,
                stages: Vec::new(),
                raw: Some(vec![b'x']),
                mouse: None,
                if_current: None,
                lock_user_input: true,
                journal: false,
            }),
            Some("input"),
        );
        assert!(check_shape(&submission).is_ok());
        assert!(check_grants(&submission, "test.plugin", &grants(&["terminal.write"])).is_err());
        assert!(check_grants(
            &submission,
            "test.plugin",
            &grants(&["terminal.write", "terminal.raw-input"])
        )
        .is_err());
        assert!(check_grants(
            &submission,
            "test.plugin",
            &grants(&[
                "terminal.write",
                "terminal.raw-input",
                "terminal.input-lock"
            ])
        )
        .is_ok());
    }

    #[test]
    fn scoped_command_coverage_uses_the_existing_grant_parser() {
        let submission = item(
            Intent::InvokeCommand(InvokeCommandSpec {
                command: "example.tools/run".into(),
                arguments_json: "{}".into(),
            }),
            None,
        );
        assert!(check_grants(
            &submission,
            "test.plugin",
            &grants(&["commands.invoke:example.tools/*"])
        )
        .is_ok());
        assert!(check_grants(
            &submission,
            "test.plugin",
            &grants(&["commands.invoke:example.other/*"])
        )
        .is_err());
    }

    #[test]
    fn empty_batch_has_no_refusal() {
        assert!(batch_refusals(&[], "test.plugin", &[]).is_empty());
    }

    #[test]
    fn grant_refusal_preserves_the_lowest_offender_and_cancels_siblings() {
        let results = batch_refusals(
            &[
                item(Intent::PlaySound("ding".into()), None),
                item(Intent::CloseNode(NodeId(2)), None),
            ],
            "test.plugin",
            &[],
        );
        assert_eq!(
            results[0].as_ref().expect("offender").code,
            ErrorCode::NotPermitted
        );
        assert_eq!(
            results[0].as_ref().expect("offender").message,
            "batch refused: submission 0"
        );
        assert_eq!(
            results[1].as_ref().expect("sibling").code,
            ErrorCode::Cancelled
        );
    }
}
