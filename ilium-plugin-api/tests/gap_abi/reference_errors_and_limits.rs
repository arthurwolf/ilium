//! ABI-063 and ABI-082 from reference-errors-and-limits.md.

use ilium_plugin_api::abi::{admit_decoded, refusal_metric_count, ValueOrigin};
use ilium_plugin_api::conformance::run_corpus_with_message_override;
use ilium_plugin_api::limits::LimitKey;

fn errors_and_limits_page() -> String {
    let path = format!(
        "{}/../docs/plugins-manual/pages/reference-errors-and-limits.md",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).expect("manual page should be readable")
}

/// REQ: ABI-063
#[test]
fn abi_063_decode_limits_match_for_plugin_return_and_host_event() {
    let refusal_count_before = refusal_metric_count("abi.limit-exceeded");
    for key in LimitKey::ALL {
        let oversize_length = ilium_plugin_api::limits::default_of(key) + 1;
        let from_plugin = admit_decoded(ValueOrigin::PluginReturn, key, oversize_length);
        let from_host_event = admit_decoded(ValueOrigin::HostEvent, key, oversize_length);
        assert_eq!(
            from_plugin.result, from_host_event.result,
            "{key:?}: the same oversize decode must get the same result from both origins"
        );
        assert!(
            from_plugin.result.is_err(),
            "{key:?}: a value one past the limit must be refused from a plugin return"
        );
    }
    assert!(
        refusal_metric_count("abi.limit-exceeded") > refusal_count_before,
        "each refusal must be counted in the ABI refusal metric of ABI-079"
    );
}

/// REQ: ABI-082
#[test]
fn abi_082_reworded_message_tail_keeps_corpus_passing() {
    let report = run_corpus_with_message_override("reworded tail, same code and reason prefix");
    assert!(
        report.all_passed(),
        "the corpus must not assert on the non-prefix text of a message: {:?}",
        report.failures()
    );
    let page = errors_and_limits_page();
    assert!(
        page.contains("[ABI-082] Within one ABI major, only the error code and the documented reason-word prefix"),
        "reference-errors-and-limits.md must state the ABI-082 MUST text"
    );
}

/// REQ: ABI-082
#[test]
fn abi_082_sdks_are_told_not_to_branch_on_message_text() {
    let page = errors_and_limits_page();
    assert!(
        page.lines().any(|line| {
            line.contains("[ABI-082]") && line.contains("the SDKs MUST NOT branch on message text")
        }),
        "the ABI-082 requirement must forbid SDKs from branching on message text"
    );
}

/// REQ: ABI-063
#[test]
fn abi_063_oversize_decode_is_refused_before_host_logic_runs_from_either_origin() {
    for key in LimitKey::ALL {
        let oversize_length = ilium_plugin_api::limits::default_of(key) + 1;
        for origin in [ValueOrigin::PluginReturn, ValueOrigin::HostEvent] {
            let admission = admit_decoded(origin, key, oversize_length);
            assert!(
                !admission.host_logic_reached,
                "{key:?} from {origin:?}: decoding work must stop before host logic reads the value"
            );
        }
    }
}
