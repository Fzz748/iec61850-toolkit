use crate::{
    Result,
    report::{Check, Status},
    scl::{SclDocument, document::SCL_NS},
};
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn not_run(detail: impl Into<String>) -> Check {
    Check {
        name: "xsd".into(),
        status: Status::NotRun,
        detail: detail.into(),
    }
}

/// Uses the caller's local schema bundle; no schema downloads or shell invocation.
pub fn validate(
    doc: &SclDocument<'_>,
    schema: &Path,
    executable: &Path,
    timeout: Duration,
) -> Result<Check> {
    let schema = match schema.canonicalize() {
        Ok(p) => p,
        Err(e) => return Ok(not_run(format!("cannot open schema: {e}"))),
    };
    let mut schema_text = String::new();
    let schema_file = std::fs::File::open(&schema)?;
    if let Err(e) = schema_file
        .take(super::super::scl::document::MAX_BYTES as u64 + 1)
        .read_to_string(&mut schema_text)
    {
        return Ok(not_run(format!("cannot read UTF-8 schema: {e}")));
    }
    if schema_text.len() > super::super::scl::document::MAX_BYTES {
        return Ok(not_run("schema exceeds the input limit"));
    }
    let schema_doc = match roxmltree::Document::parse(&schema_text) {
        Ok(d) => d,
        Err(e) => return Ok(not_run(format!("invalid schema XML: {e}"))),
    };
    let root = doc.xml.root_element();
    let profile = format!(
        "{}{}{}",
        root.attribute("version").unwrap_or(""),
        root.attribute("revision").unwrap_or(""),
        root.attribute("release").unwrap_or("")
    );
    let schema_root = schema_doc.root_element();
    if schema_root.tag_name().name() != "schema"
        || schema_root.tag_name().namespace() != Some("http://www.w3.org/2001/XMLSchema")
        || schema_root.attribute("targetNamespace") != Some(SCL_NS)
        || schema_root.attribute("version") != Some(profile.as_str())
    {
        return Ok(not_run(format!(
            "schema must declare the SCL targetNamespace and version={profile:?}"
        )));
    }
    let mut input = tempfile::NamedTempFile::new()?;
    input.write_all(doc.source.as_bytes())?;
    input.flush()?;
    let mut command = Command::new(executable);
    command
        .arg("--nonet")
        .arg("--noout")
        .arg("--schema")
        .arg(&schema)
        .arg(input.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Ok(not_run(format!(
                "cannot start {}: {e}",
                executable.display()
            )));
        }
    };
    let mut stderr = child.stderr.take().expect("stderr configured as piped");
    let reader = thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let keep = n.min(65536usize.saturating_sub(output.len()));
                    output.extend_from_slice(&buffer[..keep]);
                }
            }
        }
        String::from_utf8_lossy(&output).into_owned()
    });
    let start = Instant::now();
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("XSD validation timed out".to_owned());
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("cannot wait for XSD validator: {e}"));
            }
        }
    };
    let detail = reader
        .join()
        .unwrap_or_else(|_| "validator stderr reader failed".into());
    match outcome {
        Err(e) => Ok(not_run(e)),
        Ok(status) => Ok(Check {
            name: "xsd".into(),
            status: match status.code() {
                Some(0) => Status::Pass,
                Some(3) => Status::Fail,
                _ => Status::NotRun,
            },
            detail: format!(
                "xmllint exit {:?}; schema={}; {}",
                status.code(),
                schema.display(),
                detail.trim()
            ),
        }),
    }
}
