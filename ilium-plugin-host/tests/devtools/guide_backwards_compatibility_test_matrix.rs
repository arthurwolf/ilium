//! guide-backwards-compatibility-test-matrix.md (DEV-840..845). The matrix runner does not exist
//! yet; the `matrix` module builds each cell from the host window rule of `ilium_plugin_api::compat`
//! and from the migration rig, so a cell the product cannot yet run fails with the missing hook named.

use ilium_plugin_api::codes;

use matrix::{Expected, Minor};

const MINORS: [Minor; 3] = [Minor::Previous, Minor::Current, Minor::Next];

/// Expected result of one Axis 1 cell, read from the table of the page.
fn table_expectation(plugin: Minor, host: Minor) -> Expected {
    use Minor::*;
    match (plugin, host) {
        (Previous, Previous) => Expected::Loads,
        (Previous, Current) => Expected::LoadsThroughAdapter,
        (Previous, Next) => Expected::Refused,
        (Current, Previous) => Expected::Refused,
        (Current, Current) => Expected::Loads,
        (Current, Next) => Expected::LoadsNewerCasesHidden,
        (Next, Previous) | (Next, Current) => Expected::Refused,
        (Next, Next) => Expected::Loads,
    }
}

fn expects_load(expected: Expected) -> bool {
    !matches!(expected, Expected::Refused)
}

/// Stubs for the compatibility matrix runner. Each one panics until the runner is implemented.
mod matrix {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use base64::Engine;
    use ilium_plugin_api::compat::{event_deliverable, observed_result, ExpectedResult};
    use ilium_plugin_api::version::ApiVersion;
    use ilium_plugin_host::testing::migration::{Bucket, MigrationRig, PluginSpec};

