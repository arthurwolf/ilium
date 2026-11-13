# API gaps for lane lifecycle (part 1)

All items are crate `ilium-plugin-host` unless stated. The module is `testing::life`, behind feature `testing`, and its items are re-exported from `testing`.

- Dev-dependency `serde_json = "1"` in ilium-plugin-host/Cargo.toml (all LIF cli/JSON tests): the tests read `CliOutput.stdout_jsonl` values and `installed.json`.
- `ilium_plugin_host::testing::life::PluginSpec` builder: `new(&str)`, `tier(Tier)`, `sides(&[Side])`, `startup()`, `requires_plugin(&str)`, `requires_service(&str)`, `provides_service(&str)`, `owns_point(&str)`, `contributes_to(&str)`, `memory_mib(u64)`, `activate_delay_ms(u64)` (LIF-045..047, 055: declares one test plugin without building a package).
- `LifeHost::new() -> LifeHost`, `with_pool_mib(u64) -> LifeHost` (LIF-011, 034, 141, 142): a multi-plugin, multi-client host with a manual monotonic clock around a real `LifecycleManager`.
- `LifeHost::install(&mut self, PluginSpec)`, `install_refused(&mut self, PluginSpec, code: &str)`, `install_policy_blocked(&mut self, PluginSpec)` (LIF-003, 007).
- `LifeHost::start(&mut self, StartMode) -> StartReport`, `shutdown(&mut self)`, `restart_server(&mut self)`, `crash_server(&mut self)` (LIF-008, 037, 038).
- `LifeHost::start_safe(&mut self, SafeMode, flag: Option<&str>, env: Option<&str>)` with `SafeMode {Off, ThirdParty, AllOptional}` and `LifeHost::safe_mode(&self) -> SafeMode` (LIF-180, 181).
- `LifeHost::attach_client(&mut self, &str) -> String` (returns the scope), `detach_client(&mut self, &str)`, `kill_client(&mut self, &str)` (LIF-015, 025, 026).
- `LifeHost::advance_ms(&mut self, u64)`, `unix_ms(&self) -> u64` (monotonic manual clock; all timing tests).
- `FailureKind` enum: `Trap, ActivateError, ActivateDeadline, ExportDeadline, DeactivateDeadline(DeactivationReason), EventQueueOverflow, MemoryLimit, ServiceProviderActivateFails, JobTrap, ExtensionHandlerTimeout, HelperCrashRunning, HelperCrashOther` and `LifeHost::fail(&mut self, plugin: &str, side: Side, scope: &str, FailureKind)` (LIF-033, 035).
- `LifeHost::side(&self, plugin: &str, side: Side, scope: &str) -> SideState` and `package_state(&self, plugin: &str) -> PackageState` (LIF-001, 002).
- `LifeHost::activations(&self) -> Vec<ActivationRecord { plugin, side, scope, begin_ms, end_ms, begin_seq, end_seq }>` (LIF-032, 045..047).
- `LifeHost::deactivations(&self, plugin: &str) -> Vec<DeactivationReason>` and `deactivation_order(&self) -> Vec<String>` (LIF-012, 014, 015, 043, 044, 050).
- `LifeHost::state_changes(&self) -> Vec<StateChange { plugin, side, scope, from, to, reason, at_unix_ms }>` (LIF-016). The generated `Event` enum lacks the `plugin-state-changed` and `service-availability-changed` variants, so the record type stands in for them.
- `LifeHost::service_events(&self, consumer: &str) -> Vec<AvailabilityChange { interface, available, provider }>` (LIF-053).
- `LifeHost::call_service(&mut self, consumer: &str, interface: &str) -> Result<(), PluginError>` (LIF-052).
- `LifeHost::installed_json(&self) -> serde_json::Value` (LIF-004, 008, 037, 184).
- `LifeHost::cli(&mut self, &[&str]) -> CliOutput`, `LifeHost::plugins_api(&mut self) -> &mut dyn HostPlugins`, `reset_failures(&mut self, plugin: &str)`, `update(&mut self, plugin: &str, version: &str)`, `update_candidate_fails(&mut self, plugin: &str, version: &str)`, `revoke(&mut self, plugin: &str, grant: &str)`, `uninstall(&mut self, plugin: &str)`, `granted_of_last_activation(&self, plugin: &str) -> Vec<String>`, `active_version(&self, plugin: &str, side: Side) -> String` (LIF-009, 010, 013, 036, 040, 041).
- `LifeHost::log(&self) -> Vec<LifeLogLine { plugin, side, scope, code, message }>` (LIF-038, 056).
- `LifeHost::queued_events(&self, plugin: &str, side: Side) -> usize`, `dropped_events(&self, plugin: &str, side: Side) -> usize` (LIF-006).
- `LifeHost::banner(&self) -> Vec<String>` essential-failure banner lines (LIF-042).
- `LifeHost::near_miss(&mut self, plugin: &str, count: u32)`, `fill_event_queue(&mut self, plugin: &str, fraction_permille: u32)`, `rate_limited(&mut self, plugin: &str)`, `quota_exceeded(&mut self, plugin: &str)`, `health(&self, plugin: &str) -> HealthState` (LIF-122..124).
- `LifeHost::run_call(&mut self, plugin: &str, deadline_ms: u64, work_ms: u64, sync_host_ms: u64, async_wait_ms: u64) -> CallResult { trapped: bool, elapsed_ms: u64 }` (LIF-125, 126, 129).
- `PluginEngine::fuel_enabled(&self) -> bool`, `PluginEngine::deadline_ticks(&self, deadline_ms: u64) -> u64` (LIF-127, 128; same names as the runtime lane).
- `LifeHost::pending_requests(&mut self, plugin: &str, count: u32)`, `request_outcomes(&self, plugin: &str) -> Vec<Outcome>`, `request_never_answered(&mut self, plugin: &str, timeout_ms: u64) -> RequestId` (LIF-130, 131).
- `LifeHost::helper_hang(&mut self, missed_heartbeats: u32, past_deadline_ms: u64) -> Option<u64>` (kill delay), `helper_crash(&mut self, running: &str, hosted: &[&str])`, `helper_restarts(&self) -> u32` (LIF-132..134).
- `LifeHost::metric_buckets(&self, plugin: &str, side: Side) -> usize` (LIF-135, 136).
- `LifeHost::diagnostics_report(&self) -> String` (LIF-138).
- `LifeHost::set_user_limit(&mut self, plugin: Option<&str>, key: LimitKey, value: u64)`, `effective_limit(&self, plugin: &str, key: LimitKey) -> (u64, LimitSource)` with `LimitSource {Default, Manifest, User, Ceiling, Fixed}`, `LifeHost::bucket_refusal(&mut self, plugin: &str, balance_ms: i64) -> PluginError`, `cgroup_memory_max_mib(&self) -> u64` (LIF-139, 143..145).
- `LifeHost::footer(&self) -> String`, `running_plugins(&self) -> Vec<String>` (LIF-182, 183).
- `ilium_plugin_host::lifecycle::LifecycleManager` has no public constructor (LIF-005, 019): `LifecycleManager::new(registry, kernel, clock)`; the tests reach it through `LifeHost::manager()`.
- `DeactivationReason`: tests need `Hash` and `Ord` derives on the generated enum (LIF-043).
- `LifeHost::deliver_event(&mut self, plugin: &str, side: Side, scope: &str, count: u32)` and `export_calls(&self, plugin: &str, side: Side) -> usize` (LIF-006).
- `LifeHost::force_package_state(&mut self, plugin: &str, PackageState)` (LIF-018: parametrised over the package states).
- `LifeHost::client_deactivations(&self, plugin: &str, client: &str) -> Vec<DeactivationReason>` (LIF-015).
- `LifeHost::race_disable_and_uninstall(&mut self, plugin: &str)`, `begin_disable(&mut self, plugin: &str) -> PendingTransition` with `PendingTransition::finished(&self) -> bool`, and `PluginSpec::deactivate_delay_ms(u64)` (LIF-020).
- `ilium_plugin_host::testing::life::SafeMode` is re-exported by `support` (LIF-004, 180).
- `PluginSpec::grants(&[&str])`, `requires_optional_plugin(&str)`, `requires_host(&str)` (LIF-041, 045, 049).
- `LifeHost::trigger_activation(&mut self, plugin: &str)`, `capacity_retries(&self, plugin: &str) -> u32` (retry attempts so far), `recovery_page_enable(&mut self, plugin: &str)` (LIF-034, 036).
- `LifeHost::begin_shutdown(&mut self)` (a stop that has begun but not finished) (LIF-038).
- `LifeHost::all_deactivations(&self, plugin: &str) -> Vec<DeactivationReason>` across every side and scope (LIF-044).
- `LifeHost::start_order(&self) -> Vec<String>` (plugin ids in activation begin order), `peak_concurrent_activations(&self, Side) -> usize`, `peak_concurrent_deactivations(&self) -> usize` (LIF-045..047, 050).
- `LifeHost::try_install(&mut self, PluginSpec) -> Result<(), Diagnostic>` and `try_install_manifest_key(&mut self, plugin: &str, key: &str, json: &str) -> Result<(), Diagnostic>` (LIF-048, 049; `Diagnostic` from `ilium_plugin_api`).
- `LifeHost::call_hook(&mut self, owner: &str, point: &str) -> HookResult { passes: u32 }` (LIF-056).
- `LifeHost::update_server_only(&mut self, plugin: &str, version: &str)`, `client_version(&self, plugin: &str, scope: &str) -> String`, `send_change_notice(&mut self, plugin: &str)`, `message_between_sides(&mut self, plugin: &str, scope: &str) -> Result<(), PluginError>` (LIF-057).
- `LifeHost::connect_client_with_api(&mut self, name: &str, api: &str) -> Result<String, PluginError>` (LIF-058).
- `LifeHost::first_frame_ms(&self, client: &str) -> u64`, `first_frame_marks(&self, client: &str) -> Vec<String>`, `essential_active(&self, client: &str) -> bool` (LIF-059).
- `PluginEngine::fuel_enabled` and `deadline_ticks` as above; `EngineConfig` is built by the tests directly, and `PluginEngine::new` is expected to accept it (LIF-127, 128).
- `LifeHost::export_calls_after_drop(&self, plugin: &str) -> usize` (LIF-130).
- `ilium_plugin_host::testing::life::LimitSource` enum `{Default, Manifest, User, Ceiling, Fixed}` deriving `Debug, PartialEq` (LIF-143).
- `LifeHost::drain_bucket(&mut self, plugin: &str)`, `bucket_balance_ms(&self, plugin: &str) -> i64`, `submit_payload(&mut self, plugin: &str, bytes: u64)`, `session_pool_used_mib(&self) -> u64` (LIF-139, 145).
- `PluginSpec::replaces(&str)` (LIF-180).
- `LifeHost::profile_snapshot(&self) -> Vec<(String, Vec<u8>)>` all files under the data and config dirs (LIF-180).
- `SafeMode` deriving `Debug, Clone, Copy, PartialEq` (LIF-181).

## Part 2

- `ilium-plugin-host` `testing::life::LifeHost::crash_during_start(&mut self)` (LIF-186): writes one crash-during-start mark at the manual clock's current time.
- `ilium-plugin-host` `testing::life::LifeHost::start_after_crashes(&mut self)` (LIF-186): starts the host through the real crash-loop guard so that `safe_mode()` reflects the chosen mode.
- `LifeHost::running_plugins(&self) -> Vec<String>` is already used by part 1 (LIF-183, 184) and is assumed here too (LIF-185).
