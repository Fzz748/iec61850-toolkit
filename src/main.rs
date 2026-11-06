use clap::Parser;
use iec61850_toolkit::{
    app,
    cli::Cli,
    report::{Diagnostic, Report, Status},
};
use serde_json::Value;
use std::process::ExitCode;

fn failure(message: String, code: &str) -> Report {
    let mut report = Report::new("cli", "execution", Value::Null);
    report.status = Status::Fail;
    report.diagnostics.push(Diagnostic {
        code: code.into(),
        severity: "error".into(),
        message,
        path: String::new(),
        line: 0,
        column: 0,
    });
    report
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let json = args.iter().any(|a| a == "--json");
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if !error.use_stderr() {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            if json {
                let _ = app::print(&failure(error.to_string(), "CLI_ARGUMENT"), true);
            } else {
                let _ = error.print();
            }
            return ExitCode::from(2);
        }
    };
    let (report, code) = match app::run(&cli.command) {
        Ok(report) => {
            let code = report.exit_code();
            (report, code)
        }
        Err(error) => (failure(error.to_string(), "CLI_EXECUTION"), 2),
    };
    match app::print(&report, cli.json) {
        Ok(()) => ExitCode::from(code),
        Err(error) => {
            eprintln!("output error: {error}");
            ExitCode::from(2)
        }
    }
}