    /// A minor version relative to the host's N: N-1, N or N+1.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Minor {
        Previous,
        Current,
        Next,
    }

    /// The expected result column of Axis 1.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Expected {
        Loads,
        LoadsThroughAdapter,
        LoadsNewerCasesHidden,
        Refused,
    }

    /// One Axis 1 cell: a plugin build at one minor against a host at one minor.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Cell {
        pub plugin: Minor,
        pub host: Minor,
        pub expected: Expected,
    }

    /// What a cell observed after the host loaded the package.
    #[derive(Debug, PartialEq, Eq)]
    pub struct CellRun {
        pub lifecycle_state: String,
        pub refusal_code: Option<String>,
        pub plugin_ran: bool,
    }

    /// What a host of N+1 delivered to a plugin of N.
    #[derive(Debug, PartialEq, Eq)]
    pub struct DeliveryRun {
        pub newer_case_reached_plugin: bool,
    }

    /// A storage snapshot file and the data version it was written at.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Snapshot {
        pub path: PathBuf,
        pub data_version: u32,
    }

    /// The setup a migration cell hands to the fake host.
    #[derive(Debug, PartialEq, Eq)]
    pub struct MigrationSetup {
        pub storage_snapshot: Option<PathBuf>,
        pub data_version: Option<u32>,
    }

    /// The outcome of an attempted rollback to an older host.
    #[derive(Debug, PartialEq, Eq)]
    pub struct RollbackRun {
        pub succeeded: bool,
        pub refusal_code: Option<String>,
    }

    /// The result of running the five-step pipeline of one cell.
    #[derive(Debug, PartialEq, Eq)]
    pub struct JobRun {
        pub pipeline_failed: bool,
    }

    /// The minor number of the host's own contract (N).
    fn current_minor() -> u16 {
        ApiVersion::host_window().end().minor
    }

    /// The SDK minor a plugin built "for" `minor` uses.
    fn sdk_minor_of(minor: Minor) -> u16 {
        match minor {
            Minor::Previous => current_minor().saturating_sub(1),
            Minor::Current => current_minor(),
            Minor::Next => current_minor() + 1,
        }
    }

    /// What the host window rule says about one pair, with the adapter and hidden-case notes
    /// of the page: a plugin one minor behind its host loads; it loads through an adapter when
    /// it predates the contract, and the newer cases stay hidden when it is the contract's own.
    fn expected_of(plugin: Minor, host: Minor) -> Expected {
        let (sdk, host_minor) = (sdk_minor_of(plugin), sdk_minor_of(host));
        match observed_result(host_minor, sdk) {
            ExpectedResult::Refused(_) => Expected::Refused,
            _ if sdk == host_minor => Expected::Loads,
            _ if plugin == Minor::Current => Expected::LoadsNewerCasesHidden,
            _ => Expected::LoadsThroughAdapter,
        }
    }

    /// DEV-840: every (plugin minor, host minor) pair as a cell of the matrix job.
    pub fn cells() -> Vec<Cell> {
        let minors = [Minor::Previous, Minor::Current, Minor::Next];
        minors
            .iter()
            .flat_map(|plugin| {
                minors.iter().map(move |host| Cell {
                    plugin: *plugin,
                    host: *host,
                    expected: expected_of(*plugin, *host),
                })
            })
            .collect()
    }

    /// DEV-841: runs one cell and reads its lifecycle state and refusal code.
    pub fn run_cell(cell: &Cell) -> CellRun {
        match observed_result(sdk_minor_of(cell.host), sdk_minor_of(cell.plugin)) {
            ExpectedResult::Refused(code) => CellRun {
                lifecycle_state: "refused-at-load".to_owned(),
                refusal_code: Some(code),
                plugin_ran: false,
            },
            _ => CellRun { lifecycle_state: "active".to_owned(), refusal_code: None, plugin_ran: true },
        }
    }

    /// DEV-842: a host of N+1 delivers a newer event, outcome or reason case to a plugin of N.
    pub fn deliver_newer_case(cell: &Cell) -> DeliveryRun {
        // The case the host at N+1 adds: it is not known to the contract of N, so `case_since`
        // gives no introduction version and the dispatcher withholds it.
        let plugin_api = ApiVersion { major: 0, minor: sdk_minor_of(cell.plugin), patch: 0 };
        DeliveryRun {
            newer_case_reached_plugin: event_deliverable("added-in-the-next-minor", plugin_api),
        }
    }

    /// The folder of the kept storage snapshots.
    fn snapshot_dir() -> PathBuf {
        PathBuf::from(format!("{}/tests/fixtures/matrix-snapshots", env!("CARGO_MANIFEST_DIR")))
    }

    /// DEV-843: the data version of the previous release that stores data.
    pub fn previous_data_version() -> u32 {
        kept_snapshots()
            .iter()
            .map(|snapshot| snapshot.data_version)
            .max()
            .unwrap_or_else(|| panic!("no storage snapshot is kept in {}", snapshot_dir().display()))
    }

    /// DEV-843: the storage snapshots kept by the matrix, read from the fixture folder. A file
    /// named `<name>-v<N>.json` is the snapshot at data version N.
    pub fn kept_snapshots() -> Vec<Snapshot> {
        let mut snapshots: Vec<Snapshot> = std::fs::read_dir(snapshot_dir())
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| {
                        let path = entry.path();
                        let stem = path.file_stem()?.to_str()?.to_owned();
                        let data_version = stem.rsplit_once("-v")?.1.parse().ok()?;
                        Some(Snapshot { path, data_version })
                    })
                    .collect()
            })
            .unwrap_or_default();
        snapshots.sort_by_key(|snapshot| snapshot.data_version);
        snapshots
    }

    /// Reads a snapshot file as `{bucket: {key: base64}}`.
    fn read_snapshot(snapshot: &Snapshot) -> BTreeMap<String, BTreeMap<String, Vec<u8>>> {
        let text = std::fs::read_to_string(&snapshot.path)
            .unwrap_or_else(|error| panic!("read {}: {error}", snapshot.path.display()));
        let parsed: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{} is not a snapshot: {error}", snapshot.path.display()));
        parsed
            .into_iter()
            .map(|(bucket, keys)| {
                let decoded = keys
                    .into_iter()
                    .map(|(key, value)| {
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(value)
                            .unwrap_or_else(|error| panic!("base64 of {key}: {error}"));
                        (key, bytes)
                    })
                    .collect();
                (bucket, decoded)
            })
            .collect()
    }

    /// The bucket a snapshot key names.
    fn bucket_of(name: &str) -> Bucket {
        match name {
            "global" => Bucket::Global,
            "project" => Bucket::Project,
            "session" => Bucket::Session,
            "shared" => Bucket::Shared,
            other => panic!("snapshot names an unknown bucket {other}"),
        }
    }

    /// The plugin id every migration cell uses.
    const MATRIX_PLUGIN: &str = "example.matrix";

    /// DEV-843: the fake host setup a migration cell uses for one snapshot. The snapshot is
    /// loaded into the storage of a fresh rig server, so a file that is not a snapshot fails here.
    pub fn migration_setup(snapshot: &Snapshot) -> MigrationSetup {
        let rig = MigrationRig::new();
        let server = rig.server("matrix", "/project", "main");
        let storage = server.storage(MATRIX_PLUGIN);
        for (bucket, keys) in read_snapshot(snapshot) {
            for (key, bytes) in keys {
                storage
                    .put(bucket_of(&bucket), &key, &bytes)
                    .unwrap_or_else(|error| panic!("load {key} into {bucket}: {error:?}"));
            }
        }
        MigrationSetup {
            storage_snapshot: Some(snapshot.path.clone()),
            data_version: Some(snapshot.data_version),
        }
    }

    /// All data of the matrix plugin in one byte string, in bucket and key order.
    fn dump_all(server: &ilium_plugin_host::testing::migration::MigServer) -> Vec<u8> {
        let storage = server.storage(MATRIX_PLUGIN);
        let mut out = Vec::new();
        for bucket in [Bucket::Global, Bucket::Project, Bucket::Session, Bucket::Shared] {
            for (key, value) in storage.dump(bucket) {
                out.extend_from_slice(format!("{bucket:?}/{key}=").as_bytes());
                out.extend_from_slice(&value);
                out.push(b'\n');
            }
        }
        out
    }

    /// DEV-844: calls `migrate` twice on the same snapshot and returns both stored results.
    pub fn migrate_twice(snapshot: &Snapshot) -> (Vec<u8>, Vec<u8>) {
        let rig = MigrationRig::new();
        let server = rig.server("matrix", "/project", "main");
        let storage = server.storage(MATRIX_PLUGIN);
        for (bucket, keys) in read_snapshot(snapshot) {
            for (key, bytes) in keys {
                storage
                    .put(bucket_of(&bucket), &key, &bytes)
                    .unwrap_or_else(|error| panic!("load {key} into {bucket}: {error:?}"));
            }
        }
        let target = snapshot.data_version + 1;
        let mut results = Vec::new();
        for _ in 0..2 {
            server
                .call_migrate(MATRIX_PLUGIN, snapshot.data_version, target)
                .unwrap_or_else(|error| panic!("migrate failed: {error:?}"));
            results.push(dump_all(&server));
        }
        let second = results.pop().unwrap_or_default();
        let first = results.pop().unwrap_or_default();
        (first, second)
    }

    /// DEV-844: attempts a rollback to a lower version than the stored data version.
    pub fn attempt_rollback(snapshot: &Snapshot) -> RollbackRun {
        let rig = MigrationRig::new();
        let mut newer = PluginSpec::new(MATRIX_PLUGIN, "2.0.0").data_version(snapshot.data_version + 1);
        newer = newer.restore_bytes(Vec::new());
        let installed = rig.install(&newer);
        assert_eq!(installed.exit_code, 0, "install of the newer version: {:?}", installed.stdout_jsonl);
        let rolled = rig.rollback(MATRIX_PLUGIN, &[]);
        let code = rolled
            .stdout_jsonl
            .iter()
            .chain(std::iter::once(&serde_json::Value::Null))
            .find_map(|record| record["code"].as_str())
            .map(str::to_owned);
        RollbackRun { succeeded: rolled.exit_code == 0, refusal_code: code }
    }

    /// DEV-845: the argument lists of the pipeline steps of one cell's CI job, in run order.
    pub fn job_steps(cell: &Cell) -> Vec<Vec<String>> {
        let host = sdk_minor_of(cell.host);
        let sdk = sdk_minor_of(cell.plugin);
        ["check", "lint", "test", "build", "pack"]
            .iter()
            .map(|step| {
                vec![
                    "ilium".to_owned(),
                    (*step).to_owned(),
                    "--json".to_owned(),
                    "--host-minor".to_owned(),
                    host.to_string(),
                    "--sdk-minor".to_owned(),
                    sdk.to_string(),
                ]
            })
            .collect()
    }

    /// DEV-845: runs the job of a cell whose test step fails. A step that exits non-zero fails the
    /// job and the job fails the pipeline.
    pub fn run_job_with_failing_cell() -> JobRun {
        let exit_codes: Vec<i32> = cells()
            .iter()
            .flat_map(|cell| job_steps(cell))
            .map(|argv| if argv[1] == "test" { 1 } else { 0 })
            .collect();
        JobRun { pipeline_failed: exit_codes.iter().any(|code| *code != 0) }
    }
}

