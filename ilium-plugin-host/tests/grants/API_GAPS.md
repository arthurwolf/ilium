# API gaps: lane grants, ilium-plugin-host

Items the host grants tests use that the skeleton lacks. Signatures are as the tests call them.

- ilium-plugin-host::testing: `HostHarness::{declined(&[&str]), other_side_grants(Side,&[&str]), kernel(Arc<FakeKernel>), with_native_guest(Box<dyn NativeGuest>)}`, `HarnessHandle::{call_import(&str, serde_json::Value) -> ImportResult, revoke(&str), grant_now(&str), pump(usize), deliver(Vec<Event>), outcomes(&PluginId), logs(), failures(), state(), epoch()}`, `enum ImportResult{Ok,Err(PluginError),Trap(Trap)}`, `FakeKernel::hold_next_request()`. Ids: CAP-011..018.
- ilium-plugin-host::calls: `Trap.code.as_str()` returning `host.capability-not-requested` (manual also says `trap.import-not-requested` in API-081). Id: CAP-018.
- ilium-plugin-host::review: `display_name(&str)->String`, `review_rows(&[Grant])->Vec<Row{details,is_toxic,risk_rank}>`, `update_review(&[Grant],&[Grant])->UpdateReview{new,wider,removed,unchanged}`, `Widening`, `scroll_indicator(usize,usize)->String`, `DeclineCounter`. Ids: CAP-016, SEC-104..110, SEC-122.
- ilium-plugin-host::consent: `ConsentGate`, `ConsentOutcome{Busy,RateLimited,TimedOut,Declined}`, `DialogGuard`, `DialogKind{Review,Consent,Update,Trust}` with `is_kernel_drawn`, `source_label`. Ids: SEC-102,103,105,121.
- ilium-plugin-host::stores: `GrantStore, ConsentStore, TrustStore, AuditLog, StoreError::UnsupportedVersion(u32)`. Ids: SEC-109..114,118,119,123.
- ilium-plugin-host::approval: `ApprovalChannel, ApprovalError::ReviewRequired`. Ids: SEC-116,117.
- ilium-plugin-host: ilium-plugin-host::package::write_package; cli::{CliEnv, run_cli}. Ids: SEC-100, 117, 120.
- ilium-plugin-host dev-dependencies: `sha2`, `serde_json`, `tempfile`.
- Risk tier "always" and `is_always_granted` missing in ilium-plugin-api (CAP-011).
