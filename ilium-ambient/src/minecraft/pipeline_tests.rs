//! Synthetic orchestration controls. Mock windows do not establish decoded
//! coverage; real decoder/loader acceptance remains a separate gate.
use super::*;

fn catalog() -> catalog::Catalog {
    catalog::Catalog {
        maps: [1, 4, 3, 2]
            .map(|number| catalog::Save {
                directory: PathBuf::from(format!("map-{number}")),
                metadata: catalog::Metadata {
                    name: "Synthetic map".into(),
                    data_version: 3218,
                    last_played: number,
                    spawn_position: None,
                    world_seed: Some(-number),
                },
            })
            .into(),
        ..catalog::Catalog::default()
    }
}
fn bindings(catalog: &catalog::Catalog) -> BTreeMap<PathBuf, MapContext> {
    catalog
        .maps
        .iter()
        .enumerate()
        .map(|(index, save)| {
            (
                save.directory.clone(),
                MapContext {
                    map: evidence::MapId([index as u8; 16]),
                    recent: vec![],
                },
            )
        })
        .collect()
}
fn report(save: &catalog::Save) -> MapReport {
    MapReport {
        directory: save.directory.clone(),
        allocated_chunks: 0,
        rejected_regions: 0,
        header_candidates: 0,
        scan_complete: true,
        rejected_windows: 0,
        error: None,
    }
}
fn mock_window(context: &MapContext) -> preparation::PreparedWindow {
    preparation::PreparedWindow {
        source: evidence::Source {
            map: context.map,
            generation: 9,
        },
        core: super::super::surface::Bounds {
            minimum: [0, 0],
            maximum: [15, 15],
        },
        bounds: super::super::surface::Bounds {
            minimum: [0, 0],
            maximum: [15, 15],
        },
        loaded: loader::LoadedWindow::default(),
        targets: vec![],
        stats: evidence::Stats::default(),
        work_used: 0,
    }
}

#[test]
fn recent_ranking_is_independent_of_catalog_order_and_failed_maps_fall_through() {
    let catalog = catalog();
    let bindings = bindings(&catalog);
    let mut order = Vec::new();
    let output = prepare_catalog_with(
        &catalog,
        &bindings,
        9,
        Limits::default(),
        &|| false,
        |save, context| {
            order.push(save.metadata.last_played);
            Ok((
                if save.metadata.last_played == 4 {
                    None
                } else {
                    Some(mock_window(context))
                },
                report(save),
            ))
        },
    )
    .unwrap();
    assert_eq!(order, [4, 3, 2, 1]);
    assert_eq!(
        output
            .maps
            .iter()
            .map(|map| map.last_played)
            .collect::<Vec<_>>(),
        [3, 2, 1]
    );
    assert_eq!(output.reports.len(), 4);
    assert_eq!(
        output
            .maps
            .iter()
            .map(|map| map.world_seed)
            .collect::<Vec<_>>(),
        [Some(-3), Some(-2), Some(-1)]
    );
    assert!(output.catalog_complete);
}

#[test]
fn finite_attempt_and_output_caps_are_explicit_and_unbound_maps_are_counted() {
    let catalog = catalog();
    let mut bindings = bindings(&catalog);
    let output = prepare_catalog_with(
        &catalog,
        &bindings,
        9,
        Limits {
            maps: 1,
            map_attempts: 2,
            ..Limits::default()
        },
        &|| false,
        |save, context| Ok((Some(mock_window(context)), report(save))),
    )
    .unwrap();
    assert_eq!(output.maps.len(), 1);
    assert_eq!(output.reports.len(), 1);
    assert!(!output.catalog_complete);
    bindings.remove(&PathBuf::from("map-4"));
    let output = prepare_catalog_with(
        &catalog,
        &bindings,
        9,
        Limits {
            maps: 1,
            map_attempts: 2,
            ..Limits::default()
        },
        &|| false,
        |save, _| Ok((None, report(save))),
    )
    .unwrap();
    assert_eq!(output.unbound_maps, 1);
    assert_eq!(output.reports.len(), 2);
    assert!(!output.catalog_complete);
}

#[test]
fn cancellation_or_wrong_generation_discards_already_prepared_maps() {
    use std::cell::Cell;
    let catalog = catalog();
    let bindings = bindings(&catalog);
    let stop = Cell::new(false);
    assert!(matches!(
        prepare_catalog_with(
            &catalog,
            &bindings,
            9,
            Limits::default(),
            &|| stop.get(),
            |save, context| {
                stop.set(true);
                Ok((Some(mock_window(context)), report(save)))
            }
        ),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        prepare_catalog_with(
            &catalog,
            &bindings,
            10,
            Limits::default(),
            &|| false,
            |save, context| Ok((Some(mock_window(context)), report(save)))
        ),
        Err(Error::Context)
    ));
}

#[test]
fn malformed_context_and_resource_limits_fail_before_io() {
    let mut catalog = catalog();
    let mut bindings = bindings(&catalog);
    let run = |catalog: &catalog::Catalog,
               bindings: &BTreeMap<PathBuf, MapContext>,
               generation,
               limits| {
        prepare_catalog_with(catalog, bindings, generation, limits, &|| false, |_, _| {
            panic!("invalid request performed I/O")
        })
    };
    assert!(matches!(
        run(&catalog, &bindings, 0, Limits::default()),
        Err(Error::Context)
    ));
    assert!(matches!(
        run(
            &catalog,
            &bindings,
            9,
            Limits {
                maps: 5,
                ..Limits::default()
            }
        ),
        Err(Error::Limits)
    ));
    let first = bindings.values().next().unwrap().map;
    bindings.values_mut().nth(1).unwrap().map = first;
    assert!(matches!(
        run(&catalog, &bindings, 9, Limits::default()),
        Err(Error::Context)
    ));
    let valid = super::tests::bindings(&catalog);
    catalog.maps[1].directory = catalog.maps[0].directory.clone();
    assert!(matches!(
        run(&catalog, &valid, 9, Limits::default()),
        Err(Error::Context)
    ));
}

#[test]
fn real_adapter_surfaces_absent_regions_without_creating_directories() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = catalog();
    for save in &mut catalog.maps {
        save.directory = root.path().join(&save.directory);
    }
    let output = prepare_catalog(&catalog, &bindings(&catalog), 9, Limits::default(), &|| {
        false
    })
    .unwrap();
    assert!(output.maps.is_empty());
    assert!(output.catalog_complete);
    assert_eq!(output.reports.len(), 4);
    assert!(output.reports.iter().all(|report| report.error.is_some()));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