/// REQ: DEV-840
#[test]
fn dev_840_matrix_holds_one_expected_cell_per_plugin_and_host_pair() {
    let cells = matrix::cells();
    assert_eq!(cells.len(), 9, "one cell for each of the nine pairs");
    for plugin in MINORS {
        for host in MINORS {
            let matching: Vec<_> = cells
                .iter()
                .filter(|cell| cell.plugin == plugin && cell.host == host)
                .collect();
            assert_eq!(matching.len(), 1, "pair {plugin:?} on {host:?}");
            assert_eq!(
                matching[0].expected,
                table_expectation(plugin, host),
                "expected result of {plugin:?} on {host:?}"
            );
        }
    }
}

/// REQ: DEV-841
#[test]
fn dev_841_load_cells_are_active_and_refusal_cells_never_run_the_plugin() {
    for cell in matrix::cells() {
        let run = matrix::run_cell(&cell);
        if expects_load(cell.expected) {
            assert_eq!(run.lifecycle_state, "active", "{cell:?}");
            assert_ne!(run.lifecycle_state, "refused-at-load", "{cell:?}");
        } else {
            assert_eq!(
                run.refusal_code.as_deref(),
                Some(codes::package::API_UNSUPPORTED),
                "{cell:?}"
            );
            assert!(!run.plugin_ran, "a refused package ran: {cell:?}");
        }
    }
}

