//! Submission batches: ABI-008, ABI-009, ABI-031, ABI-032.

use ilium_plugin_api::abi::{
    batch_verdicts, check_submission_count, BatchCheck, BatchVerdict, BATCH_REFUSED_PREFIX,
    SUBMISSION_BATCH_LIMIT,
};
use ilium_plugin_api::codes;
use ilium_plugin_api::ErrorCode;
use proptest::prelude::*;

const OK: BatchCheck = BatchCheck {
    shape_ok: true,
    grant_ok: true,
};
const BAD_SHAPE: BatchCheck = BatchCheck {
    shape_ok: false,
    grant_ok: true,
};
const NO_GRANT: BatchCheck = BatchCheck {
    shape_ok: true,
    grant_ok: false,
};
const BOTH_BAD: BatchCheck = BatchCheck {
    shape_ok: false,
    grant_ok: false,
};

fn cancelled(index: usize) -> BatchVerdict {
    BatchVerdict::Cancelled(format!("{BATCH_REFUSED_PREFIX}{index}"))
}

/// REQ: ABI-008
#[test]
fn abi_008_limit_is_256() {
    assert_eq!(SUBMISSION_BATCH_LIMIT, 256);
}

/// REQ: ABI-008
#[test]
fn abi_008_256_submissions_are_accepted() {
    assert_eq!(check_submission_count(256), Ok(()));
}

/// REQ: ABI-008
#[test]
fn abi_008_257_submissions_trap() {
    let code = check_submission_count(257).expect_err("over the limit");
    assert_eq!(code.as_str(), codes::trap::SUBMISSION_BATCH);
}

/// REQ: ABI-008
#[test]
fn abi_008_zero_submissions_are_accepted() {
    assert_eq!(check_submission_count(0), Ok(()));
}

/// REQ: ABI-009
#[test]
fn abi_009_message_prefix_is_exact() {
    assert_eq!(BATCH_REFUSED_PREFIX, "batch refused: submission ");
}

/// REQ: ABI-009
#[test]
fn abi_009_bad_shape_refuses_invalid_argument_and_cancels_the_rest() {
    assert_eq!(
        batch_verdicts(&[OK, OK, BAD_SHAPE, OK]),
        vec![
            cancelled(2),
            cancelled(2),
            BatchVerdict::Refused(ErrorCode::InvalidArgument),
            cancelled(2),
        ]
    );
}

/// REQ: ABI-009
#[test]
fn abi_009_missing_grant_refuses_not_permitted() {
    assert_eq!(
        batch_verdicts(&[NO_GRANT, OK]),
        vec![BatchVerdict::Refused(ErrorCode::NotPermitted), cancelled(0)]
    );
}

/// REQ: ABI-009
#[test]
fn abi_009_only_the_lowest_index_offender_is_refused() {
    assert_eq!(
        batch_verdicts(&[OK, BAD_SHAPE, BAD_SHAPE]),
        vec![
            cancelled(1),
            BatchVerdict::Refused(ErrorCode::InvalidArgument),
            cancelled(1),
        ]
    );
}

/// REQ: ABI-031
#[test]
fn abi_031_shape_pass_runs_before_grant_pass() {
    // A missing grant at index 1 and a bad shape at index 2: index 2 is refused.
    assert_eq!(
        batch_verdicts(&[OK, NO_GRANT, BAD_SHAPE]),
        vec![
            cancelled(2),
            cancelled(2),
            BatchVerdict::Refused(ErrorCode::InvalidArgument),
        ]
    );
}

/// REQ: ABI-031
#[test]
fn abi_031_inside_one_submission_shape_wins_over_grant() {
    assert_eq!(
        batch_verdicts(&[BOTH_BAD]),
        vec![BatchVerdict::Refused(ErrorCode::InvalidArgument)]
    );
}

/// REQ: ABI-032
#[test]
fn abi_032_clean_batch_proceeds_to_per_submission_checks() {
    assert_eq!(
        batch_verdicts(&[OK, OK, OK]),
        vec![BatchVerdict::Proceed; 3]
    );
}

/// REQ: ABI-032
#[test]
fn abi_032_empty_batch_has_no_verdicts() {
    assert_eq!(batch_verdicts(&[]), Vec::<BatchVerdict>::new());
}

fn check_strategy() -> impl Strategy<Value = BatchCheck> {
    (any::<bool>(), any::<bool>())
        .prop_map(|(shape_ok, grant_ok)| BatchCheck { shape_ok, grant_ok })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // REQ: ABI-008
    #[test]
    fn abi_008_count_rule_holds_for_any_count(count in 0usize..2048) {
        prop_assert_eq!(check_submission_count(count).is_ok(), count <= 256);
    }

    // REQ: ABI-009, ABI-031
    #[test]
    fn abi_031_verdicts_follow_the_two_pass_rule(checks in proptest::collection::vec(check_strategy(), 0..64)) {
        let verdicts = batch_verdicts(&checks);
        prop_assert_eq!(verdicts.len(), checks.len());
        let first_shape = checks.iter().position(|check| !check.shape_ok);
        let first_grant = checks.iter().position(|check| !check.grant_ok);
        let offender = first_shape.or(first_grant);
        match offender {
            None => prop_assert!(verdicts.iter().all(|verdict| *verdict == BatchVerdict::Proceed)),
            Some(index) => {
                let expected_code = if first_shape.is_some() { ErrorCode::InvalidArgument } else { ErrorCode::NotPermitted };
                for (position, verdict) in verdicts.iter().enumerate() {
                    if position == index {
                        prop_assert_eq!(verdict, &BatchVerdict::Refused(expected_code));
                    } else {
                        prop_assert_eq!(verdict, &cancelled(index));
                    }
                }
            }
        }
    }
}
