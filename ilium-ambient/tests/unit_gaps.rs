use ilium_ambient::minecraft::nbt::{self, Compound, Document, Tag, Text};
use ilium_ambient::minecraft::region::{chunk_body, verify_identity, Error, Identity, Layout};
use ilium_ambient::ImagesSettings;

fn must<T, E: std::fmt::Debug>(result: Result<T, E>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{what}: expected Ok, got {error:?}"),
    }
}

fn compound(entries: Vec<(&str, Tag)>) -> Compound {
    entries
        .into_iter()
        .map(|(key, value)| (Text::from(key), value))
        .collect()
}

fn document(root: Compound) -> Document {
    Document {
        name: Text::from(""),
        root,
    }
}

/// A chunk stored in the pre-1.18 `Level` schema.
fn level_document(data_version: i32, x_pos: i32, z_pos: i32) -> Document {
    let level = compound(vec![("xPos", Tag::Int(x_pos)), ("zPos", Tag::Int(z_pos))]);
    document(compound(vec![
        ("DataVersion", Tag::Int(data_version)),
        ("Level", Tag::Compound(level)),
    ]))
}

/// A chunk stored in the root schema, with coordinates at the root.
fn root_document(data_version: i32, x_pos: i32, z_pos: i32) -> Document {
    document(compound(vec![
        ("DataVersion", Tag::Int(data_version)),
        ("xPos", Tag::Int(x_pos)),
        ("zPos", Tag::Int(z_pos)),
    ]))
}

#[test]
fn chunk_body_returns_level_compound_for_level_schema() {
    let doc = level_document(3000, 1, -1);
    let (version, layout, body) = must(chunk_body(&doc), "level chunk");
    assert_eq!(version, 3000, "DataVersion must be returned unchanged");
    assert_eq!(layout, Layout::Level);
    assert_eq!(nbt::get(body, "xPos"), Some(&Tag::Int(1)));
    assert_eq!(nbt::get(body, "zPos"), Some(&Tag::Int(-1)));
}

#[test]
fn chunk_body_returns_root_compound_when_no_level_key() {
    let doc = root_document(3100, 5, 6);
    let (version, layout, body) = must(chunk_body(&doc), "root chunk");
    assert_eq!(version, 3100);
    assert_eq!(layout, Layout::Root);
    assert_eq!(nbt::get(body, "xPos"), Some(&Tag::Int(5)));
    assert_eq!(nbt::get(body, "zPos"), Some(&Tag::Int(6)));
}

#[test]
fn chunk_body_missing_data_version_is_an_invalid_error() {
    let doc = document(compound(vec![("Level", Tag::Compound(Compound::new()))]));
    let result = chunk_body(&doc);
    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "a chunk without DataVersion must be rejected as invalid Anvil data"
    );
}

#[test]
fn chunk_body_non_int_data_version_is_an_invalid_error() {
    let doc = document(compound(vec![("DataVersion", Tag::Long(3000))]));
    let result = chunk_body(&doc);
    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "DataVersion stored as a Long must be rejected, only Int is an identity field"
    );
}

#[test]
fn chunk_body_accepts_data_version_at_both_admitted_boundaries() {
    let lowest_document = root_document(2834, 0, 0);
    let lowest = must(chunk_body(&lowest_document), "lowest version");
    assert_eq!(lowest.0, 2834);
    let highest_document = root_document(3218, 0, 0);
    let highest = must(chunk_body(&highest_document), "highest version");
    assert_eq!(highest.0, 3218);
}

#[test]
fn chunk_body_rejects_data_version_just_outside_admitted_range() {
    let too_old_document = root_document(2833, 0, 0);
    let too_old = chunk_body(&too_old_document);
    assert!(
        matches!(too_old, Err(Error::DataVersion(2833))),
        "DataVersion 2833 is below the admitted range 2834..=3218"
    );
    let too_new_document = root_document(3219, 0, 0);
    let too_new = chunk_body(&too_new_document);
    assert!(
        matches!(too_new, Err(Error::DataVersion(3219))),
        "DataVersion 3219 is above the admitted range 2834..=3218"
    );
}

#[test]
fn chunk_body_rejects_level_that_is_not_a_compound() {
    let doc = document(compound(vec![
        ("DataVersion", Tag::Int(3000)),
        ("Level", Tag::Int(7)),
    ]));
    let result = chunk_body(&doc);
    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "a Level tag that is not a compound must be rejected"
    );
}

#[test]
fn chunk_body_rejects_ambiguous_root_and_level_schemas() {
    let level = compound(vec![("xPos", Tag::Int(1)), ("zPos", Tag::Int(2))]);
    let doc = document(compound(vec![
        ("DataVersion", Tag::Int(3000)),
        ("Level", Tag::Compound(level)),
        ("xPos", Tag::Int(1)),
    ]));
    let result = chunk_body(&doc);
    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "a root that carries both Level and root-level xPos must be ambiguous"
    );
}

#[test]
fn verify_identity_matching_level_chunk_returns_identity() {
    let doc = level_document(3100, 4, -7);
    let identity = must(verify_identity(&doc, [4, -7]), "matching level chunk");
    assert_eq!(
        identity,
        Identity {
            position: [4, -7],
            data_version: 3100,
            layout: Layout::Level,
        }
    );
}

#[test]
fn verify_identity_matching_root_chunk_reports_root_layout() {
    let doc = root_document(3100, 0, 0);
    let identity = must(verify_identity(&doc, [0, 0]), "matching root chunk");
    assert_eq!(identity.layout, Layout::Root);
    assert_eq!(identity.position, [0, 0]);
}

