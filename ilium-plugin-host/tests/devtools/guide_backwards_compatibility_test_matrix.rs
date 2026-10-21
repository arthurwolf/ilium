//! guide-backwards-compatibility-test-matrix.md (DEV-840..845). The matrix runner does not exist
//! yet, so the `matrix` module holds stubs that panic at run time until it is built.

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
    use std::path::PathBuf;

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

    /// DEV-840: every (plugin minor, host minor) pair as a cell of the matrix job.
    pub fn cells() -> Vec<Cell> {
        unimplemented!("ilium-plugin-host::matrix::cells")
    }

    /// DEV-841: runs one cell and reads its lifecycle state and refusal code.
    pub fn run_cell(cell: &Cell) -> CellRun {
        let _ = cell;
        unimplemented!("ilium-plugin-host::matrix::run_cell")
    }

    /// DEV-842: a host of N+1 delivers a newer event, outcome or reason case to a plugin of N.
    pub fn deliver_newer_case(cell: &Cell) -> DeliveryRun {
        let _ = cell;
        unimplemented!("ilium-plugin-host::matrix::deliver_newer_case")
    }

    /// DEV-843: the data version of the previous release that stores data.
    pub fn previous_data_version() -> u32 {
        unimplemented!("ilium-plugin-host::matrix::previous_data_version")
    }

    /// DEV-843: the storage snapshots kept by the matrix, read from the fixture folder.
    pub fn kept_snapshots() -> Vec<Snapshot> {
        unimplemented!("ilium-plugin-host::matrix::kept_snapshots")
    }

    /// DEV-843: the fake host setup a migration cell uses for one snapshot.
    pub fn migration_setup(snapshot: &Snapshot) -> MigrationSetup {
        let _ = snapshot;
        unimplemented!("ilium-plugin-host::matrix::migration_setup")
    }

    /// DEV-844: calls `migrate` twice on the same snapshot and returns both stored results.
    pub fn migrate_twice(snapshot: &Snapshot) -> (Vec<u8>, Vec<u8>) {
        let _ = snapshot;
        unimplemented!("ilium-plugin-host::matrix::migrate_twice")
    }

    /// DEV-844: attempts a rollback to a lower version than the stored data version.
    pub fn attempt_rollback(snapshot: &Snapshot) -> RollbackRun {
        let _ = snapshot;
        unimplemented!("ilium-plugin-host::matrix::attempt_rollback")
    }

    /// DEV-845: the argument lists of the pipeline steps of one cell's CI job, in run order.
    pub fn job_steps(cell: &Cell) -> Vec<Vec<String>> {
        let _ = cell;
        unimplemented!("ilium-plugin-host::matrix::job_steps")
    }

    /// DEV-845: runs the job of a cell whose test step fails.
    pub fn run_job_with_failing_cell() -> JobRun {
        unimplemented!("ilium-plugin-host::matrix::run_job_with_failing_cell")
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
    assert!(run.pipeline_failed, "a failing cell left the pipeline green");
}
