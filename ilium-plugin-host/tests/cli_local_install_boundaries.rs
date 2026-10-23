use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::{SystemTime, Duration}};
use ilium_plugin_host::{cli::{CliEnv, local_install::factory}, audit::{AuditActor, AuditContext}, install::{Consent, ConsentDecision, ConsentRequest}, registry_client::{Fetcher, FetchError}, package::{read_package, write_package}};
struct NoNetwork;
impl Fetcher for NoNetwork { fn get(&self, _: &str) -> Result<Vec<u8>, FetchError> { panic!("local install must not fetch") } }
struct Answer { calls: AtomicUsize, approve: bool }
impl Consent for Answer { fn decide(&self, request: &ConsentRequest) -> ConsentDecision {
 self.calls.fetch_add(1, Ordering::SeqCst);
 if self.approve { ConsentDecision::Approve(request.requested.clone()) } else { ConsentDecision::Deny }
} }
fn context() -> AuditContext { AuditContext { session:"local-install-test".into(), actor:AuditActor { kind:"cli".into(), pane:None, plugin:None } } }
fn environment(root: &std::path::Path) -> CliEnv {
 CliEnv::new(root.join("data"), root.join("state"), root.join("config"), root.into(), SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_700_000), false, Arc::new(NoNetwork))
}
fn vector() -> Vec<u8> {
 let hex: String = include_str!("package/golden/minimal_unsigned.hex").chars().filter(|c| !c.is_whitespace()).collect();
 (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i+2],16).unwrap()).collect()
}
fn files() -> Vec<(String, Vec<u8>)> { read_package(&vector()).unwrap().entries.into_iter().map(|entry| (entry.path,entry.bytes)).collect() }
const POLICY_COMPONENT: &str = r#"(component

  (core module $activation_m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (func (export "activate") (param i32 i32 i32 i32 i64 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
      local.get 4 i64.const 1 i64.ne if unreachable end
      i32.const 0))
  (core instance $activation_m (instantiate $activation_m))
  (type $activation (record
    (field "plugin" string) (field "api" string) (field "instance" u64)
    (field "side" string) (field "settings-json" string)
    (field "granted" (list string)) (field "restored" (option (list u8)))))
  (type $code (enum "not-permitted" "not-found" "invalid-argument" "conflict" "quota-exceeded" "rate-limited" "busy" "timed-out" "cancelled" "unsupported" "pane-not-ready" "revoked" "io" "network" "internal"))
  (type $error (record (field "code" $code) (field "message" string) (field "retry-after-ms" (option u32))))
  (func $activate (param "context" $activation) (result (result (error $error)))
    (canon lift (core func $activation_m "activate") (memory $activation_m "memory") (realloc (func $activation_m "realloc"))))
  (instance $lifecycle
    (export "activation" (type $activation)) (export "error-code" (type $code)) (export "plugin-error" (type $error))
    (export "activate" (func $activate)))
  (export "ilium:exports/lifecycle@0.1.0" (instance $lifecycle))
  (core module $handler_m
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      global.get $heap
      global.get $heap local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $heap)
    (data (i32.const 0) "\20\00\00\00\01\00\00\00")
    (data (i32.const 40) "\0d\00\00\00\00\00\00\00\2a\00\00\00\00\00\00\00")
    (func (export "events") (param i32 i32) (result i32)
      local.get 1 i32.const 1 i32.ne if unreachable end
      i32.const 0))
  (core instance $handler_m (instantiate $handler_m))
  (type $event (variant (case "node-removed" u64)))
  (type $intent (variant (case "close-node" u64)))
  (type $submission (record (field "intent" $intent) (field "correlation" u64) (field "dedup-key" (option string))))
  (func $events (param "batch" (list $event)) (result (list $submission))
    (canon lift (core func $handler_m "events") (memory $handler_m "memory") (realloc (func $handler_m "realloc"))))
  (instance $handler (export "event" (type $event)) (export "intent" (type $intent)) (export "submission" (type $submission)) (export "on-events" (func $events)))
  (export "ilium:exports/handler@0.1.0" (instance $handler))
)"#;
fn policy_files() -> Vec<(String, Vec<u8>)> {
    use sha2::{Digest, Sha256};
    let mut entries = files();
    let component = entries.iter_mut().find(|(path, _)| path == "server.wasm").unwrap();
    let api_section = component.1[8..].to_vec();
    component.1 = wat::parse_str(POLICY_COMPONENT).unwrap();
    // Preserve the actual API-version custom section from the normative vector.
    component.1.extend_from_slice(&api_section);
    let length = component.1.len();
    let digest = format!("{:x}", Sha256::digest(&component.1));
    let manifest = entries.iter_mut().find(|(path, _)| path == "manifest.json").unwrap();
    let mut document: serde_json::Value = serde_json::from_slice(&manifest.1).unwrap();
    let inventory = document["files"].as_array_mut().unwrap().iter_mut()
        .find(|row| row["path"] == "server.wasm").unwrap();
    inventory["length"] = serde_json::json!(length);
    inventory["sha256"] = serde_json::json!(digest);
    manifest.1 = serde_json::to_vec(&document).unwrap();
    entries
}

