use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pass,
    Fail,
    NotRun,
    Incomplete,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub tool_version: &'static str,
    pub command: String,
    pub status: Status,
    pub scope: String,
    pub data: Value,
    pub checks: Vec<Check>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn new(command: &str, scope: &str, data: Value) -> Self {
        Self {
            schema_version: 1,
            tool_version: env!("CARGO_PKG_VERSION"),
            command: command.into(),
            status: Status::Pass,
            scope: scope.into(),
            data,
            checks: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self.status {
            Status::Pass => 0,
            Status::Fail => 1,
            Status::NotRun | Status::Incomplete => 3,
        }
    }
}
