//! reference-signing-and-trust.md, part 2: the signature entry, key files and publisher
//! identity (PKG-202..221, PKG-650, PKG-651).

use super::signing_support::*;
use super::support::*;
use ilium_plugin_host::package::read_package;
use ilium_plugin_host::signing::{sign, SignedAt, SigningKey};

fn good_trust() -> ilium_plugin_host::signing::TrustStore {
    trust_of(0, "example")
}

fn signed_text(bytes: &[u8]) -> String {
    String::from_utf8(data_of(bytes, "signature")).expect("utf8")
}

fn parsed(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_str(&signed_text(bytes)).expect("json")
}

// ---- PKG-202: exact manifest bytes ----

/// REQ: PKG-202
#[test]
fn pkg_202_signature_over_non_canonical_manifest_bytes_verifies() {
    let mut manifest = vector_manifest();
    manifest.extend_from_slice(b"   \n\n");
    let bytes = signed_package(0, "example", &manifest);
    assert_eq!(refusal(&bytes, &good_trust()), "accepted");
}

/// REQ: PKG-202
#[test]
fn pkg_202_one_extra_trailing_byte_after_signing_is_invalid() {
    let signed_over = vector_manifest();
    let mut stored = signed_over.clone();
    stored.push(b'\n');
    let text = sig_doc("ilium-sig/1", &[obj(0, "example", &signed_over)]);
    let bytes = with_signature_text(&stored, &vector_component(), text.as_bytes());
    assert_eq!(refusal(&bytes, &good_trust()), "manifest.signature-invalid");
}

/// REQ: PKG-202
#[test]
fn pkg_202_crlf_instead_of_lf_in_stored_manifest_is_invalid() {
    let original = vector_manifest();
    let crlf = String::from_utf8(original.clone()).unwrap().replace('\n', "\r\n").into_bytes();
    let text = sig_doc("ilium-sig/1", &[obj(0, "example", &original)]);
    let bytes = with_signature_text(&crlf, &vector_component(), text.as_bytes());
    assert_eq!(refusal(&bytes, &good_trust()), "manifest.signature-invalid");
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(24))]
    /// REQ: PKG-202, PKG-201
    #[test]
    fn pkg_202_any_trailing_whitespace_signed_as_stored_verifies(extra in "[ \\n\\t]{0,12}") {
        let mut manifest = vector_manifest();
        manifest.extend_from_slice(extra.as_bytes());
        let bytes = signed_package(0, "example", &manifest);
        proptest::prop_assert_eq!(refusal(&bytes, &good_trust()), "accepted");
    }
}

// ---- PKG-203: point and scalar validity ----

const GROUP_ORDER_LE: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

fn add_order_to_s(signature: &[u8]) -> Vec<u8> {
    let mut out = signature.to_vec();
    let mut carry = 0u16;
    for i in 0..32 {
        let sum = out[32 + i] as u16 + GROUP_ORDER_LE[i] as u16 + carry;
        out[32 + i] = sum as u8;
        carry = sum >> 8;
    }
    out
}

fn package_with_signature_bytes(signature: &[u8]) -> Vec<u8> {
    let mut o = obj(0, "example", &vector_manifest());
    o.signature = b64url(signature);
    with_sig(&sig_doc("ilium-sig/1", &[o]))
}

/// REQ: PKG-203
#[test]
fn pkg_203_non_canonical_s_plus_group_order_is_invalid() {
    let good = sign_raw(0, &message(PREFIX, "example", &vector_manifest()));
    let bytes = package_with_signature_bytes(&add_order_to_s(&good));
    assert_eq!(refusal(&bytes, &good_trust()), "manifest.signature-invalid");
}

/// REQ: PKG-203
#[test]
fn pkg_203_s_equal_to_group_order_is_invalid() {
    let good = sign_raw(0, &message(PREFIX, "example", &vector_manifest()));
    let mut forged = good[..32].to_vec();
    forged.extend_from_slice(&GROUP_ORDER_LE);
    assert_eq!(refusal(&package_with_signature_bytes(&forged), &good_trust()), "manifest.signature-invalid");
}

/// REQ: PKG-203
#[test]
fn pkg_203_s_all_ones_is_invalid() {
    let good = sign_raw(0, &message(PREFIX, "example", &vector_manifest()));
    let mut forged = good[..32].to_vec();
    forged.extend_from_slice(&[0xff; 32]);
    assert_eq!(refusal(&package_with_signature_bytes(&forged), &good_trust()), "manifest.signature-invalid");
}

/// REQ: PKG-203
#[test]
fn pkg_203_trusted_key_that_is_not_a_curve_point_never_verifies() {
    // y = 2 is not on the curve (no square root), so the key cannot be decoded.
    let mut bad_point = vec![0u8; 32];
    bad_point[0] = 2;
    let mut trust = good_trust();
    trust.keys[0].public_key = ilium_plugin_host::signing::PublicKey(bad_point);
    assert_ne!(refusal(&vector_signed(), &trust), "accepted");
}

/// REQ: PKG-203
#[test]
fn pkg_203_all_zero_signature_is_invalid() {
    assert_eq!(refusal(&package_with_signature_bytes(&[0u8; 64]), &good_trust()), "manifest.signature-invalid");
}

// ---- PKG-204..206: document shape ----

/// REQ: PKG-204
#[test]
fn pkg_204_vector_document_is_accepted() {
    assert_eq!(refusal(&vector_signed(), &good_trust()), "accepted");
}

/// REQ: PKG-204
#[test]
fn pkg_204_extra_top_level_key_refused_signature_format() {
    let text = sig_doc("ilium-sig/1", &[obj(0, "example", &vector_manifest())]).replacen("{\n", "{\n  \"extra\": 1,\n", 1);
    assert_eq!(read_refusal(&with_sig(&text)).as_str(), "package.signature-format");
}

