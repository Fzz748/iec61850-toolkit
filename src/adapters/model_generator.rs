use super::runtime::{OwnedChild, hidden};
use crate::{Error, Result, cli::SimulationOptions};
use std::{
    fs::{self, File},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn generate(
    options: &SimulationOptions,
    input: &Path,
    output: &Path,
    ied: &str,
    ap: &str,
) -> Result<Vec<String>> {
    // Java's jar launcher does not accept Windows verbatim (\\?\) paths.
    let jar = std::path::absolute(&options.genconfig)?;
    if !jar.is_file() {
        return Err(Error::Environment(format!(
            "GENCONFIG_MISSING: {}",
            jar.display()
        )));
    }
    let stdout_path = output.with_extension("stdout");
    let stderr_path = output.with_extension("stderr");
    let mut command = Command::new(&options.java);
    command
        .args([
            "-Dfile.encoding=UTF-8",
            "-Dstdout.encoding=UTF-8",
            "-Dstderr.encoding=UTF-8",
            "-jar",
        ])
        .arg(jar)
        .arg(input)
        .args(["-ied", ied, "-ap", ap])
        .arg(output)
        .stdin(Stdio::null())
        .stdout(File::create(&stdout_path)?)
        .stderr(File::create(&stderr_path)?);
    hidden(&mut command);
    let mut child = OwnedChild(
        command
            .spawn()
            .map_err(|e| Error::Environment(format!("GENCONFIG_START: {e}")))?,
    );
    let deadline = Instant::now() + Duration::from_secs(options.backend.timeout);
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err(Error::Environment("GENCONFIG_TIMEOUT".into()));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = fs::read_to_string(stdout_path)?;
    let stderr = fs::read_to_string(stderr_path)?;
    // Upstream catches some parse errors without setting a failing exit code.
    if !status.success()
        || !stderr.trim().is_empty()
        || stdout.lines().any(|line| {
            line.contains("ERROR")
                || (line.contains("WARNING")
                    && line != "WARNING: No communication section found!"
                    && line != format!("WARNING: IED \"{ied}\" has no connected access point!"))
                || line.contains("Unknown default value")
        })
    {
        return Err(Error::Environment(format!(
            "GENCONFIG_FAILED: {status}; {stderr}; {stdout}"
        )));
    }
    let generated = fs::read_to_string(output)
        .map_err(|e| Error::Environment(format!("GENCONFIG_OUTPUT: {e}")))?;
    if !generated.starts_with("MODEL(") || !generated.trim_end().ends_with('}') {
        return Err(Error::Environment(
            "GENCONFIG_OUTPUT: incomplete model".into(),
        ));
    }
    Ok(stdout
        .lines()
        .filter(|line| line.contains("WARNING"))
        .map(str::to_owned)
        .collect())
}
