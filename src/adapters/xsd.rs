use crate::{
    Result,
    report::{Check, Status},
    scl::SclDocument,
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
    // Schema version strings are descriptive (some include publication dates).
    // Let the XSD engine check the actual declarations instead of comparing labels.
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
                    output.extend_from_slice(&buffer[..n]);
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