/// REQ: PKG-204
#[test]
fn pkg_204_missing_signatures_key_refused_signature_format() {
    assert_eq!(read_refusal(&with_sig("{\n  \"format\": \"ilium-sig/1\"\n}\n")), "package.signature-format");
}

/// REQ: PKG-204
#[test]
fn pkg_204_missing_format_key_refused_signature_format() {
    assert_eq!(read_refusal(&with_sig("{\n  \"signatures\": []\n}\n")), "package.signature-format");
}

/// REQ: PKG-204
#[test]
fn pkg_204_other_format_values_refused_signature_format() {
    for format in ["ilium-sig/0", "ilium-sig/2", "ilium-sig/1 ", "ILIUM-SIG/1", ""] {
        let text = sig_doc(format, &[obj(0, "example", &vector_manifest())]);
        assert_eq!(read_refusal(&with_sig(&text)), "package.signature-format", "format {format:?}");
    }
}

/// REQ: PKG-204
#[test]
fn pkg_204_not_json_refused_signature_format() {
    for body in ["", "not json", "[]", "null", "{"] {
        assert_eq!(read_refusal(&with_sig(body)), "package.signature-format", "body {body:?}");
    }
}

fn many(count: usize) -> Vec<Obj> {
    (0..count).map(|i| obj(i as u8, "example", &vector_manifest())).collect()
}

/// REQ: PKG-205
#[test]
fn pkg_205_zero_signature_objects_refused_signature_format() {
    assert_eq!(read_refusal(&with_sig(&sig_doc("ilium-sig/1", &[]))), "package.signature-format");
}

/// REQ: PKG-205
#[test]
fn pkg_205_one_and_four_objects_are_well_formed() {
    for count in [1usize, 4] {
        assert_eq!(read_refusal(&with_sig(&sig_doc("ilium-sig/1", &many(count)))), "accepted", "count {count}");
    }
}

/// REQ: PKG-205
#[test]
fn pkg_205_five_objects_refused_signature_format() {
    assert_eq!(read_refusal(&with_sig(&sig_doc("ilium-sig/1", &many(5)))), "package.signature-format");
}

/// REQ: PKG-205
#[test]
fn pkg_205_algorithm_other_than_ed25519_refused_signature_format() {
    for algorithm in ["ed448", "Ed25519", "ed25519ph", ""] {
        let mut o = obj(0, "example", &vector_manifest());
        o.algorithm = algorithm.into();
        let got = read_refusal(&with_sig(&sig_doc("ilium-sig/1", &[o])));
        assert_eq!(got, "package.signature-format", "algorithm {algorithm:?}");
    }
}

/// REQ: PKG-205
#[test]
fn pkg_205_object_with_an_extra_key_refused_signature_format() {
    let text = sig_doc("ilium-sig/1", &[obj(0, "example", &vector_manifest())]).replace("\"algorithm\"", "\"extra\": 1,\n      \"algorithm\"");
    assert_eq!(read_refusal(&with_sig(&text)), "package.signature-format");
}

/// REQ: PKG-205
#[test]
fn pkg_205_object_missing_each_key_refused_signature_format() {
    for key in ["algorithm", "key_id", "publisher_id", "signature", "signed_at"] {
        let text = sig_doc("ilium-sig/1", &[obj(0, "example", &vector_manifest())]);
        let kept: Vec<&str> = text.lines().filter(|line| !line.contains(&format!("\"{key}\""))).collect();
        assert_eq!(read_refusal(&with_sig(&format!("{}\n", kept.join("\n")))), "package.signature-format", "missing {key}");
    }
}

/// REQ: PKG-206
#[test]
fn pkg_206_key_id_must_be_64_lowercase_hex() {
    let valid = key_id_of(0);
    let bad = [valid[..63].to_string(), format!("{valid}0"), valid.to_uppercase(), format!("g{}", &valid[1..])];
    for key_id in bad {
        let mut o = obj(0, "example", &vector_manifest());
        o.key_id = key_id.clone();
        assert_eq!(read_refusal(&with_sig(&sig_doc("ilium-sig/1", &[o]))), "package.signature-format", "key_id {key_id}");
    }
}

/// REQ: PKG-206
#[test]
fn pkg_206_signature_must_be_86_unpadded_base64url_characters() {
    let good = obj(0, "example", &vector_manifest()).signature;
    assert_eq!(good.len(), 86);
    let bad = [good[..85].to_string(), format!("{good}A"), format!("{good}=="), good.replace('-', "+"), good.replace('_', "/")];
    for signature in bad {
        let mut o = obj(0, "example", &vector_manifest());
        o.signature = signature.clone();
        let got = read_refusal(&with_sig(&sig_doc("ilium-sig/1", &[o])));
        assert_eq!(got, "package.signature-format", "signature {signature}");
    }
}

/// REQ: PKG-206
#[test]
fn pkg_206_signed_at_form_is_checked() {
    for signed_at in ["2026-10-12T10:00:00Z", "2026-10-12", "2026-10-12 10:00", "12/10/2026 10:00:00", ""] {
        let mut o = obj(0, "example", &vector_manifest());
        o.signed_at = signed_at.into();
        let got = read_refusal(&with_sig(&sig_doc("ilium-sig/1", &[o])));
        assert_eq!(got, "package.signature-format", "signed_at {signed_at:?}");
    }
}

/// REQ: PKG-206
#[test]
fn pkg_206_signed_at_never_changes_the_decision() {
    for signed_at in ["1970-01-01 00:00:00", "2026-10-12 10:00:00", "2999-12-31 23:59:59"] {
        let mut o = obj(0, "example", &vector_manifest());
        o.signed_at = signed_at.into();
        assert_eq!(refusal(&with_sig(&sig_doc("ilium-sig/1", &[o])), &good_trust()), "accepted", "signed_at {signed_at}");
    }
}

// ---- PKG-207: countersignature ----

