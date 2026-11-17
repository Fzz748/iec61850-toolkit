use crate::{
    Error, Result,
    adapters::{model_generator, runtime},
    cli::{Command, GooseCommand, MmsCommand, RuntimeOptions, SimCommand, SimulationOptions},
    report::{Check, Report, Status},
    scl::{SclDocument, runtime::project},
    simulation::{SimulationSpec, SubscriptionSpec, nonempty, validate_write_value},
};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};

fn report(
    command: &str,
    options: &RuntimeOptions,
    request: Value,
    events: Option<&Path>,
    duration: Option<u64>,
) -> Result<Report> {
    let execution = runtime::execute(options, &request, events, duration.map(Duration::from_secs))?;
    let mut report = Report::new(command, "runtime_operation", execution.data);
    if execution.cancelled {
        report.status = Status::Incomplete;
    } else if report.data["validation_failed"] == true {
        report.status = Status::Fail;
    } else if report.data["observed"] == false {
        report.status = Status::Incomplete;
    }
    if let Some(path) = events {
        report.data["events"] = json!(path);
    }
    Ok(report)
}

fn simulate(
    options: &SimulationOptions,
    duration: Option<u32>,
    events: Option<&Path>,
) -> Result<Report> {
    let spec_path = options.spec.canonicalize()?;
    let spec: SimulationSpec = serde_json::from_str(&super::read_text(&spec_path)?)?;
    spec.validate(duration.map(|s| u64::from(s) * 1000))?;
    let input = spec_path
        .parent()
        .expect("absolute spec path")
        .join(&spec.scl);
    let source = super::read_text(&input)?;
    let doc = SclDocument::parse(&source)?;
    let projection = project(&doc, &spec)?;
    let work = tempfile::tempdir()?;
    let snapshot = work.path().join("source.icd");
    let model = work.path().join("model.cfg");
    fs::write(&snapshot, &source)?;
    let generator_warnings =
        model_generator::generate(options, &snapshot, &model, &spec.ied, &spec.access_point)?;
    let request = json!({"protocol_version":2, "action":if duration.is_some() {"simulate"} else {"check"},
        "model":model, "ied":spec.ied, "mms":spec.mms,
        "interface":spec.goose.as_ref().map(|g| &g.interface), "controls":projection["controls"],
        "initial":spec.initial, "timeline":spec.timeline, "duration_ms":duration.map(|d| u64::from(d)*1000)});
    let mut result = report(
        if duration.is_some() {
            "sim.run"
        } else {
            "sim.check"
        },
        &options.backend,
        request,
        events,
        duration.map(u64::from),
    )?;
    result.data["model"] = projection;
    result.data["generator_warnings"] = json!(generator_warnings);
    result.checks.push(Check { name:"runtime_model".into(), status:Status::Pass, detail:"SCL references, selected runtime controls, value types and model loading checked; excludes XSD/NSD and device behavior".into() });
    Ok(result)
}

pub fn run(command: &Command) -> Result<Report> {
    match command {
        Command::Sim {
            command: SimCommand::Check { options },
        } => simulate(options, None, None),
        Command::Sim {
            command:
                SimCommand::Run {
                    options,
                    duration,
                    events,
                },
        } => simulate(options, Some(*duration), events.as_deref()),
        Command::Runtime { runtime } => report(
            "runtime",
            runtime,
            json!({"protocol_version":2,"action":"capabilities"}),
            None,
            None,
        ),
        Command::Mms { command } => {
            let (options, action, reference, fc, write_value) = match command {
                MmsCommand::Browse { options } => (options, "browse", None, None, Value::Null),
                MmsCommand::Read {
                    options,
                    reference,
                    fc,
                } => {
                    nonempty(reference, "reference")?;
                    nonempty(fc, "fc")?;
                    (options, "read", Some(reference), Some(fc), Value::Null)
                }
                MmsCommand::Write {
                    options,
                    reference,
                    fc,
                    value,
                    value_file,
                } => {
                    nonempty(reference, "reference")?;
                    nonempty(fc, "fc")?;
                    if fc == "CO" {
                        return Err(Error::Input("FC=CO requires control/SBO/Operate services; mms write is a data-write operation".into()));
                    }
                    if !matches!(
                        fc.as_str(),
                        "ST" | "MX"
                            | "SP"
                            | "SV"
                            | "CF"
                            | "DC"
                            | "SG"
                            | "SE"
                            | "SR"
                            | "OR"
                            | "BL"
                            | "EX"
                            | "US"
                            | "MS"
                            | "RP"
                            | "BR"
                            | "LG"
                            | "GO"
                    ) {
                        return Err(Error::Input(
                            "invalid functional constraint for mms write".into(),
                        ));
                    }
                    let input = match (value, value_file) {
                        (Some(text), None) => text.clone(),
                        (None, Some(path)) => super::read_text(path)?,
                        _ => {
                            return Err(Error::Input(
                                "provide exactly one of --value and --value-file".into(),
                            ));
                        }
                    };
                    let parsed: Value = serde_json::from_str(&input)?;
                    validate_write_value(&parsed)?;
                    (options, "write", Some(reference), Some(fc), parsed)
                }
            };
            nonempty(&options.host, "host")?;
            report(
                &format!("mms.{action}"),
                &options.backend,
                json!({"protocol_version":2,"action":action,
                "host":options.host,"port":options.port,"reference":reference,"fc":fc,"value":write_value,"timeout_ms":options.backend.timeout.saturating_mul(1000)}),
                None,
                None,
            )
        }
        Command::Goose {
            command:
                GooseCommand::Subscribe {
                    spec,
                    duration,
                    events,
                    backend,
                },
        } => {
            let spec: SubscriptionSpec = serde_json::from_str(&super::read_text(spec)?)?;
            spec.validate()?;
            report(
                "goose.subscribe",
                backend,
                json!({"protocol_version":2,"action":"subscribe","subscription":spec,"duration_ms":u64::from(*duration)*1000}),
                events.as_deref(),
                Some(u64::from(*duration)),
            )
        }
        _ => Err(Error::Input("not a runtime command".into())),
    }
}
