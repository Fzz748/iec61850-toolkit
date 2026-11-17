use serde_json::{Value, json};
use std::process::Command;
use std::{fs, path::Path};

fn run(args: &[&str]) -> (i32, Value) {
    let result = Command::new(env!("CARGO_BIN_EXE_iec61850"))
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        result.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value = serde_json::from_slice(&result.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid JSON: {e}; {}",
            String::from_utf8_lossy(&result.stdout)
        )
    });
    (result.status.code().unwrap(), value)
}

fn fixture(dir: &Path) -> std::path::PathBuf {
    let file = dir.join("设备 original.icd");
    let spec = serde_json::from_str(include_str!("../../examples/device.json")).unwrap();
    let content = iec61850_toolkit::scl::generate::create(&spec).unwrap();
    fs::write(&file, content).unwrap();
    file
}

fn goose_patch(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("change.json");
    fs::write(&path, include_str!("../../examples/add-goose.json")).unwrap();
    path
}

#[test]
fn capabilities_are_machine_readable() {
    let result = Command::new(env!("CARGO_BIN_EXE_iec61850"))
        .args(["capabilities", "--json"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["data"]["version_whitelist"], false);
}

#[test]
fn ordinary_edit_can_save_a_draft_with_unrelated_validation_errors() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let draft = fs::read_to_string(&input)
        .unwrap()
        .replace(" release=\"4\"", "")
        .replace("lnType=\"TK_LPHD\"", "lnType=\"missing\"");
    fs::write(&input, &draft).unwrap();
    let request = dir.path().join("edit.json");
    fs::write(&request,serde_json::to_vec(&json!({"schema_version":1,"operations":[{"op":"set_description","target":{"ied":"DEMO","access_point":"AP1","ld_inst":"LD0","ln_class":"LLN0"},"description":"draft edit"}]})).unwrap()).unwrap();
    let output = dir.path().join("updated.icd");
    let (code, report) = run(&[
        "scl",
        "edit",
        input.to_str().unwrap(),
        "--patch",
        request.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["data"]["written"], true);
    assert_eq!(fs::read_to_string(&input).unwrap(), draft);
    let checked = dir.path().join("checked.icd");
    let (code, report) = run(&[
        "scl",
        "edit",
        input.to_str().unwrap(),
        "--patch",
        request.to_str().unwrap(),
        "-o",
        checked.to_str().unwrap(),
        "--references-only",
    ]);
    assert_eq!(code, 1, "{report}");
    assert!(!checked.exists());
}

#[test]
fn end_to_end_create_inspect_edit_validate_and_diff() {
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("device.json");
    fs::write(&spec, include_str!("../../examples/device.json")).unwrap();
    let original = dir.path().join("new.icd");
    let updated = dir.path().join("updated.icd");
    let patch = goose_patch(dir.path());
    let (code, report) = run(&[
        "icd",
        "create",
        "--spec",
        spec.to_str().unwrap(),
        "-o",
        original.to_str().unwrap(),
        "--references-only",
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["data"]["written"], true);
    let original_bytes = fs::read(&original).unwrap();
    let (code, view) = run(&[
        "scl",
        "inspect",
        original.to_str().unwrap(),
        "--ied",
        "DEMO",
    ]);
    assert_eq!(code, 0);
    assert_eq!(view["data"]["ieds"], json!(["DEMO"]));
    let (code, report) = run(&[
        "scl",
        "edit",
        original.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "-o",
        updated.to_str().unwrap(),
        "--references-only",
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
    assert!(fs::read_to_string(&updated).unwrap().contains("gcbStatus"));
    let (code, report) = run(&[
        "scl",
        "validate",
        updated.to_str().unwrap(),
        "--references-only",
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["checks"][2]["status"], "not_run");
    let (code, report) = run(&[
        "scl",
        "diff",
        original.to_str().unwrap(),
        updated.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(!report["data"]["changes"].as_array().unwrap().is_empty());
}

#[test]
fn dry_run_uses_the_same_candidate_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let original = fixture(dir.path());
    let patch = goose_patch(dir.path());
    let output = dir.path().join("result.icd");
    let (code, preview) = run(&[
        "scl",
        "edit",
        original.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--dry-run",
        "--references-only",
    ]);
    assert_eq!(code, 0, "{preview}");
    assert!(!output.exists());
    let (code, actual) = run(&[
        "scl",
        "edit",
        original.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--references-only",
    ]);
    assert_eq!(code, 0);
    assert_eq!(
        preview["data"]["candidate_sha256"],
        actual["data"]["candidate_sha256"]
    );
    assert_eq!(preview["data"]["changes"], actual["data"]["changes"]);
}

#[test]
fn invalid_reference_prevents_output_and_preserves_original() {
    let dir = tempfile::tempdir().unwrap();
    let original = fixture(dir.path());
    let bytes = fs::read(&original).unwrap();
    let patch = goose_patch(dir.path());
    let bad = fs::read_to_string(&patch)
        .unwrap()
        .replace("\"Ind1\"", "\"Missing\"");
    fs::write(&patch, bad).unwrap();
    let output = dir.path().join("result.icd");
    let (code, report) = run(&[
        "scl",
        "edit",
        original.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
        "--references-only",
    ]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["status"], "fail");
    assert!(!output.exists());
    assert_eq!(fs::read(&original).unwrap(), bytes);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn existing_output_and_original_paths_are_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let original = fixture(dir.path());
    let patch = goose_patch(dir.path());
    let original_bytes = fs::read(&original).unwrap();
    let output = dir.path().join("result.icd");
    fs::write(&output, "user content").unwrap();
    for target in [&output, &original] {
        let (code, _) = run(&[
            "scl",
            "edit",
            original.to_str().unwrap(),
            "--patch",
            patch.to_str().unwrap(),
            "-o",
            target.to_str().unwrap(),
            "--references-only",
        ]);
        assert_eq!(code, 2);
    }
    assert_eq!(fs::read_to_string(&output).unwrap(), "user content");
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
}

#[test]
fn default_validation_checks_references_and_default_edit_does_not_require_xsd() {
    let dir = tempfile::tempdir().unwrap();
    let original = fixture(dir.path());
    let patch = goose_patch(dir.path());
    let output = dir.path().join("result.icd");
    let (code, report) = run(&["scl", "validate", original.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(report["status"], "pass");
    assert_eq!(report["scope"], "xml_and_supported_references");
    assert_eq!(report["checks"][2]["status"], "not_run");
    let (code, _) = run(&[
        "scl",
        "edit",
        original.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "-o",
        output.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(output.exists());
}

#[test]
fn malformed_input_and_unknown_patch_fields_are_structured_errors() {
    let dir = tempfile::tempdir().unwrap();
    let malformed = dir.path().join("broken.icd");
    fs::write(&malformed, "<SCL").unwrap();
    let (code, report) = run(&["scl", "inspect", malformed.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CLI_EXECUTION");
    let input = fixture(dir.path());
    let patch = dir.path().join("unknown.json");
    fs::write(
        &patch,
        r#"{"schema_version":1,"operations":[],"typo":true}"#,
    )
    .unwrap();
    let (code, report) = run(&[
        "scl",
        "edit",
        input.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "--dry-run",
        "--references-only",
    ]);
    assert_eq!(code, 2);
    assert!(
        report["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("unknown field")
    );
}

#[test]
fn argument_errors_are_json_and_validation_modes_conflict() {
    let (code, report) = run(&[
        "scl",
        "validate",
        "file.icd",
        "--references-only",
        "--xsd",
        "SCL.xsd",
    ]);
    assert_eq!(code, 2);
    assert_eq!(report["diagnostics"][0]["code"], "CLI_ARGUMENT");
}

#[test]
fn mismatched_schema_and_unavailable_validator_cannot_pass() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let schema = dir.path().join("SCL.xsd");
    let missing = dir.path().join("does-not-exist.exe");
    for version in ["wrong", "2007B4"] {
        fs::write(&schema, format!(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="http://www.iec.ch/61850/2003/SCL" version="{version}"/>"#)).unwrap();
        let (code, report) = run(&[
            "scl",
            "validate",
            input.to_str().unwrap(),
            "--xsd",
            schema.to_str().unwrap(),
            "--xmllint",
            missing.to_str().unwrap(),
        ]);
        assert_eq!(code, 3);
        assert_eq!(report["checks"][2]["status"], "not_run");
    }
}

#[test]
#[ignore = "requires IEC61850_TEST_XSD and IEC61850_TEST_XMLLINT pointing to real local resources"]
fn real_xsd_accepts_create_and_edit_and_rejects_schema_only_errors() {
    let schema = std::env::var("IEC61850_TEST_XSD").expect("set IEC61850_TEST_XSD");
    let xmllint = std::env::var("IEC61850_TEST_XMLLINT").expect("set IEC61850_TEST_XMLLINT");
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("device.json");
    fs::write(&spec, include_str!("../../examples/device.json")).unwrap();
    let original = dir.path().join("original.icd");
    let updated = dir.path().join("updated.icd");
    let patch = goose_patch(dir.path());
    let (code, report) = run(&[
        "icd",
        "create",
        "--spec",
        spec.to_str().unwrap(),
        "-o",
        original.to_str().unwrap(),
        "--xsd",
        &schema,
        "--xmllint",
        &xmllint,
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["checks"][2]["status"], "pass");
    let (code, report) = run(&[
        "scl",
        "edit",
        original.to_str().unwrap(),
        "--patch",
        patch.to_str().unwrap(),
        "-o",
        updated.to_str().unwrap(),
        "--xsd",
        &schema,
        "--xmllint",
        &xmllint,
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["checks"][2]["status"], "pass");
    let invalid = fs::read_to_string(&updated).unwrap().replace(
        "manufacturer=\"iec61850-toolkit\"",
        "unsupportedAttribute=\"bad\"",
    );
    fs::write(&updated, invalid).unwrap();
    let (code, report) = run(&[
        "scl",
        "validate",
        updated.to_str().unwrap(),
        "--xsd",
        &schema,
        "--xmllint",
        &xmllint,
    ]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["checks"][1]["status"], "pass");
    assert_eq!(report["checks"][2]["status"], "fail");
}

#[test]
#[ignore = "requires IEC61850_TEST_LEGACY_XSD and IEC61850_TEST_XMLLINT"]
fn real_legacy_xsd_accepts_dated_schema_metadata() {
    let schema = std::env::var("IEC61850_TEST_LEGACY_XSD").expect("set IEC61850_TEST_LEGACY_XSD");
    let xmllint = std::env::var("IEC61850_TEST_XMLLINT").expect("set IEC61850_TEST_XMLLINT");
    let dir = tempfile::tempdir().unwrap();
    let input = fixture(dir.path());
    let legacy = fs::read_to_string(&input)
        .unwrap()
        .replace(" release=\"4\"", "");
    fs::write(&input, legacy).unwrap();
    let (code, report) = run(&[
        "scl",
        "validate",
        input.to_str().unwrap(),
        "--xsd",
        &schema,
        "--xmllint",
        &xmllint,
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["checks"][2]["status"], "pass");
}
