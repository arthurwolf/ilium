use std::sync::{Arc, Mutex, atomic::{AtomicU64,Ordering}};
use ilium_plugin_api::{Side,Tier,generated::host::log::{HostLog,Level}};
use ilium_plugin_host::{imports::log::{LogClock,LogDiagnostics,LogFailure,LogImport,LogTime,PluginLogger},telemetry::{LogConfig,LogLevel,PluginLogInstanceBudget,Refusal}};

struct Clock(AtomicU64);
impl LogClock for Clock {
    fn now(&self)->Result<LogTime,Refusal> { Ok(LogTime {local_timestamp:"2026-10-23 12:00:00".into(),monotonic_millis:self.0.load(Ordering::SeqCst)}) }
}
#[derive(Default)]
struct Diagnostics(Mutex<Vec<LogFailure>>);
impl LogDiagnostics for Diagnostics { fn failed(&self,failure:LogFailure) { self.0.lock().unwrap().push(failure); } }
fn setup(config:LogConfig, secrets:&[String],capacity:usize) -> (tempfile::TempDir,Arc<Mutex<PluginLogger>>,Arc<Clock>,Arc<Diagnostics>,LogImport) {
    let root=tempfile::tempdir().unwrap();
    let logger=Arc::new(Mutex::new(PluginLogger::open(root.path(),"example.logger",Tier::T2,config,capacity,secrets).unwrap()));
    let clock=Arc::new(Clock(AtomicU64::new(0))); let diagnostics=Arc::new(Diagnostics::default());
    let import=LogImport::new(logger.clone(),Arc::new(Mutex::new(PluginLogInstanceBudget::new(1,Side::Server).unwrap())),clock.clone(),diagnostics.clone()).unwrap();
    (root,logger,clock,diagnostics,import)
}
fn text(root:&tempfile::TempDir)->String { std::fs::read_to_string(root.path().join("example.logger.log")).unwrap() }
#[test]
fn all_severities_map_and_guest_cannot_impersonate_host_side() {
    let (root,logger,_clock,diagnostics,mut import)=setup(LogConfig {threshold:LogLevel::Trace,..LogConfig::default()},&[],8);
    for level in [Level::Trace,Level::Debug,Level::Info,Level::Warn,Level::Error] { import.log(level,"guest record".into()); }
    let levels:Vec<_>=logger.lock().unwrap().lines().into_iter().map(|line|line.level).collect();
    assert_eq!(levels,["trace","debug","info","warn","error"]);
    let output=text(&root); assert_eq!(output.lines().count(),5);
    assert!(output.lines().all(|line|line.contains("server example.logger")));
    assert!(diagnostics.0.lock().unwrap().is_empty());
}
#[test]
fn redaction_precedes_persistence_and_ring_capture_including_encoded_variants() {
    let secret="actual-secret-123".to_string();
    let variants=[secret.clone(),ilium_plugin_host::secrets::base64_standard(secret.as_bytes()),ilium_plugin_host::secrets::hex_lower(secret.as_bytes()),ilium_plugin_host::secrets::percent_encode(&secret)];
    let (root,logger,_clock,_diagnostics,mut import)=setup(LogConfig::default(),&[secret],8);
    import.log(Level::Info,variants.join(" | "));
    let disk=text(&root); let ring=logger.lock().unwrap().lines();
    for variant in variants { assert!(!disk.contains(&variant)); assert!(!ring[0].message.contains(&variant)); }
    assert!(disk.contains("<redacted>")); assert!(ring[0].message.contains("<redacted>"));
    assert!(!format!("{import:?}").contains("actual-secret"));
}
#[test]
fn unicode_messages_are_cut_to_real_byte_limit_and_ring_is_bounded() {
    let (_root,logger,_clock,_diagnostics,mut import)=setup(LogConfig::default(),&[],2);
    import.log(Level::Info,"😀".repeat(5000));
    let first=logger.lock().unwrap().lines()[0].message.clone();
    assert!(first.len()<=16384); assert!(first.ends_with("…[truncated]"));
    import.log(Level::Warn,"second".into()); import.log(Level::Error,"third".into());
    let lines=logger.lock().unwrap().lines(); assert_eq!(lines.len(),2); assert_eq!(lines[0].message,"second");
}
#[test]
fn multiline_is_one_escaped_entry_and_quota_drop_summary_is_real() {
    let (root,logger,clock,_diagnostics,mut import)=setup(LogConfig {rate_per_second:2,..LogConfig::default()},&[],8);
    import.log(Level::Info,"one\ntwo\r\nthree".into());
    import.log(Level::Info,"second entry".into()); import.log(Level::Error,"dropped entry".into());
    assert_eq!(logger.lock().unwrap().lines().len(),2);
    assert_eq!(text(&root).lines().count(),2); assert!(text(&root).contains("one\\ntwo\\r\\nthree"));
    clock.0.store(1000,Ordering::SeqCst); import.tick();
    assert!(text(&root).contains("1 lines dropped in the last second")); assert!(!text(&root).contains("dropped entry"));
}
#[test]
fn threshold_drops_are_not_captured_or_reported_as_sink_failure() {
    let (root,logger,_clock,diagnostics,mut import)=setup(LogConfig::default(),&[],8);
    import.log(Level::Debug,"hidden detail".into());
    assert!(logger.lock().unwrap().lines().is_empty()); assert!(!text(&root).contains("hidden detail"));
    assert!(diagnostics.0.lock().unwrap().is_empty());
}
#[test]
fn actual_persistence_failure_is_reported_without_message_or_secret_payload() {
    let (root,logger,_clock,diagnostics,mut import)=setup(LogConfig::default(),&["private-value".into()],8);
    let path=root.path().join("example.logger.log"); std::fs::remove_file(&path).unwrap(); std::fs::create_dir(&path).unwrap();
    import.log(Level::Error,"private-value must never reach failure callback".into());
    assert!(logger.lock().unwrap().lines().is_empty());
    let errors=diagnostics.0.lock().unwrap(); assert_eq!(errors.len(),1); assert_eq!(errors[0].plugin,"example.logger"); assert_eq!(errors[0].side,Side::Server);
    assert!(!format!("{errors:?}").contains("private-value"));
}
#[test]
fn ordinary_tier_cannot_claim_trusted_rate_ceiling() {
    let root=tempfile::tempdir().unwrap();
    assert!(PluginLogger::open(root.path(),"example.logger",Tier::T2,LogConfig {rate_per_second:1000,..LogConfig::default()},8,&[]).is_err());
}
struct BrokenClock;
impl LogClock for BrokenClock {
    fn now(&self)->Result<LogTime,Refusal> { Err(Refusal {code:"log.clock-unavailable".into(),message:"host-private-clock-detail".into()}) }
}
struct PanickingDiagnostics;
impl LogDiagnostics for PanickingDiagnostics { fn failed(&self,_failure:LogFailure) { panic!("test callback failure"); } }
#[test]
fn clock_failure_is_payload_free_and_broken_callback_cannot_escape_void_import() {
    let (_root,logger,_clock,diagnostics,_import)=setup(LogConfig::default(),&[],8);
    let mut import=LogImport::new(logger.clone(),Arc::new(Mutex::new(PluginLogInstanceBudget::new(2,Side::Client).unwrap())),Arc::new(BrokenClock),diagnostics.clone()).unwrap();
    import.log(Level::Info,"private guest message".into());
    assert!(logger.lock().unwrap().lines().is_empty());
    let errors=diagnostics.0.lock().unwrap(); assert_eq!(errors.len(),1); assert_eq!(errors[0].code,"log.clock-unavailable");
    assert!(!format!("{errors:?}").contains("private")); drop(errors);
    let mut import=LogImport::new(logger,Arc::new(Mutex::new(PluginLogInstanceBudget::new(3,Side::Scene).unwrap())),Arc::new(BrokenClock),Arc::new(PanickingDiagnostics)).unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||import.log(Level::Error,"guest message".into()))).is_ok());
}