#[test]
fn verify_identity_accepts_extreme_matching_coordinates() {
    let doc = root_document(3000, i32::MIN, i32::MAX);
    let identity = must(verify_identity(&doc, [i32::MIN, i32::MAX]), "extreme chunk");
    assert_eq!(identity.position, [i32::MIN, i32::MAX]);
}

#[test]
fn verify_identity_mismatched_coordinates_return_coordinates_error() {
    let doc = level_document(3100, 4, -7);
    match verify_identity(&doc, [4, -6]) {
        Err(Error::Coordinates { expected, actual }) => {
            assert_eq!(expected, [4, -6], "expected position must be echoed back");
            assert_eq!(actual, [4, -7], "actual position must come from the file");
        }
        other => panic!("mismatched chunk coordinates must fail, got {other:?}"),
    }
}

#[test]
fn verify_identity_missing_coordinate_field_is_an_invalid_error() {
    let level = compound(vec![("zPos", Tag::Int(2))]);
    let doc = document(compound(vec![
        ("DataVersion", Tag::Int(3100)),
        ("Level", Tag::Compound(level)),
    ]));
    let result = verify_identity(&doc, [1, 2]);
    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "a chunk without xPos has no identity and must be rejected"
    );
}

#[test]
fn verify_identity_empty_document_is_an_invalid_error() {
    let doc = document(Compound::new());
    let result = verify_identity(&doc, [0, 0]);
    assert!(
        matches!(result, Err(Error::Invalid(_))),
        "an empty document has no DataVersion and no identity"
    );
}

#[test]
fn verify_identity_version_boundaries_follow_admitted_range() {
    let lowest = must(
        verify_identity(&root_document(2834, 9, 9), [9, 9]),
        "lowest",
    );
    assert_eq!(lowest.data_version, 2834);
    let highest = must(
        verify_identity(&root_document(3218, 9, 9), [9, 9]),
        "highest",
    );
    assert_eq!(highest.data_version, 3218);

    let too_old = verify_identity(&root_document(2833, 9, 9), [9, 9]);
    assert!(
        matches!(too_old, Err(Error::DataVersion(2833))),
        "version gate must run before the coordinate check"
    );
    let too_new = verify_identity(&root_document(3219, 9, 9), [9, 9]);
    assert!(matches!(too_new, Err(Error::DataVersion(3219))));
}

#[test]
fn images_source_label_single_mode_names_default_builtin_image() {
    let settings: ImagesSettings = must(serde_json::from_str("{}"), "default settings");
    assert_eq!(settings.source_label(), "NightCafe dreamscape");
}

#[test]
fn images_source_label_single_mode_out_of_range_builtin_falls_back() {
    let settings: ImagesSettings = must(
        serde_json::from_str(r#"{"mode":"single","source":{"Builtin":9}}"#),
        "out of range builtin",
    );
    assert_eq!(settings.source_label(), "Built-in image");
}

#[test]
fn images_source_label_single_mode_shows_local_file_path() {
    let settings: ImagesSettings = must(
        serde_json::from_str(r#"{"source":{"Local":"/srv/pictures/lake.jpg"}}"#),
        "local source",
    );
    assert_eq!(settings.source_label(), "File: /srv/pictures/lake.jpg");
}

#[test]
fn images_source_label_single_mode_shows_url_source() {
    let settings: ImagesSettings = must(
        serde_json::from_str(r#"{"source":{"Url":"https://img.example/a.jpg"}}"#),
        "url source",
    );
    assert_eq!(settings.source_label(), "URL: https://img.example/a.jpg");
}

#[test]
fn images_source_label_folders_mode_lists_folder_patterns() {
    let settings: ImagesSettings = must(
        serde_json::from_str(r#"{"mode":"folders","folders":"~/Pictures/*.jpg;/srv/art"}"#),
        "folders mode",
    );
    assert_eq!(
        settings.source_label(),
        "Folders: ~/Pictures/*.jpg;/srv/art"
    );
}

#[test]
fn images_source_label_folders_mode_ignores_single_source() {
    let settings: ImagesSettings = must(
        serde_json::from_str(r#"{"mode":"folders","folders":"/srv/art","source":{"Builtin":1}}"#),
        "folders mode with source",
    );
    assert_eq!(settings.source_label(), "Folders: /srv/art");
}

#[test]
fn images_source_label_url_list_mode_lists_urls() {
    let settings: ImagesSettings = must(
        serde_json::from_str(
            r#"{"mode":"url_list","urls":"https://a.example/1.jpg;https://a.example/2.jpg"}"#,
        ),
        "url list mode",
    );
    assert_eq!(
        settings.source_label(),
        "URLs: https://a.example/1.jpg;https://a.example/2.jpg"
    );
}

#[test]
fn images_source_label_single_mode_ignores_folders_and_urls() {
    let settings: ImagesSettings = must(
        serde_json::from_str(r#"{"mode":"single","folders":"/x","urls":"https://u.example/x"}"#),
        "single mode with other lists",
    );
    assert_eq!(settings.source_label(), "NightCafe dreamscape");
}

#[test]
fn images_source_label_folders_mode_with_empty_list_keeps_prefix() {
    let settings: ImagesSettings = must(
        serde_json::from_str(r#"{"mode":"folders"}"#),
        "folders mode, empty list",
    );
    assert_eq!(settings.source_label(), "Folders: ");
}