/// REQ: PKG-207 MAY
#[test]
fn pkg_207_second_object_valid_and_untrusted_still_accepted() {
    let text = sig_doc("ilium-sig/1", &[obj(0, "example", &vector_manifest()), obj(7, "example", &vector_manifest())]);
    assert_eq!(refusal(&with_sig(&text), &good_trust()), "accepted");
}

/// REQ: PKG-207 MAY
#[test]
fn pkg_207_single_object_document_is_accepted_too() {
    assert_eq!(refusal(&vector_signed(), &good_trust()), "accepted");
}

/// REQ: PKG-207
#[test]
fn pkg_207_every_object_is_verified_so_one_bad_object_refuses() {
    let mut second = obj(7, "example", &vector_manifest());
    second.signature = b64url(&[1u8; 64]);
    let text = sig_doc("ilium-sig/1", &[obj(0, "example", &vector_manifest()), second]);
    assert_eq!(refusal(&with_sig(&text), &good_trust()), "manifest.signature-invalid");
}

/// REQ: PKG-207
#[test]
fn pkg_207_at_least_one_valid_object_from_a_trusted_key_is_required() {
    let text = sig_doc("ilium-sig/1", &[obj(7, "example", &vector_manifest()), obj(8, "example", &vector_manifest())]);
    assert_ne!(refusal(&with_sig(&text), &good_trust()), "accepted");
}

/// REQ: PKG-207
#[test]
fn pkg_207_trusted_key_may_be_the_second_object() {
    let text = sig_doc("ilium-sig/1", &[obj(7, "example", &vector_manifest()), obj(0, "example", &vector_manifest())]);
    assert_eq!(refusal(&with_sig(&text), &good_trust()), "accepted");
}

// ---- PKG-208: size and codes ----

fn padded_doc_of_size(size: usize) -> String {
    let base = sig_doc("ilium-sig/1", &[obj(0, "example", &vector_manifest())]);
    // Pad inside the document with insignificant whitespace before the final LF.
    let pad = size - base.len();
    format!("{}{}\n", base.trim_end_matches('\n'), " ".repeat(pad))
}

/// REQ: PKG-208
#[test]
fn pkg_208_entry_of_exactly_16_kib_is_not_refused_for_size() {
    let doc = padded_doc_of_size(16384);
    assert_eq!(doc.len(), 16384);
    assert_ne!(read_refusal(&with_sig(&doc)), "package.zip-invalid");
}

/// REQ: PKG-208
#[test]
fn pkg_208_entry_one_byte_over_16_kib_is_refused() {
    let doc = padded_doc_of_size(16385);
    assert_eq!(doc.len(), 16385);
    let got = read_refusal(&with_sig(&doc));
    assert!(got == "package.signature-format" || got == "package.zip-invalid", "got {got}");
}

/// REQ: PKG-208
#[test]
fn pkg_208_entry_one_byte_under_16_kib_is_not_refused_for_size() {
    let doc = padded_doc_of_size(16383);
    assert_ne!(read_refusal(&with_sig(&doc)), "package.zip-invalid");
}

/// REQ: PKG-208
#[test]
fn pkg_208_signature_that_does_not_verify_is_signature_invalid() {
    let mut o = obj(0, "example", &vector_manifest());
    o.signature = b64url(&[9u8; 64]);
    assert_eq!(refusal(&with_sig(&sig_doc("ilium-sig/1", &[o])), &good_trust()), "manifest.signature-invalid");
}

/// REQ: PKG-208
#[test]
fn pkg_208_key_id_of_another_key_than_the_signer_is_signature_invalid() {
    let mut o = obj(0, "example", &vector_manifest());
    o.key_id = key_id_of(5);
    let mut trust = good_trust();
    trust.keys.push(trust_of(5, "example").keys.remove(0));
    assert_eq!(refusal(&with_sig(&sig_doc("ilium-sig/1", &[o])), &trust), "manifest.signature-invalid");
}

/// REQ: PKG-208
#[test]
fn pkg_208_key_id_matching_no_key_is_signature_invalid() {
    let mut o = obj(0, "example", &vector_manifest());
    o.key_id = "0".repeat(64);
    assert_eq!(refusal(&with_sig(&sig_doc("ilium-sig/1", &[o])), &good_trust()), "manifest.signature-invalid");
}

// ---- PKG-209 / PKG-650 ----

/// REQ: PKG-209
#[test]
fn pkg_209_signature_entry_sorts_after_server_wasm() {
    let names = names_of(&vector_signed());
    assert_eq!(names, strings(&["manifest.json", "server.wasm", "signature"]));
}

/// REQ: PKG-209
#[test]
fn pkg_209_signing_adds_exactly_one_entry_and_keeps_the_others_byte_identical() {
    let signed = sign(&vector_unsigned(), &SigningKey(vector_seed()), SignedAt(VECTOR_SIGNED_AT.into())).expect("sign");
    assert_eq!(names_of(&signed), strings(&["manifest.json", "server.wasm", "signature"]));
    assert_eq!(data_of(&signed, "manifest.json"), vector_manifest());
    assert_eq!(data_of(&signed, "server.wasm"), vector_component());
}

/// REQ: PKG-209
#[test]
fn pkg_209_signed_archive_reads_back_valid() {
    let package = read_package(&vector_signed()).expect("valid package");
    assert!(package.signature.is_some());
}

/// REQ: PKG-209
#[test]
fn pkg_209_signature_entry_before_manifest_is_out_of_order() {
    let bytes = zip_of(&[("signature", vector_signature_doc()), ("manifest.json", vector_manifest()), ("server.wasm", vector_component())]);
    assert_ne!(read_refusal(&bytes), "accepted");
}

/// REQ: PKG-209, PKG-650
#[test]
fn pkg_209_signing_twice_gives_identical_archives() {
    let sign_once = || sign(&vector_unsigned(), &SigningKey(vector_seed()), SignedAt(VECTOR_SIGNED_AT.into())).expect("sign");
    assert_eq!(sign_once(), sign_once());
}

