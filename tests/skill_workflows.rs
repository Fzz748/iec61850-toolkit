//! Execute the Skill's bundled JSON examples against the real CLI from an isolated working folder.
use std::{fs, path::Path, process::Command};

use serde_json::Value;

fn examples() -> (Value, Value) {
    let reference = include_str!("../skills/iec61850/references/model-authoring.md");
    let mut examples = reference.split("```json").skip(1).map(|section| {
        let content = section.split("```").next().unwrap().trim();
        serde_json::from_str(content).expect("Skill JSON example must be executable input")
    });
    (
        examples.next().expect("device specification example"),
        examples.next().expect("edit patch example"),
    )
}

fn write_json(dir: &Path, name: &str, value: &Value) {
    fs::write(dir.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn run(dir: &Path, args: &[&str], expected_exit: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_iec61850"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("one structured CLI result");
    assert_eq!(output.status.code(), Some(expected_exit), "{report}");
    report
}

#[test]
fn bundled_authoring_and_review_examples_work_outside_the_checkout() {
    let workspace = tempfile::Builder::new()
        .prefix("iec61850 skill ")
        .tempdir()
        .unwrap();
    let dir = workspace.path();
    let (spec, mut patch) = examples();
    write_json(dir, "device.json", &spec);

    let capabilities = run(dir, &["capabilities"], 0);
    assert_eq!(capabilities["schema_version"], 1);
    for operation in patch["operations"].as_array().unwrap() {
        assert!(
            capabilities["data"]["operations"]
                .as_array()
                .unwrap()
                .contains(&operation["op"])
        );
    }

    let preview = run(
        dir,
        &[
            "icd",
            "create",
            "--spec",
            "device.json",
            "--dry-run",
            "--references-only",
        ],
        0,
    );
    assert_eq!(preview["data"]["written"], false);
    assert!(!dir.join("device.icd").exists());
    let created = run(
        dir,
        &[
            "icd",
            "create",
            "--spec",
            "device.json",
            "-o",
            "device.icd",
            "--references-only",
        ],
        0,
    );
    assert_eq!(created["data"]["written"], true);
    assert_eq!(
        preview["data"]["candidate_sha256"],
        created["data"]["candidate_sha256"]
    );
    let original = fs::read(dir.join("device.icd")).unwrap();

    let inspected = run(dir, &["scl", "inspect", "device.icd"], 0);
    assert_eq!(inspected["data"]["ieds"][0], spec["ied_name"]);
    patch["source_sha256"] = inspected["data"]["source_sha256"].clone();
    write_json(dir, "changes.json", &patch);

    let preview = run(
        dir,
        &[
            "scl",
            "edit",
            "device.icd",
            "--patch",
            "changes.json",
            "--dry-run",
            "--references-only",
        ],
        0,
    );
    assert_eq!(preview["data"]["written"], false);
    assert!(!dir.join("device.updated.icd").exists());
    let saved = run(
        dir,
        &[
            "scl",
            "edit",
            "device.icd",
            "--patch",
            "changes.json",
            "-o",
            "device.updated.icd",
            "--references-only",
        ],
        0,
    );
    assert_eq!(saved["data"]["written"], true);
    assert_eq!(saved["data"]["output"], "device.updated.icd");
    assert_eq!(
        preview["data"]["candidate_sha256"],
        saved["data"]["candidate_sha256"]
    );
    assert_eq!(fs::read(dir.join("device.icd")).unwrap(), original);

    let checked = run(
        dir,
        &["scl", "validate", "device.updated.icd", "--references-only"],
        0,
    );
    assert_eq!(checked["scope"], "xml_and_supported_references");
    assert_eq!(checked["checks"][2]["status"], "not_run");
    let comparison = run(dir, &["scl", "diff", "device.icd", "device.updated.icd"], 0);
    assert_eq!(comparison["data"]["changes"], saved["data"]["changes"]);
    assert!(!comparison["data"]["changes"].as_array().unwrap().is_empty());
}

#[test]
fn workflow_preserves_failures_for_required_xsd_and_stale_source_hashes() {
    let workspace = tempfile::tempdir().unwrap();
    let dir = workspace.path();
    let (spec, mut patch) = examples();
    write_json(dir, "device.json", &spec);

    let unavailable = run(
        dir,
        &[
            "icd",
            "create",
            "--spec",
            "device.json",
            "-o",
            "required-xsd.icd",
            "--xsd",
            "missing-schema.xsd",
        ],
        3,
    );
    assert_eq!(unavailable["status"], "incomplete");
    assert_eq!(unavailable["checks"][2]["status"], "not_run");
    assert_eq!(unavailable["data"]["written"], false);
    assert!(!dir.join("required-xsd.icd").exists());

    run(
        dir,
        &[
            "icd",
            "create",
            "--spec",
            "device.json",
            "-o",
            "device.icd",
            "--references-only",
        ],
        0,
    );
    let inspected = run(dir, &["scl", "inspect", "device.icd"], 0);
    patch["source_sha256"] = inspected["data"]["source_sha256"].clone();
    write_json(dir, "changes.json", &patch);

    let original_path = dir.join("device.icd");
    let updated_source = format!(
        "{}\n<!-- changed after inspection -->\n",
        fs::read_to_string(&original_path).unwrap()
    );
    fs::write(&original_path, &updated_source).unwrap();
    let rejected = run(
        dir,
        &[
            "scl",
            "edit",
            "device.icd",
            "--patch",
            "changes.json",
            "-o",
            "device.updated.icd",
            "--references-only",
        ],
        2,
    );
    assert_eq!(rejected["status"], "fail");
    assert!(
        rejected["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("source_sha256")
    );
    assert_eq!(fs::read_to_string(original_path).unwrap(), updated_source);
    assert!(!dir.join("device.updated.icd").exists());
}