#[test]
fn instance_handles_share_quota_while_distinct_instances_remain_independent() {
    let (root,logger,clock,diagnostics,_unused)=setup(LogConfig {rate_per_second:1,..LogConfig::default()},&[],8);
    let budget=Arc::new(Mutex::new(PluginLogInstanceBudget::new(40,Side::Server).unwrap()));
    let mut first=LogImport::new(logger.clone(),budget.clone(),clock.clone(),diagnostics.clone()).unwrap();
    let mut same_instance=LogImport::new(logger.clone(),budget,clock.clone(),diagnostics.clone()).unwrap();
    let mut other_instance=LogImport::new(logger.clone(),Arc::new(Mutex::new(PluginLogInstanceBudget::new(41,Side::Server).unwrap())),clock.clone(),diagnostics.clone()).unwrap();
    let mut client_instance=LogImport::new(logger.clone(),Arc::new(Mutex::new(PluginLogInstanceBudget::new(42,Side::Client).unwrap())),clock.clone(),diagnostics.clone()).unwrap();
    first.log(Level::Info,"first admitted".into());
    same_instance.log(Level::Error,"same instance suppressed".into());
    other_instance.log(Level::Info,"other server admitted".into());
    client_instance.log(Level::Info,"client admitted".into());
    let output=text(&root);
    assert_eq!(output.lines().count(),3);
    assert!(!output.contains("same instance suppressed"));
    assert!(output.contains("other server admitted"));
    assert!(output.contains("client example.logger"));
    assert_eq!(logger.lock().unwrap().lines().len(),3);
    clock.0.store(1000,Ordering::SeqCst);
    same_instance.tick();
    first.tick();
    assert_eq!(text(&root).matches("1 lines dropped in the last second").count(),1);
    assert!(diagnostics.0.lock().unwrap().is_empty());
}