fn signed_package(root: &std::path::Path) -> std::path::PathBuf {
 use base64::Engine as _;
 use sha2::{Digest, Sha256};
 use ilium_plugin_host::trust::{TrustDocument,TrustEntry,AddedBy,Status};
 let public=ed25519_dalek::SigningKey::from_bytes(&[19;32]).verifying_key().to_bytes();
 let entry=TrustEntry { key_id:format!("{:x}",Sha256::digest(public)),publisher_id:"example".into(), publisher:"Example Labs".into(),public_key:base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public),added:"2026-10-22 12:00:00".into(),added_by:AddedBy::User,status:Status::Trusted,revoked:None };
 std::fs::create_dir_all(root.join("state")).unwrap();
 std::fs::write(root.join("state/trust.json"),serde_json::to_vec(&TrustDocument {keys:vec![entry],..TrustDocument::default()}).unwrap()).unwrap();
 let bytes=write_package(&policy_files()).unwrap();
 let signed=ilium_plugin_host::signing::sign(&bytes,&ilium_plugin_host::signing::SigningKey::from_seed([19;32]),ilium_plugin_host::signing::SignedAt("2026-10-22 12:00:00".into())).unwrap();
 let path=root.join("signed.iliumpkg"); std::fs::write(&path,signed).unwrap(); path
}
#[test]
fn explicit_consent_installs_without_session_and_writes_verified_audit() {
 let root=tempfile::tempdir().unwrap(); let env=environment(root.path()); let path=signed_package(root.path());
 let consent=Arc::new(Answer {calls:AtomicUsize::new(0),approve:true});
 let service=factory(&env,consent.clone(),context()).unwrap();
 let output=service.install(&path);
 assert_eq!(output.exit_code,0,"{:?}",output.stdout_jsonl);
 assert_eq!(consent.calls.load(Ordering::SeqCst),1);
 assert!(env.data_dir.join("plugins/installed.json").exists());
 let audit=env.state_dir.join("audit/audit.jsonl");
 ilium_plugin_host::audit::AuditLog::open(audit.clone()).unwrap().verify().unwrap();
 assert!(std::fs::read_to_string(audit).unwrap().contains("consent-given"));
}
#[test]
fn denied_consent_never_commits_registry_or_package() {
 let root=tempfile::tempdir().unwrap(); let env=environment(root.path()); let path=signed_package(root.path());
 let consent=Arc::new(Answer {calls:AtomicUsize::new(0),approve:false});
 let output=factory(&env,consent.clone(),context()).unwrap().install(&path);
 assert_ne!(output.exit_code,0);
 assert_eq!(consent.calls.load(Ordering::SeqCst),1);
 assert!(!env.data_dir.join("plugins/store/example.minimal/0.1.0").exists());
 let registry=ilium_plugin_host::registry::InstalledRegistry::open(&env.data_dir.join("plugins/installed.json")).unwrap();
 assert!(registry.list().unwrap().is_empty());
}
#[test]
fn invalid_override_and_unverified_system_authority_are_rejected() {
 let root=tempfile::tempdir().unwrap(); let mut env=environment(root.path());
 let consent=Arc::new(Answer {calls:AtomicUsize::new(0),approve:true});
 env.env.insert("ILIUM_TRUST_STORE".into(),"relative.json".into());
 assert!(factory(&env,consent.clone(),context()).is_err());
 env.env.clear(); env.policy_file=Some(root.path().join("policy.toml"));
 assert!(factory(&env,consent.clone(),context()).is_err());
 assert_eq!(consent.calls.load(Ordering::SeqCst),0);
 assert!(!env.data_dir.exists());
}
#[test]
fn actual_untrusted_signer_is_refused_before_consent_or_commit() {
 let root=tempfile::tempdir().unwrap(); let env=environment(root.path()); let path=signed_package(root.path());
 std::fs::write(env.state_dir.join("trust.json"), br#"{"format":"ilium-trust/1","keys":[]}"#).unwrap();
 let consent=Arc::new(Answer {calls:AtomicUsize::new(0),approve:true});
 let output=factory(&env,consent.clone(),context()).unwrap().install(&path);
 assert_ne!(output.exit_code,0,"{:?}",output.stdout_jsonl);
 assert_eq!(consent.calls.load(Ordering::SeqCst),0);
 assert!(ilium_plugin_host::registry::InstalledRegistry::open(&env.data_dir.join("plugins/installed.json")).unwrap().list().unwrap().is_empty());
}
#[test]
fn unreadable_audit_destination_blocks_consent_and_commit() {
 let root=tempfile::tempdir().unwrap(); let env=environment(root.path()); let path=signed_package(root.path());
 std::fs::write(env.state_dir.join("audit"), b"regular file cannot hold audit log").unwrap();
 let consent=Arc::new(Answer {calls:AtomicUsize::new(0),approve:true});
 let output=factory(&env,consent.clone(),context()).unwrap().install(&path);
 assert_ne!(output.exit_code,0);
 assert_eq!(consent.calls.load(Ordering::SeqCst),0);
 assert!(ilium_plugin_host::registry::InstalledRegistry::open(&env.data_dir.join("plugins/installed.json")).unwrap().list().unwrap().is_empty());
}
