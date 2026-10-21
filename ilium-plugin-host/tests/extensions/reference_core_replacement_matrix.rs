//! reference-core-replacement-matrix.md: the surface every replacement of a core plugin must
//! provide (DEV-846), the stored data it must leave alone (DEV-847), and the matrix rows that must
//! match the core plugin pages (DEV-849). The CLI names (DEV-848) are tested in
//! ilium/tests/plugin_user_flows/reference_core_replacement_matrix.rs.

use std::collections::BTreeSet;

use crate::support::*;

const MATRIX_PAGE: &str = "reference-core-replacement-matrix.md";
const MATRIX_HEADING: &str = "### The matrix";

/// One row of the "The matrix" table of the replacement matrix page.
struct MatrixRow {
    /// The core plugin id, for example `ilium.core.search`.
    plugin: String,
    /// The surface items as written in the row: (kind, name), for example ("points", "rank").
    surface: Vec<(String, String)>,
}

impl MatrixRow {
    /// The surface items in full form `<plugin-id>/<name>`, the form the manual uses for names.
    fn full_surface_items(&self) -> Vec<String> {
        self.surface
            .iter()
            .map(|(_, name)| format!("{}/{}", self.plugin, name))
            .collect()
    }
}

/// The id of the replacement package used for one core plugin in these tests.
fn replacement_id(core: &str) -> String {
    format!(
        "com.example.replacement-{}",
        core.trim_start_matches("ilium.core.")
    )
}

/// The first backticked token of a cell, which is the plugin id in the first column.
fn first_backticked(cell: &str) -> String {
    cell.split('`').nth(1).unwrap_or_default().to_owned()
}

/// Reads the rows of the matrix table, one per core plugin listed on the page.
fn matrix_rows() -> Vec<MatrixRow> {
    table_after(&page(MATRIX_PAGE), MATRIX_HEADING)
        .into_iter()
        .map(|cells| {
            let surface = cells[1]
                .split(',')
                .map(|item| item.trim().trim_matches('`'))
                .filter(|item| !item.is_empty())
                .filter_map(|item| {
                    item.split_once(':')
                        .map(|(kind, name)| (kind.to_owned(), name.to_owned()))
                })
                .collect();
            MatrixRow {
                plugin: first_backticked(&cells[0]),
                surface,
            }
        })
        .collect()
}

/// The text and id of every core plugin page in the manual directory.
fn core_pages() -> Vec<(String, String)> {
    let entries = match std::fs::read_dir(pages_dir()) {
        Ok(entries) => entries,
        Err(error) => panic!("cannot list the manual pages: {error}"),
    };
    let mut pages = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("core-plugin-") && name.ends_with(".md")) {
            continue;
        }
        let text = page(&name);
        let id = text
            .split("(id `")
            .nth(1)
            .and_then(|rest| rest.split('`').next())
            .unwrap_or_default()
            .to_owned();
        pages.push((id, text));
    }
    pages
}

/// REQ: DEV-846
#[test]
fn dev_846_replacement_missing_any_surface_item_fails_and_lists_it() {
    for row in matrix_rows() {
        for item in row.full_surface_items() {
            let mut world = ExtWorld::with_core_plugins();
            assert!(
                world.core_surface(&row.plugin).contains(&item),
                "{}: the core surface lacks {item}",
                row.plugin
            );
            let manifest = world.replacement_manifest(
                &replacement_id(&row.plugin),
                &row.plugin,
                ReplacementShape {
                    drop_item: Some(item.clone()),
                    ..ReplacementShape::default()
                },
            );
            let result = world.install(Tier::T1, manifest, Vec::new());
            assert_eq!(
                diag_code(&result),
                "manifest.replaces-surface-incomplete",
                "{} without {item}",
                row.plugin
            );
            match result {
                Err(diagnostic) => assert!(
                    diagnostic.message.contains(&item),
                    "{} without {item}: the error does not list it: {}",
                    row.plugin,
                    diagnostic.message
                ),
                Ok(()) => panic!("{} without {item} must be refused", row.plugin),
            }
        }
    }
}

/// REQ: DEV-847
#[test]
fn dev_847_replacement_leaves_snapshot_section_and_config_table_of_its_row_unchanged() {
    const SCHEDULER: &str = "ilium.core.scheduler";
    const SCHEDULER_SECTION: &str = "ilium.core.scheduler";
    const COST: &str = "ilium.core.cost";

    let mut world = ExtWorld::with_core_plugins();
    world.register_snapshot_section(SCHEDULER, SCHEDULER_SECTION, KIB64 as u64);
    world.set_snapshot_reply(
        SCHEDULER,
        SCHEDULER_SECTION,
        r#"{"scheduled_input":[],"prompt_queue":[]}"#,
    );
    world.take_snapshot();
    world.set_project_value(COST, "cost", json!({"daily_limit": 5}));
    let snapshot_before = world.stored_snapshot_section(SCHEDULER_SECTION);
    let cost_before = world.get_setting(COST, "cost");
    assert!(snapshot_before.is_some(), "the snapshot section must be stored first");

    for core in [SCHEDULER, COST] {
        let replacement = replacement_id(core);
        world.set_enabled(core, false);
        let manifest =
            world.replacement_manifest(&replacement, core, ReplacementShape::default());
        world
            .install(Tier::T1, manifest, Vec::new())
            .expect("a complete replacement installs");
        world.set_enabled(&replacement, true);
        world.restore_snapshot();
        world.set_enabled(&replacement, false);
        world.set_enabled(core, true);
    }

    assert_eq!(
        world.stored_snapshot_section(SCHEDULER_SECTION),
        snapshot_before,
        "a replacement changed the snapshot section named in its row"
    );
    assert_eq!(
        world.get_setting(COST, "cost"),
        cost_before,
        "a replacement changed the config table named in its row"
    );
}

/// REQ: DEV-849
#[test]
fn dev_849_matrix_has_one_row_per_replaceable_core_page_and_each_row_matches_its_page() {
    let rows = matrix_rows();
    let row_ids: BTreeSet<String> = rows.iter().map(|row| row.plugin.clone()).collect();
    assert_eq!(
        row_ids.len(),
        rows.len(),
        "the matrix lists a core plugin twice"
    );

    let pages = core_pages();
    let replaceable: BTreeSet<String> = pages
        .iter()
        .filter(|(_, text)| text.contains("**Replaceable:** yes"))
        .map(|(id, _)| id.clone())
        .collect();
    assert_eq!(
        row_ids, replaceable,
        "matrix rows differ from the pages that say Replaceable: yes"
    );

    for row in &rows {
        let text = pages
            .iter()
            .find(|(id, _)| *id == row.plugin)
            .map(|(_, text)| text.as_str())
            .unwrap_or_default();
        for (kind, name) in &row.surface {
            assert!(
                text.contains(name.as_str()),
                "{}: {kind}:{name} is in the matrix but not on its core page",
                row.plugin
            );
        }
    }
}
