use crate::{
    Error, Result,
    adapters::xsd,
    cli::{Command, IcdCommand, OutputOptions, SclCommand, ValidationOptions},
    report::{Check, Report, Status},
    scl::{SclDocument, diff, document::MAX_BYTES, edit, generate, query, validate},
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

pub fn read_text(path: &Path) -> Result<String> {
    let file = File::open(path).map_err(|e| Error::Input(format!("{}: {e}", path.display())))?;
    if !file.metadata()?.is_file() {
        return Err(Error::Input(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::Input(format!(
            "{} exceeds the 32 MiB input limit",
            path.display()
        )));
    }
    String::from_utf8(bytes).map_err(|_| Error::Input(format!("{} is not UTF-8", path.display())))
}

fn save_new(path: &Path, content: &str) -> Result<()> {
    if path.try_exists()? || path.symlink_metadata().is_ok() {
        return Err(Error::Input(format!(
            "output already exists: {}",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(content.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(path)
        .map_err(|e| Error::Io(e.error))?;
    Ok(())
}

fn validate_into(
    report: &mut Report,
    doc: &SclDocument<'_>,
    options: &ValidationOptions,
) -> Result<()> {
    report.scope = if options.references_only {
        "xml_and_supported_references"
    } else {
        "xml_supported_references_and_xsd"
    }
    .into();
    report.checks.push(Check {
        name: "xml".into(),
        status: Status::Pass,
        detail: "UTF-8 XML with SCL namespace; DTD disabled".into(),
    });
    report.diagnostics = validate::references(doc);
    report.checks.push(Check { name: "references".into(), status: if report.diagnostics.is_empty() { Status::Pass } else { Status::Fail },
        detail: "type references/cycles, LN identities, explicit non-array FCDA paths, local control-block DataSet links; excludes NSD, ExtRef, network engineering and device behavior".into() });
    let xsd_check = match &options.xsd {
        Some(schema) => xsd::validate(
            doc,
            schema,
            &options.xmllint,
            Duration::from_secs(options.xsd_timeout),
        )?,
        None => Check {
            name: "xsd".into(),
            status: Status::NotRun,
            detail: if options.references_only {
                "not requested (--references-only)"
            } else {
                "provide --xsd for schema validation or explicitly select --references-only"
            }
            .into(),
        },
    };
    report.checks.push(xsd_check);
    report.status = if report.checks.iter().any(|c| c.status == Status::Fail) {
        Status::Fail
    } else if !options.references_only && report.checks.iter().any(|c| c.status == Status::NotRun) {
        Status::Incomplete
    } else {
        Status::Pass
    };
    Ok(())
}

fn finish_mutation(
    mut report: Report,
    candidate: &str,
    options: &OutputOptions,
    validation: &ValidationOptions,
) -> Result<Report> {
    let doc = SclDocument::parse(candidate)?;
    report.data["candidate_sha256"] = json!(doc.digest());
    report.data["dry_run"] = json!(options.dry_run);
    report.data["written"] = json!(false);
    validate_into(&mut report, &doc, validation)?;
    if report.status == Status::Pass && !options.dry_run {
        let output = options
            .output
            .as_ref()
            .ok_or_else(|| Error::Input("--output is required unless --dry-run is used".into()))?;
        save_new(output, candidate)?;
        report.data["output"] = json!(output);
        report.data["written"] = json!(true);
    }
    Ok(report)
}

pub fn run(command: &Command) -> Result<Report> {
    match command {
        Command::Capabilities => Ok(Report::new(
            "capabilities",
            "tool",
            json!({
                "commands": ["scl inspect", "scl validate", "scl diff", "scl edit", "icd create"],
                "edit_profile": "2007B4", "encoding": "UTF-8 (optional BOM)", "maximum_input_bytes": MAX_BYTES,
                "templates": ["minimal-ggio"], "signal_kinds": ["sps", "mv"],
                "operations": ["set_description", "add_logical_node", "add_dataset", "replace_dataset", "remove_dataset", "add_goose", "set_goose_dataset"],
                "validation": {"built_in": "XML, type references/cycles, LN identities, explicit non-array FCDA paths, local control-block DataSet links", "xsd": "external xmllint with matching local schema", "not_implemented": ["NSD/CDC completeness", "ExtRef bindings", "network engineering", "device behavior"]},
                "not_implemented": ["live networking", "packet capture analysis", "code generation", "general data-type editing", "automatic schema downloads"]
            }),
        )),
        Command::Scl {
            command: SclCommand::Inspect { input, ied },
        } => {
            let source = read_text(input)?;
            let doc = SclDocument::parse(&source)?;
            Ok(Report::new(
                "scl.inspect",
                "inspection_only",
                query::inspect(&doc, ied.as_deref())?,
            ))
        }
        Command::Scl {
            command: SclCommand::Validate { input, validation },
        } => {
            let source = read_text(input)?;
            let doc = SclDocument::parse(&source)?;
            let mut report = Report::new(
                "scl.validate",
                "",
                json!({"input": input, "source_sha256": doc.digest()}),
            );
            validate_into(&mut report, &doc, validation)?;
            Ok(report)
        }
        Command::Scl {
            command: SclCommand::Diff { before, after },
        } => {
            let old = read_text(before)?;
            let new = read_text(after)?;
            let old = SclDocument::parse(&old)?;
            let new = SclDocument::parse(&new)?;
            Ok(Report::new(
                "scl.diff",
                "semantic_diff_only",
                json!({"before_sha256": old.digest(), "after_sha256": new.digest(), "source_changed": old.source != new.source, "changes": diff::semantic(&old, &new), "note": "element/attribute content; FCDA order is significant; Private and foreign-namespace subtrees are compared as raw XML; other comments/formatting are ignored"}),
            ))
        }
        Command::Scl {
            command:
                SclCommand::Edit {
                    input,
                    patch,
                    output,
                    validation,
                },
        } => {
            let source = read_text(input)?;
            let patch: edit::Patch = serde_json::from_str(&read_text(patch)?)?;
            let candidate = edit::prepare(&source, &patch)?;
            let old = SclDocument::parse(&source)?;
            let new = SclDocument::parse(&candidate)?;
            let report = Report::new(
                "scl.edit",
                "",
                json!({"input": input, "source_sha256": old.digest(), "changes": diff::semantic(&old, &new)}),
            );
            finish_mutation(report, &candidate, output, validation)
        }
        Command::Icd {
            command:
                IcdCommand::Create {
                    spec,
                    output,
                    validation,
                },
        } => {
            let spec: generate::DeviceSpec = serde_json::from_str(&read_text(spec)?)?;
            let candidate = generate::create(&spec)?;
            let report = Report::new(
                "icd.create",
                "",
                json!({"template": spec.template, "ied": spec.ied_name, "note": "structural example template; verify supported services and data-model completeness against your device"}),
            );
            finish_mutation(report, &candidate, output, validation)
        }
    }
}

pub fn print(report: &Report, json_output: bool) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if json_output {
        serde_json::to_writer_pretty(&mut out, report)?;
        writeln!(out)?;
    } else {
        writeln!(
            out,
            "{}: {:?} (scope: {})",
            report.command, report.status, report.scope
        )?;
        for check in &report.checks {
            writeln!(
                out,
                "  {}: {:?} - {}",
                check.name, check.status, check.detail
            )?;
        }
        for diagnostic in &report.diagnostics {
            writeln!(
                out,
                "  {} at {}:{} {}: {}",
                diagnostic.code,
                diagnostic.line,
                diagnostic.column,
                diagnostic.path,
                diagnostic.message
            )?;
        }
        if report.data != Value::Null {
            serde_json::to_writer_pretty(&mut out, &report.data)?;
            writeln!(out)?;
        }
    }
    Ok(())
}