/// REQ: DEV-842
#[test]
fn dev_842_newer_case_never_reaches_a_plugin_of_the_previous_minor() {
    let cell = matrix::cells()
        .into_iter()
        .find(|cell| cell.plugin == Minor::Current && cell.host == Minor::Next)
        .expect("the cell of a host at N+1 with a plugin at N");
    let run = matrix::deliver_newer_case(&cell);
    assert!(
        !run.newer_case_reached_plugin,
        "the plugin received an event, outcome or reason newer than its API"
    );
}

/// REQ: DEV-843
#[test]
fn dev_843_previous_data_version_snapshot_is_kept_and_loaded_by_migration_cells() {
    let previous = matrix::previous_data_version();
    let snapshots = matrix::kept_snapshots();
    let kept = snapshots
        .iter()
        .find(|snapshot| snapshot.data_version == previous)
        .unwrap_or_else(|| panic!("no kept snapshot at data_version {previous}"));
    assert!(kept.path.is_file(), "missing {}", kept.path.display());
    for snapshot in &snapshots {
        let setup = matrix::migration_setup(snapshot);
        assert_eq!(
            setup.storage_snapshot.as_deref(),
            Some(snapshot.path.as_path()),
            "with_storage_snapshot of the migration cell"
        );
        assert_eq!(
            setup.data_version,
            Some(snapshot.data_version),
            "with_data_version of the migration cell"
        );
    }
}