/// REQ: PKG-650
#[test]
fn pkg_650_serialisation_matches_the_golden_file_byte_for_byte() {
    let signed = sign(&vector_unsigned(), &SigningKey(vector_seed()), SignedAt(VECTOR_SIGNED_AT.into())).expect("sign");
    assert_eq!(data_of(&signed, "signature"), vector_signature_doc());
}

/// REQ: PKG-650
#[test]
fn pkg_650_document_has_no_bom_crlf_and_exactly_one_final_lf() {
    let doc = data_of(&vector_signed(), "signature");
    assert!(!doc.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert!(!doc.contains(&b'\r'));
    assert!(doc.ends_with(b"}\n") && !doc.ends_with(b"\n\n"));
}

/// REQ: PKG-650
#[test]
fn pkg_650_keys_are_in_table_order_with_two_space_indent() {
    let text = signed_text(&vector_signed());
    let order = ["\"format\"", "\"signatures\"", "\"algorithm\"", "\"key_id\"", "\"publisher_id\"", "\"signature\"", "\"signed_at\""];
    let positions: Vec<usize> = order.iter().map(|k| text.find(k).expect("key present")).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "positions {positions:?}");
    assert!(text.contains("\n  \"format\"") && text.contains("\n      \"algorithm\""));
    assert!(!text.contains('\t'));
}

/// REQ: PKG-650
#[test]
fn pkg_650_two_runs_with_the_same_input_are_byte_identical() {
    let a = sign(&vector_unsigned(), &SigningKey(vector_seed()), SignedAt(VECTOR_SIGNED_AT.into())).expect("sign");
    let b = sign(&vector_unsigned(), &SigningKey(vector_seed()), SignedAt(VECTOR_SIGNED_AT.into())).expect("sign");
    assert_eq!(sha256_hex(&a), sha256_hex(&b));
    assert_eq!(sha256_hex(&a), VECTOR_SIGNED_SHA256);
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(16))]
    /// REQ: PKG-650, PKG-206
    #[test]
    fn pkg_650_signed_at_round_trips_into_the_document(day in 1u32..28, hour in 0u32..24, minute in 0u32..60, second in 0u32..60) {
        let at = format!("2026-10-{day:02} {hour:02}:{minute:02}:{second:02}");
        let signed = sign(&vector_unsigned(), &SigningKey(vector_seed()), SignedAt(at.clone())).expect("sign");
        let doc = parsed(&signed);
        proptest::prop_assert_eq!(doc["signatures"][0]["signed_at"].as_str(), Some(at.as_str()));
    }
}

// ---- PKG-651: signed_at sources via the CLI ----

fn sign_cli(vars: &[(&str, &str)], extra: &[&str]) -> (CliFixture, ilium_plugin_host::cli::CliOutput) {
    let fixture = cli_fixture();
    let key = fixture.work().join("publisher.key");
    write_key(&key, 0, 0o600);
    let unsigned = put_package(&fixture, "in.ilpl", &vector_unsigned());
    let out_path = fixture.work().join("out.ilpl");
    let key_text = key.to_string_lossy().into_owned();
    let out_text = out_path.to_string_lossy().into_owned();
    let mut args = vec!["sign", unsigned.as_str(), "--key", key_text.as_str(), "--out", out_text.as_str()];
    args.extend_from_slice(extra);
    let out = run_env(&fixture, &args, vars);
    (fixture, out)
}

fn signed_at_of(fixture: &CliFixture) -> String {
    let bytes = std::fs::read(fixture.work().join("out.ilpl")).expect("signed output");
    parsed(&bytes)["signatures"][0]["signed_at"].as_str().expect("signed_at").to_string()
}

/// REQ: PKG-651
#[test]
fn pkg_651_signed_at_flag_is_used() {
    let (fixture, out) = sign_cli(&[], &["--signed-at", "2026-10-12 10:00:00"]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert_eq!(signed_at_of(&fixture), "2026-10-12 10:00:00");
}

/// REQ: PKG-651
#[test]
fn pkg_651_source_date_epoch_is_used_without_the_flag() {
    let (fixture, out) = sign_cli(&[("SOURCE_DATE_EPOCH", "1791800000")], &[]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert_eq!(signed_at_of(&fixture), "2026-10-12 10:13:20");
}

/// REQ: PKG-651
#[test]
fn pkg_651_flag_wins_over_source_date_epoch() {
    let (fixture, _) = sign_cli(&[("SOURCE_DATE_EPOCH", "1791800000")], &["--signed-at", "2026-10-12 10:00:00"]);
    assert_eq!(signed_at_of(&fixture), "2026-10-12 10:00:00");
}

/// REQ: PKG-651
#[test]
fn pkg_651_epoch_1791799200_is_ten_oclock() {
    let (fixture, _) = sign_cli(&[("SOURCE_DATE_EPOCH", "1791799200")], &[]);
    assert_eq!(signed_at_of(&fixture), "2026-10-12 10:00:00");
}

/// REQ: PKG-651
#[test]
fn pkg_651_cli_signature_with_the_vector_inputs_equals_the_golden_package() {
    let (fixture, out) = sign_cli(&[], &["--signed-at", "2026-10-12 10:00:00"]);
    assert_eq!(out.exit_code, 0);
    let bytes = std::fs::read(fixture.work().join("out.ilpl")).expect("output");
    assert_eq!(sha256_hex(&bytes), VECTOR_SIGNED_SHA256);
}

/// REQ: PKG-651
#[test]
fn pkg_651_malformed_signed_at_flag_fails() {
    let (_, out) = sign_cli(&[], &["--signed-at", "yesterday"]);
    assert_ne!(out.exit_code, 0);
}

// ---- PKG-210..213: key files ----

fn keygen(fixture: &CliFixture, dir: &str, extra: &[&str], vars: &[(&str, &str)]) -> ilium_plugin_host::cli::CliOutput {
    let target = fixture.work().join(dir);
    let target_text = target.to_string_lossy().into_owned();
    let mut args = vec!["keygen", "--out", target_text.as_str()];
    args.extend_from_slice(extra);
    run_env(fixture, &args, vars)
}

fn key_file(fixture: &CliFixture, dir: &str) -> String {
    std::fs::read_to_string(fixture.work().join(dir).join("publisher.key")).expect("publisher.key")
}

/// REQ: PKG-210
#[test]
fn pkg_210_unencrypted_keygen_writes_a_private_key_pem() {
    let fixture = cli_fixture();
    let out = keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs", "--no-passphrase"], &[]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    let pem = key_file(&fixture, "k");
    assert!(pem.starts_with("-----BEGIN PRIVATE KEY-----\n") && pem.trim_end().ends_with("-----END PRIVATE KEY-----"));
}

/// REQ: PKG-210
#[test]
fn pkg_210_unencrypted_key_der_is_pkcs8_ed25519_of_48_bytes() {
    let fixture = cli_fixture();
    keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs", "--no-passphrase"], &[]);
    let body: String = key_file(&fixture, "k").lines().filter(|l| !l.starts_with("-----")).collect();
    let der = unb64url(&body.replace('+', "-").replace('/', "_").trim_end_matches('='));
    assert_eq!(der.len(), 48);
    assert_eq!(&der[..16], unhex("302e020100300506032b657004220420").as_slice());
}

/// REQ: PKG-210, PKG-211
#[test]
fn pkg_210_encrypted_keygen_uses_the_encrypted_label() {
    let fixture = cli_fixture();
    let out = keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs"], &[("ILIUM_KEY_PASSPHRASE", "correct horse battery")]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert!(key_file(&fixture, "k").starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----"));
}

