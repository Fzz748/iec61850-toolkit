//! Own one native process per invocation. Its stdin lifetime is the parent-death signal.
use super::cancellation::{self, Cancellation};
use crate::{Error, Result, cli::RuntimeOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub protocol_version: u32,
    pub event: String,
    pub data: Value,
}

pub struct Execution {
    pub data: Value,
    pub cancelled: bool,
}

pub(super) struct OwnedChild(pub Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.0.stdin.take();
        match self.0.try_wait() {
            Ok(Some(_)) => (),
            _ => {
                if let Err(error) = self.0.kill() {
                    eprintln!("cannot stop child process: {error}");
                }
                if let Err(error) = self.0.wait() {
                    eprintln!("cannot reap child process: {error}");
                }
            }
        }
    }
}

pub(super) fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

pub fn execute(
    options: &RuntimeOptions,
    request: &Value,
    events: Option<&Path>,
    duration: Option<Duration>,
) -> Result<Execution> {
    let _cancellation = Cancellation::install()?;
    let work = tempfile::tempdir()?;
    let request_path = work.path().join("request.json");
    fs::write(&request_path, serde_json::to_vec(request)?)?;
    let log_path = work.path().join("runtime.log");
    let log = File::create(&log_path)?;
    let mut event_file = events
        .map(|path| OpenOptions::new().write(true).create_new(true).open(path))
        .transpose()?;
    let mut command = Command::new(&options.runtime);
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        // Npcap's normal installation keeps its DLLs outside the standard DLL search path.
        let npcap = std::path::PathBuf::from(system_root).join("System32/Npcap");
        if npcap.is_dir() {
            let mut paths = vec![npcap];
            if let Some(path) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&path));
            }
            command.env(
                "PATH",
                std::env::join_paths(paths).map_err(|e| Error::Environment(e.to_string()))?,
            );
        }
    }
    command
        .arg("--request")
        .arg(&request_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(log);
    hidden(&mut command);
    let mut child = OwnedChild(command.spawn().map_err(|error| {
        Error::Environment(format!(
            "RUNTIME_START: {}: {error}",
            options.runtime.display()
        ))
    })?);
    let stdout = child.0.stdout.take().expect("piped stdout");
    // A rendezvous channel applies backpressure without accumulating captured packets in RAM.
    let (sender, receiver) = mpsc::sync_channel(0);
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let timeout = Duration::from_secs(options.timeout);
    let mut deadline = Instant::now() + timeout;
    let mut ready = false;
    let mut disconnected = false;
    let mut stopped = false;
    let mut result = None;
    let write_uncertainty = if request["action"] == "write" {
        "; if the write was sent, its outcome is unknown; it is not automatically retried"
    } else {
        ""
    };
    let outcome = (|| -> Result<Execution> {
        loop {
            if cancellation::requested() && !stopped {
                child.0.stdin.take();
                stopped = true;
                deadline = Instant::now() + STOP_TIMEOUT;
            }
            if Instant::now() >= deadline {
                return Err(Error::Environment(format!(
                    "{}{}",
                    if ready {
                        "RUNTIME_TIMEOUT: session did not stop by its deadline"
                    } else {
                        "RUNTIME_TIMEOUT: no completed startup/request before deadline"
                    },
                    write_uncertainty
                )));
            }
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(line) => {
                    let line = line?;
                    let event: Event = serde_json::from_str(&line).map_err(|e| {
                        Error::Environment(format!("RUNTIME_PROTOCOL: {e}: {line}"))
                    })?;
                    if event.protocol_version != 2 {
                        return Err(Error::Environment(
                            "RUNTIME_PROTOCOL: incompatible protocol version".into(),
                        ));
                    }
                    if !event.data.is_object() {
                        return Err(Error::Environment(
                            "RUNTIME_PROTOCOL: event data must be an object".into(),
                        ));
                    }
                    if let Some(file) = &mut event_file {
                        serde_json::to_writer(&mut *file, &event)?;
                        writeln!(file)?;
                        file.flush()?;
                    }
                    match event.event.as_str() {
                        "ready" if duration.is_some() && !ready && result.is_none() => {
                            ready = true;
                            if !stopped {
                                deadline =
                                    Instant::now() + duration.unwrap_or(timeout) + STOP_TIMEOUT;
                            }
                        }
                        "result" if result.is_none() && (duration.is_none() || ready) => {
                            result = Some(event.data);
                            deadline = Instant::now() + STOP_TIMEOUT;
                        }
                        "error" => {
                            return Err(Error::Environment(format!(
                                "RUNTIME_FAILURE: {}",
                                event.data
                            )));
                        }
                        "mms_write" | "mms_write_rejected"
                            if duration.is_some() && result.is_none() => {}
                        "update" | "goose" | "goose_timeout" if ready && result.is_none() => (),
                        _ => {
                            return Err(Error::Environment(format!(
                                "RUNTIME_PROTOCOL: unexpected {} event",
                                event.event
                            )));
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
            }
            if disconnected {
                if let Some(status) = child.0.try_wait()? {
                    let logs = fs::read_to_string(&log_path)?;
                    if !status.success() {
                        return Err(Error::Environment(format!(
                            "RUNTIME_EXIT: {status}; {logs}{write_uncertainty}"
                        )));
                    }
                    let mut data = result.take().ok_or_else(|| {
                        Error::Environment(format!(
                            "RUNTIME_PROTOCOL: process exited without a result; {logs}{write_uncertainty}"
                        ))
                    })?;
                    // The pinned Windows HAL has void send/filter APIs and only logs their errors.
                    // Preserve these failures instead of treating a successful child exit as success.
                    for line in logs.lines() {
                        let line = line.trim().to_ascii_lowercase();
                        if line.starts_with("error ")
                            || line.contains("packet filter failed")
                            || line.starts_with("open ethernet socket failed")
                            || line.starts_with("ethernet_getinterfacemacaddress: invalid")
                        {
                            return Err(Error::Environment(format!("RUNTIME_IO: {logs}")));
                        }
                    }
                    if !logs.trim().is_empty() {
                        data["backend_log"] = Value::String(logs);
                    }
                    return Ok(Execution {
                        data,
                        cancelled: stopped,
                    });
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    })();
    // Close the receiver first so the reader cannot remain blocked sending its last event.
    drop(receiver);
    drop(child);
    reader
        .join()
        .map_err(|_| Error::Environment("runtime reader thread panicked".into()))?;
    outcome
}