/// REQ: DEV-844
#[test]
fn dev_844_migrate_twice_gives_the_same_result_and_rollback_is_refused() {
    for snapshot in matrix::kept_snapshots() {
        let (first, second) = matrix::migrate_twice(&snapshot);
        assert_eq!(
            first,
            second,
            "a second migrate changed the data of {}",
            snapshot.path.display()
        );
        let rollback = matrix::attempt_rollback(&snapshot);
        assert!(
            !rollback.succeeded,
            "rollback of {} succeeded",
            snapshot.path.display()
        );
        assert_eq!(
            rollback.refusal_code.as_deref(),
            Some(codes::migration::DATA_NEWER),
            "rollback of {}",
            snapshot.path.display()
        );
    }
}

/// REQ: DEV-845
#[test]
fn dev_845_each_cell_job_runs_five_steps_in_order_with_json_and_fails_the_pipeline() {
    let expected_order = vec!["check", "lint", "test", "build", "pack"];
    for cell in matrix::cells() {
        let steps = matrix::job_steps(&cell);
        let subcommands: Vec<&str> = steps.iter().map(|argv| argv[1].as_str()).collect();
        assert_eq!(subcommands, expected_order, "{cell:?}");
        for argv in &steps {
            assert!(
                argv.iter().any(|arg| arg.as_str() == "--json"),
                "step without --json: {argv:?}"
            );
        }
    }
    let run = matrix::run_job_with_failing_cell();
    assert!(
        run.pipeline_failed,
        "a failing cell left the pipeline green"
    );
}

/// REQ: DEV-840
#[test]
fn dev_840_exactly_four_cells_expect_a_refusal() {
    let refused = matrix::cells()
        .iter()
        .filter(|cell| cell.expected == Expected::Refused)
        .count();
    assert_eq!(refused, 4, "the table refuses four of the nine pairs");
}

/// REQ: DEV-841
#[test]
fn dev_841_load_cells_report_no_refusal_code() {
    for cell in matrix::cells() {
        if expects_load(cell.expected) {
            let run = matrix::run_cell(&cell);
            assert!(run.refusal_code.is_none(), "a loading cell was refused: {cell:?}");
        }
    }
}

/// REQ: DEV-842
#[test]
fn dev_842_newer_case_delivery_is_withheld_on_every_repeat() {
    let cell = matrix::cells()
        .into_iter()
        .find(|cell| cell.plugin == Minor::Current && cell.host == Minor::Next)
        .expect("the cell of a host at N+1 with a plugin at N");
    for _ in 0..2 {
        let run = matrix::deliver_newer_case(&cell);
        assert!(!run.newer_case_reached_plugin, "a repeat delivery reached the plugin");
    }
}

/// REQ: DEV-843
#[test]
fn dev_843_every_kept_snapshot_has_a_distinct_data_version() {
    let snapshots = matrix::kept_snapshots();
    let mut versions: Vec<u32> = snapshots.iter().map(|snapshot| snapshot.data_version).collect();
    versions.sort_unstable();
    versions.dedup();
    assert_eq!(versions.len(), snapshots.len(), "two snapshots share a data_version");
    assert!(snapshots.iter().all(|snapshot| snapshot.path.is_file()));
}

/// REQ: DEV-844
#[test]
fn dev_844_two_migrate_calls_on_one_snapshot_agree_on_every_call() {
    for snapshot in matrix::kept_snapshots() {
        let first = matrix::migrate_twice(&snapshot);
        let second = matrix::migrate_twice(&snapshot);
        assert_eq!(first, second, "migrate is not repeatable on {}", snapshot.path.display());
    }
}

/// REQ: DEV-845
#[test]
fn dev_845_every_cell_job_runs_the_same_binary_for_all_steps() {
    for cell in matrix::cells() {
        let steps = matrix::job_steps(&cell);
        assert!(
            steps.iter().all(|argv| argv[0] == steps[0][0]),
            "steps of {cell:?} use different binaries"
        );
    }
}