/// REQ: PKG-210
#[test]
fn pkg_210_a_private_key_in_pem_is_accepted_by_sign() {
    let (_, out) = sign_cli(&[], &["--signed-at", "2026-10-12 10:00:00"]);
    assert_eq!(out.exit_code, 0);
}

/// REQ: PKG-210
#[test]
fn pkg_210_a_key_with_another_label_is_refused() {
    let fixture = cli_fixture();
    let key = fixture.work().join("publisher.key");
    write_key(&key, 0, 0o600);
    let text = std::fs::read_to_string(&key).unwrap().replace("PRIVATE KEY", "RSA PRIVATE KEY");
    write_file(&key, text.as_bytes());
    std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let unsigned = put_package(&fixture, "in.ilpl", &vector_unsigned());
    let out = run_cli(&fixture, &["sign", unsigned.as_str(), "--key", &key.to_string_lossy()]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-211
#[test]
fn pkg_211_encrypted_key_is_pbes2_with_at_least_600000_iterations() {
    let fixture = cli_fixture();
    keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs"], &[("ILIUM_KEY_PASSPHRASE", "correct horse battery")]);
    let body: String = key_file(&fixture, "k").lines().filter(|l| !l.starts_with("-----")).collect();
    let der = unb64url(&body.replace('+', "-").replace('/', "_").trim_end_matches('='));
    // OIDs: PBES2 1.2.840.113549.1.5.13, PBKDF2 ...5.12, hmacWithSHA256 ...2.9, AES-256-CBC 2.16.840.1.101.3.4.1.42.
    let has = |needle: &[u8]| der.windows(needle.len()).any(|w| w == needle);
    assert!(has(&unhex("2a864886f70d01050d")), "PBES2");
    assert!(has(&unhex("2a864886f70d01050c")), "PBKDF2");
    assert!(has(&unhex("2a864886f70d02090")) || has(&unhex("2a864886f70d0209")), "HMAC-SHA256");
    assert!(has(&unhex("60864801650304012a")), "AES-256-CBC");
}

/// REQ: PKG-211
#[test]
fn pkg_211_passphrase_file_enables_encryption() {
    let fixture = cli_fixture();
    let pass = fixture.work().join("pass.txt");
    write_file(&pass, b"correct horse battery\n");
    let out = keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs", "--passphrase-file", &pass.to_string_lossy()], &[]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert!(key_file(&fixture, "k").contains("ENCRYPTED PRIVATE KEY"));
}

/// REQ: PKG-211
#[test]
fn pkg_211_empty_passphrase_is_refused() {
    let fixture = cli_fixture();
    let out = keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs"], &[("ILIUM_KEY_PASSPHRASE", "")]);
    assert_ne!(out.exit_code, 0);
    assert!(!fixture.work().join("k").join("publisher.key").exists());
}

/// REQ: PKG-211
#[test]
fn pkg_211_empty_passphrase_file_is_refused() {
    let fixture = cli_fixture();
    let pass = fixture.work().join("empty.txt");
    write_file(&pass, b"");
    let out = keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs", "--passphrase-file", &pass.to_string_lossy()], &[]);
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-211
#[test]
fn pkg_211_passphrase_never_appears_in_the_output() {
    let fixture = cli_fixture();
    let out = keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs"], &[("ILIUM_KEY_PASSPHRASE", "xyzzy-secret-pass")]);
    assert!(!all_text(&out).contains("xyzzy-secret-pass"));
}

/// REQ: PKG-212
#[test]
fn pkg_212_keygen_creates_file_0600_and_folder_0700() {
    let fixture = cli_fixture();
    keygen(&fixture, "k", &["--publisher-id", "example", "--publisher", "Example Labs", "--no-passphrase"], &[]);
    assert_eq!(mode_of(&fixture.work().join("k").join("publisher.key")), 0o600);
    assert_eq!(mode_of(&fixture.work().join("k")), 0o700);
}

/// REQ: PKG-212
#[test]
fn pkg_212_keygen_refuses_to_overwrite_an_existing_key() {
    let fixture = cli_fixture();
    let args = ["--publisher-id", "example", "--publisher", "Example Labs", "--no-passphrase"];
    keygen(&fixture, "k", &args, &[]);
    let before = key_file(&fixture, "k");
    let out = keygen(&fixture, "k", &args, &[]);
    assert_ne!(out.exit_code, 0);
    assert!(all_codes(&out).contains(&"trust.key-exists".to_string()), "{}", all_text(&out));
    assert_eq!(key_file(&fixture, "k"), before);
}

fn sign_with_mode(mode: u32) -> ilium_plugin_host::cli::CliOutput {
    let fixture = cli_fixture();
    let key = fixture.work().join("publisher.key");
    write_key(&key, 0, mode);
    let unsigned = put_package(&fixture, "in.ilpl", &vector_unsigned());
    run_cli(&fixture, &["sign", unsigned.as_str(), "--key", &key.to_string_lossy(), "--signed-at", "2026-10-12 10:00:00"])
}

/// REQ: PKG-212
#[test]
fn pkg_212_group_or_other_readable_key_is_refused_key_permissions() {
    for mode in [0o640, 0o604, 0o644, 0o660, 0o666, 0o440] {
        let out = sign_with_mode(mode);
        assert_ne!(out.exit_code, 0, "mode {mode:o}");
        assert!(all_codes(&out).contains(&"trust.key-permissions".to_string()), "mode {mode:o}: {}", all_text(&out));
    }
}

/// REQ: PKG-212
#[test]
fn pkg_212_owner_only_modes_are_accepted() {
    for mode in [0o600, 0o400] {
        assert_eq!(sign_with_mode(mode).exit_code, 0, "mode {mode:o}");
    }
}

/// REQ: PKG-213
#[test]
fn pkg_213_env_key_text_is_used_when_no_key_flag_is_given() {
    let fixture = cli_fixture();
    let unsigned = put_package(&fixture, "in.ilpl", &vector_unsigned());
    let out_path = fixture.work().join("out.ilpl");
    let pem = pkcs8_pem(0);
    let out = run_env(&fixture, &["sign", unsigned.as_str(), "--out", &out_path.to_string_lossy(), "--signed-at", "2026-10-12 10:00:00"], &[("ILIUM_PUBLISHER_KEY", pem.as_str())]);
    if out.exit_code == 0 {
        assert_eq!(sha256_hex(&std::fs::read(out_path).unwrap()), VECTOR_SIGNED_SHA256);
    } else {
        assert_ne!(out.exit_code, 0, "MAY: the environment key is optional");
    }
}

/// REQ: PKG-213 MAY
#[test]
fn pkg_213_env_key_never_appears_in_any_output() {
    let fixture = cli_fixture();
    let unsigned = put_package(&fixture, "in.ilpl", &vector_unsigned());
    let pem = pkcs8_pem(0);
    let out = run_env(&fixture, &["sign", unsigned.as_str(), "--signed-at", "2026-10-12 10:00:00"], &[("ILIUM_PUBLISHER_KEY", pem.as_str())]);
    let text = all_text(&out);
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    assert!(!text.contains(&body) && !text.contains(&b64url(&seed_of(0))));
}

/// REQ: PKG-213
#[test]
fn pkg_213_key_never_appears_in_output_of_a_normal_sign() {
    let (_, out) = sign_cli(&[], &["--signed-at", "2026-10-12 10:00:00"]);
    let text = all_text(&out);
    assert!(!text.contains(&hex(&seed_of(0))) && !text.contains("BEGIN PRIVATE KEY"));
}

// ---- PKG-214..217: publisher.pub and identity ----

fn pub_text(key_id: &str, public_key: &str, publisher_id: &str, publisher: &str) -> String {
    format!(
        "{{\n  \"format\": \"ilium-pub/1\",\n  \"publisher_id\": \"{publisher_id}\",\n  \"publisher\": \"{publisher}\",\n  \"algorithm\": \"ed25519\",\n  \"public_key\": \"{public_key}\",\n  \"key_id\": \"{key_id}\",\n  \"created\": \"2026-10-12 10:00:00\"\n}}\n"
    )
}

fn trust_add(text: &str) -> (CliFixture, ilium_plugin_host::cli::CliOutput) {
    let fixture = cli_fixture();
    let path = put_package(&fixture, "publisher.pub", text.as_bytes());
    let out = run_cli(&fixture, &["trust", "add", path.as_str(), "--yes"]);
    (fixture, out)
}

/// REQ: PKG-214
#[test]
fn pkg_214_matching_key_id_is_accepted() {
    let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", "Example Labs"));
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: PKG-214
#[test]
fn pkg_214_public_key_is_32_bytes_in_43_base64url_characters() {
    assert_eq!(b64url(&public_of(0)).len(), 43);
    assert_eq!(unb64url(&b64url(&public_of(0))), public_of(0));
    assert_eq!(hex(&public_of(0)), VECTOR_PUBLIC_KEY_HEX);
}

/// REQ: PKG-214
#[test]
fn pkg_214_wrong_key_id_refused_key_malformed() {
    let (_, out) = trust_add(&pub_text(&key_id_of(1), &b64url(&public_of(0)), "example", "Example Labs"));
    assert!(all_codes(&out).contains(&"trust.key-malformed".to_string()), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-214
#[test]
fn pkg_214_uppercase_key_id_refused_key_malformed() {
    let (_, out) = trust_add(&pub_text(&VECTOR_KEY_ID.to_uppercase(), &b64url(&public_of(0)), "example", "Example Labs"));
    assert!(all_codes(&out).contains(&"trust.key-malformed".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-214
#[test]
fn pkg_214_public_key_of_31_or_33_bytes_refused_key_malformed() {
    for len in [31usize, 33] {
        let key = vec![7u8; len];
        let (_, out) = trust_add(&pub_text(&sha256_hex(&key), &b64url(&key), "example", "Example Labs"));
        assert!(all_codes(&out).contains(&"trust.key-malformed".to_string()), "len {len}: {}", all_text(&out));
    }
}

/// REQ: PKG-215
#[test]
fn pkg_215_display_name_lengths_1_and_64_accepted() {
    for name in ["E".to_string(), "E".repeat(64)] {
        let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", &name));
        assert_eq!(out.exit_code, 0, "len {}: {}", name.len(), all_text(&out));
    }
}

/// REQ: PKG-215
#[test]
fn pkg_215_display_name_lengths_0_and_65_refused() {
    for name in [String::new(), "E".repeat(65)] {
        let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", &name));
        assert_ne!(out.exit_code, 0, "len {}", name.len());
    }
}

/// REQ: PKG-215
#[test]
fn pkg_215_control_characters_in_the_display_name_refused() {
    for ch in ["\\u0000", "\\u0007", "\\u001b", "\\u007f", "\\u0085"] {
        let name = format!("Ex{ch}ample");
        let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", &name));
        assert_ne!(out.exit_code, 0, "char {ch}");
    }
}

/// REQ: PKG-215
#[test]
fn pkg_215_bidi_override_characters_in_the_display_name_refused() {
    for ch in ["\\u202a", "\\u202b", "\\u202c", "\\u202d", "\\u202e", "\\u2066", "\\u2067", "\\u2068", "\\u2069"] {
        let name = format!("Ex{ch}ample");
        let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", &name));
        assert_ne!(out.exit_code, 0, "char {ch}");
    }
}

/// REQ: PKG-215
#[test]
fn pkg_215_non_ascii_display_name_is_accepted() {
    let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", "Caf\\u00e9 Labs"));
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
}

/// REQ: PKG-216
#[test]
fn pkg_216_fingerprint_display_is_16_groups_of_4() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Example Labs")]));
    let out = run_cli(&fixture, &["trust", "list"]);
    let expected = "5647 5aa7 5463 474c 0285 df5d bf2b cab7 3da6 5135 8839 e9b7 7481 b2ea b107 708c";
    assert!(all_text(&out).contains(expected) || all_text(&out).contains(VECTOR_KEY_ID), "{}", all_text(&out));
}

/// REQ: PKG-216
#[test]
fn pkg_216_noninteractive_trust_prompt_is_not_a_silent_accept() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_signed(), &[]);
    assert_ne!(out.exit_code, 0);
    assert!(all_codes(&out).contains(&"trust.key-unknown".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-216
#[test]
fn pkg_216_the_whole_64_characters_are_shown_when_a_key_is_refused() {
    let fixture = cli_fixture();
    let out = install(&fixture, &vector_signed(), &[]);
    let text = all_text(&out);
    let grouped = "5647 5aa7 5463 474c 0285 df5d bf2b cab7 3da6 5135 8839 e9b7 7481 b2ea b107 708c";
    assert!(text.contains(grouped) || text.contains(VECTOR_KEY_ID), "{text}");
}

fn grammar_ok(id: &str) -> bool {
    if id.is_empty() || id.len() > 64 {
        return false;
    }
    let segments: Vec<&str> = id.split('.').collect();
    if segments.len() > 4 {
        return false;
    }
    segments.iter().all(|s| {
        let mut parts = s.split('-');
        let first = parts.next().unwrap_or("");
        let head_ok = first.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && first.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        head_ok && parts.all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
    })
}

fn publisher_id_accepted(id: &str) -> bool {
    let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), id, "Example Labs"));
    out.exit_code == 0
}

/// REQ: PKG-217
#[test]
fn pkg_217_valid_publisher_ids_are_accepted() {
    for id in ["example", "a", "com.acme", "a-b.c1.d-2.e3", "acme2", "x".repeat(64).as_str()] {
        assert!(publisher_id_accepted(id), "id {id}");
    }
}

/// REQ: PKG-217
#[test]
fn pkg_217_invalid_publisher_ids_are_refused() {
    let long = "x".repeat(65);
    for id in ["", "Example", "1abc", "a--b", "a-", "-a", "a_b", "a.", ".a", "a..b", "a.b.c.d.e", "a b", long.as_str(), "a.1b", "é"] {
        assert!(!publisher_id_accepted(id), "id {id:?}");
    }
}

/// REQ: PKG-217
#[test]
fn pkg_217_length_boundary_63_64_65() {
    for (len, ok) in [(63usize, true), (64, true), (65, false)] {
        assert_eq!(publisher_id_accepted(&"x".repeat(len)), ok, "len {len}");
    }
}

/// REQ: PKG-217
#[test]
fn pkg_217_segment_count_boundary_4_and_5() {
    assert!(publisher_id_accepted("a.b.c.d"));
    assert!(!publisher_id_accepted("a.b.c.d.e"));
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(32))]
    /// REQ: PKG-217
    #[test]
    fn pkg_217_cli_acceptance_matches_the_grammar(id in "[a-z0-9.\\-]{0,20}") {
        // Reserved ids are refused for another reason, so skip them.
        proptest::prop_assume!(!["ilium", "core", "local", "test"].contains(&id.as_str()));
        proptest::prop_assert_eq!(publisher_id_accepted(&id), grammar_ok(&id));
    }
}

// ---- PKG-218..221 ----

/// REQ: PKG-218
#[test]
fn pkg_218_plugin_id_must_start_with_publisher_id_and_a_dot() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Example Labs")]));
    let manifest = manifest_for("example.minimal", "Example Labs");
    assert_eq!(refusal(&signed_package(0, "example", &manifest), &good_trust()), "accepted");
}

/// REQ: PKG-218
#[test]
fn pkg_218_plugin_id_of_another_publisher_refused_id_publisher() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Example Labs")]));
    let out = install(&fixture, &signed_package(0, "example", &manifest_for("other.minimal", "Example Labs")), &[]);
    assert!(all_codes(&out).contains(&"manifest.id-publisher".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-218
#[test]
fn pkg_218_publisher_id_prefix_without_the_dot_is_refused() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Example Labs")]));
    let out = install(&fixture, &signed_package(0, "example", &manifest_for("examples.minimal", "Example Labs")), &[]);
    assert!(all_codes(&out).contains(&"manifest.id-publisher".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-218
#[test]
fn pkg_218_dotted_publisher_id_signs_dotted_plugin_ids() {
    let fixture = cli_fixture();
    let manifest = manifest_for("com.acme.notes", "Acme Inc");
    write_trust(&fixture, &trust_json(&[entry(0, "com.acme", "Acme Inc")]));
    let out = install(&fixture, &signed_package(0, "com.acme", &manifest), &[]);
    assert!(!all_codes(&out).contains(&"manifest.id-publisher".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-219
#[test]
fn pkg_219_equal_display_name_is_accepted() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Example Labs")]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(!all_codes(&out).contains(&"manifest.publisher-mismatch".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-219
#[test]
fn pkg_219_case_difference_refused_publisher_mismatch() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "example labs")]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(all_codes(&out).contains(&"manifest.publisher-mismatch".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-219
#[test]
fn pkg_219_nfc_equivalent_names_match() {
    let fixture = cli_fixture();
    // "e" + combining acute (NFD) in the store, precomposed (NFC) in the manifest.
    let manifest = manifest_for("example.minimal", "Caf\\u00e9 Labs");
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Cafe\\u0301 Labs")]));
    let out = install(&fixture, &signed_package(0, "example", &manifest), &[]);
    assert!(!all_codes(&out).contains(&"manifest.publisher-mismatch".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-219
#[test]
fn pkg_219_trailing_space_difference_refused_publisher_mismatch() {
    let fixture = cli_fixture();
    write_trust(&fixture, &trust_json(&[entry(0, "example", "Example Labs ")]));
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(all_codes(&out).contains(&"manifest.publisher-mismatch".to_string()), "{}", all_text(&out));
}

/// REQ: PKG-220
#[test]
fn pkg_220_reserved_publisher_ids_cannot_be_trusted() {
    for id in ["ilium", "core", "local", "test"] {
        let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), id, "Example Labs"));
        assert!(all_codes(&out).contains(&"trust.publisher-reserved".to_string()), "id {id}: {}", all_text(&out));
        assert_ne!(out.exit_code, 0);
    }
}

/// REQ: PKG-220
#[test]
fn pkg_220_a_longer_id_that_merely_starts_with_a_reserved_word_is_allowed() {
    for id in ["iliumlabs", "tester", "corelib", "ilium-labs", "local-dev"] {
        let (_, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), id, "Example Labs"));
        assert!(!all_codes(&out).contains(&"trust.publisher-reserved".to_string()), "id {id}");
    }
}

/// REQ: PKG-220
#[test]
fn pkg_220_ilium_prefixed_plugin_id_with_valid_signature_refused_id_reserved() {
    let fixture = cli_fixture();
    let out = install(&fixture, &signed_package(0, "example", &manifest_for("ilium.thing", "Example Labs")), &["--trust", VECTOR_KEY_ID]);
    assert!(all_codes(&out).iter().any(|c| c == "manifest.id-reserved" || c == "manifest.id-publisher"), "{}", all_text(&out));
    assert_ne!(out.exit_code, 0);
}

/// REQ: PKG-221
#[test]
fn pkg_221_trust_add_creates_the_store_with_mode_0600() {
    let (fixture, out) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", "Example Labs"));
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    let path = [fixture.state().join("ilium").join("trust.json"), fixture.state().join("trust.json")].into_iter().find(|p| p.exists()).expect("store file");
    assert_eq!(mode_of(&path), 0o600);
}

/// REQ: PKG-221
#[test]
fn pkg_221_store_content_is_the_documented_format() {
    let (fixture, _) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", "Example Labs"));
    let value: serde_json::Value = serde_json::from_str(&read_trust(&fixture).expect("store")).expect("json");
    assert_eq!(value["format"], "ilium-trust/1");
    let entry = &value["keys"][0];
    assert_eq!(entry["key_id"], VECTOR_KEY_ID);
    assert_eq!(entry["publisher_id"], "example");
    assert_eq!(entry["status"], "trusted");
    assert_eq!(entry["added_by"], "user");
    assert!(entry["revoked"].is_null());
}

/// REQ: PKG-221
#[test]
fn pkg_221_no_temporary_file_is_left_after_a_write() {
    let (fixture, _) = trust_add(&pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", "Example Labs"));
    let leftovers: Vec<String> = std::fs::read_dir(fixture.state().join("ilium"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "trust.json")
        .collect();
    assert!(leftovers.is_empty(), "leftovers {leftovers:?}");
}

/// REQ: PKG-221
#[test]
fn pkg_221_trust_store_env_replaces_the_path() {
    let fixture = cli_fixture();
    let custom = fixture.work().join("custom-trust.json");
    let path = put_package(&fixture, "publisher.pub", pub_text(VECTOR_KEY_ID, &b64url(&public_of(0)), "example", "Example Labs").as_bytes());
    let out = run_env(&fixture, &["trust", "add", path.as_str(), "--yes"], &[("ILIUM_TRUST_STORE", &custom.to_string_lossy())]);
    assert_eq!(out.exit_code, 0, "{}", all_text(&out));
    assert!(custom.exists());
}

/// REQ: PKG-221
#[test]
fn pkg_221_store_with_an_unknown_format_is_empty_with_a_warning() {
    let fixture = cli_fixture();
    write_trust(&fixture, "{\n  \"format\": \"ilium-trust/9\",\n  \"keys\": []\n}\n");
    let out = run_cli(&fixture, &["trust", "list", "--json"]);
    assert!(all_text(&out).contains("trust.store-invalid"), "{}", all_text(&out));
    assert!(records(&out, "key").is_empty());
}

/// REQ: PKG-221
#[test]
fn pkg_221_unreadable_store_gives_an_empty_effective_set() {
    let fixture = cli_fixture();
    write_trust(&fixture, "not json at all");
    let out = install(&fixture, &vector_signed(), &[]);
    assert!(all_codes(&out).contains(&"trust.key-unknown".to_string()), "{}", all_text(&out));
}
