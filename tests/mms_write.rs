use iec61850_toolkit::simulation::{SimulationSpec, validate_write_value};
use serde_json::{Value, json};
use std::{
    fs,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn specification() -> Value {
    serde_json::from_str(include_str!("../examples/mms-write.json")).unwrap()
}

fn cli(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_iec61850"))
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)));
    (output.status.code().unwrap(), report)
}

#[test]
fn write_contract_requires_scalar_and_explicit_unique_supported_references() {
    let mut value = specification();
    serde_json::from_value::<SimulationSpec>(value.clone())
        .unwrap()
        .validate(None)
        .unwrap();
    let first = value["mms"]["writable"][0].clone();
    value["mms"]["writable"].as_array_mut().unwrap().push(first);
    assert!(
        serde_json::from_value::<SimulationSpec>(value)
            .unwrap()
            .validate(None)
            .is_err()
    );
    let mut value = specification();
    value["mms"]["writable"][0]["fc"] = json!("MX");
    assert!(
        serde_json::from_value::<SimulationSpec>(value)
            .unwrap()
            .validate(None)
            .is_err()
    );
    for value in [json!(null), json!([]), json!({}), json!("bad\0string")] {
        assert!(validate_write_value(&value).is_err());
    }
    for value in [json!(false), json!(1.5), json!(-5), json!("text")] {
        validate_write_value(&value).unwrap();
    }
}

#[test]
fn missing_conflicting_or_nonscalar_values_fail_before_connecting() {
    let common = [
        "mms",
        "write",
        "--host",
        "127.0.0.1",
        "--reference",
        "DEMOLD0/GGIO1.Setpoint.setMag.f",
        "--fc",
        "SP",
        "--runtime",
        "__missing_backend__",
    ];
    let (code, _) = cli(&common);
    assert_eq!(code, 2);
    let (code, report) = cli(&[common.to_vec(), vec!["--value", "{}"]].concat());
    assert_eq!(code, 2);
    assert!(report.to_string().contains("JSON scalar"));
    let (code, _) = cli(&[
        common.to_vec(),
        vec!["--value", "1", "--value-file", "other.json"],
    ]
    .concat());
    assert_eq!(code, 2);
}

#[test]
fn malformed_write_fc_is_rejected_before_connecting() {
    let (code, report) = cli(&[
        "mms",
        "write",
        "--host",
        "127.0.0.1",
        "--reference",
        "DEMOLD0/GGIO1.Setpoint.setMag.f",
        "--fc",
        "SP-extra",
        "--value",
        "25.5",
        "--runtime",
        "__missing_backend__",
    ]);
    assert_eq!(code, 2);
    assert!(report.to_string().contains("invalid functional constraint"));
}

struct Server {
    child: Child,
    workspace: tempfile::TempDir,
    port: u16,
    backend: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while TcpListener::bind(("127.0.0.1", self.port)).is_err() {
            assert!(
                Instant::now() < deadline,
                "native listener survived CLI termination"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn events(path: &Path) -> Vec<Value> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!("cannot read events: {error}"),
    };
    bytes
        .split_inclusive(|b| *b == b'\n')
        .take_while(|line| line.ends_with(b"\n"))
        .map(|line| serde_json::from_slice(line).expect("complete event must be valid JSON"))
        .collect()
}

impl Server {
    fn start(writable: bool) -> Self {
        Self::start_with(writable, None)
    }

    fn start_with(writable: bool, interface: Option<&str>) -> Self {
        let backend = std::env::var("IEC61850_TEST_RUNTIME").expect("IEC61850_TEST_RUNTIME");
        let genconfig = std::env::var("IEC61850_TEST_GENCONFIG").expect("IEC61850_TEST_GENCONFIG");
        let workspace = tempfile::tempdir().unwrap();
        let guard = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = guard.local_addr().unwrap().port();
        let mut value = specification();
        value["mms"]["port"] = json!(port);
        if !writable {
            value["mms"].as_object_mut().unwrap().remove("writable");
        }
        let mut model = include_str!("../examples/mms-write.icd").to_owned();
        if let Some(interface) = interface {
            let target =
                json!({"ied":"DEMO","access_point":"AP1","ld_inst":"LD0","ln_class":"LLN0"});
            let patch = serde_json::from_value(json!({"schema_version":1,"operations":[
                {"op":"add_dataset","target":target,"name":"dsWrite","members":[{"ld_inst":"LD0","ln_class":"GGIO","ln_inst":"1","do_name":"Setpoint","da_name":"setMag.f","fc":"SP"}]},
                {"op":"add_goose","target":target,"name":"gcbWrite","dataset":"dsWrite","go_id":"WriteTest"}
            ]})).unwrap();
            model = iec61850_toolkit::scl::edit::prepare(&model, &patch).unwrap();
            value["goose"] = json!({"interface":interface,"controls":[{"ld_inst":"LD0","name":"gcbWrite","address":{
                "mac":"01-0C-CD-01-00-01","appid":4096,"vlan_id":0,"vlan_priority":4,"vlan_tagged":false,"min_time_ms":100,"max_time_ms":1000
            }}]});
        }
        fs::write(workspace.path().join("mms-write.icd"), model).unwrap();
        let spec = workspace.path().join("simulation.json");
        fs::write(&spec, serde_json::to_vec(&value).unwrap()).unwrap();
        let log = workspace.path().join("events.jsonl");
        let result = workspace.path().join("result.json");
        drop(guard);
        let child = Command::new(env!("CARGO_BIN_EXE_iec61850"))
            .args([
                "sim",
                "run",
                "--spec",
                spec.to_str().unwrap(),
                "--genconfig",
                &genconfig,
                "--runtime",
                &backend,
                "--duration",
                if interface.is_some() { "5" } else { "30" },
                "--events",
                log.to_str().unwrap(),
                "--json",
            ])
            .stdout(fs::File::create(&result).unwrap())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut server = Self {
            child,
            workspace,
            port,
            backend,
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        while !events(&log).iter().any(|v| v["event"] == "ready") {
            assert!(
                server.child.try_wait().unwrap().is_none(),
                "{}",
                fs::read_to_string(&result).unwrap()
            );
            assert!(Instant::now() < deadline, "no ready event");
            thread::sleep(Duration::from_millis(20));
        }
        server
    }

    fn request(&self, operation: &str, reference: &str, fc: &str, extra: &[&str]) -> (i32, Value) {
        let port = self.port.to_string();
        let args = [
            vec![
                "mms",
                operation,
                "--host",
                "127.0.0.1",
                "--port",
                &port,
                "--reference",
                reference,
                "--fc",
                fc,
                "--runtime",
                &self.backend,
            ],
            extra.to_vec(),
        ]
        .concat();
        cli(&args)
    }
}

#[test]
#[ignore = "real local MMS write/ack/readback; requires native dependencies"]
fn real_mms_write_roundtrips_scalars_and_emits_applied_events() {
    let server = Server::start(true);
    let cases = [
        ("DEMOLD0/GGIO1.Setpoint.setMag.f", "SP", "25.5", json!(25.5)),
        (
            "DEMOLD0/GGIO1.EnableSetting.setVal",
            "SP",
            "true",
            json!(true),
        ),
        (
            "DEMOLD0/GGIO1.CountSetting.setVal",
            "SP",
            "-17",
            json!("-17"),
        ),
    ];
    for (reference, fc, input, expected) in cases {
        let (code, written) = server.request("write", reference, fc, &["--value", input]);
        assert_eq!(code, 0, "{written}");
        assert_eq!(written["data"]["acknowledged"], true);
        let (code, read) = server.request("read", reference, fc, &[]);
        assert_eq!(code, 0, "{read}");
        assert_eq!(read["data"]["value"]["value"], expected);
    }
    let input = server.workspace.path().join("string.json");
    fs::write(&input, "\"Written by MMS\"").unwrap();
    let (code, written) = server.request(
        "write",
        "DEMOLD0/GGIO1.NamPlt.vendor",
        "DC",
        &["--value-file", input.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{written}");
    let (code, read) = server.request("read", "DEMOLD0/GGIO1.NamPlt.vendor", "DC", &[]);
    assert_eq!(code, 0, "{read}");
    assert_eq!(read["data"]["value"]["value"], "Written by MMS");
    let writes: Vec<_> = events(&server.workspace.path().join("events.jsonl"))
        .into_iter()
        .filter(|v| v["event"] == "mms_write")
        .collect();
    assert_eq!(writes.len(), 4, "{writes:?}");
    assert!(
        writes
            .iter()
            .all(|v| v["protocol_version"] == 2 && v["data"]["peer"].is_string())
    );
}

#[test]
#[ignore = "real localhost rejection and unchanged-value checks; requires native dependencies"]
fn real_mms_write_denials_and_type_errors_do_not_change_values() {
    let server = Server::start(true);
    for (reference, fc, input) in [
        ("DEMOLD0/GGIO1.EnableSetting.setVal", "SP", "1"),
        ("DEMOLD0/GGIO1.CountSetting.setVal", "SP", "2147483648"),
        ("DEMOLD0/GGIO1.Setpoint.setMag.f", "SP", "true"),
    ] {
        let (_, before) = server.request("read", reference, fc, &[]);
        let (code, failed) = server.request("write", reference, fc, &["--value", input]);
        assert_eq!(code, 2, "{failed}");
        assert!(failed.to_string().contains("MMS_WRITE_VALUE"));
        let (code, after) = server.request("read", reference, fc, &[]);
        assert_eq!(code, 0, "{after}");
        assert_eq!(after["data"]["value"], before["data"]["value"]);
    }
    let (code, denied) = server.request(
        "write",
        "DEMOLD0/LLN0.NamPlt.vendor",
        "DC",
        &["--value", "\"denied\""],
    );
    assert_eq!(code, 2, "{denied}");
    assert!(denied.to_string().contains("MMS_SERVICE"));
    assert!(
        !events(&server.workspace.path().join("events.jsonl"))
            .iter()
            .any(|v| v["event"] == "mms_write")
    );
}

#[test]
#[ignore = "real localhost compatibility check for configurations without writable; requires native dependencies"]
fn real_mms_write_is_denied_without_explicit_server_configuration() {
    let server = Server::start(false);
    let (code, denied) = server.request(
        "write",
        "DEMOLD0/GGIO1.Setpoint.setMag.f",
        "SP",
        &["--value", "25.5"],
    );
    assert_eq!(code, 2, "{denied}");
    assert!(denied.to_string().contains("MMS_SERVICE"));
    let (code, read) = server.request("read", "DEMOLD0/GGIO1.Setpoint.setMag.f", "SP", &[]);
    assert_eq!(code, 0, "{read}");
    assert_eq!(read["data"]["value"]["value"], 10.0);
}

struct Receiver(Child);
impl Drop for Receiver {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
            self.0.wait().unwrap();
        }
    }
}

#[test]
#[ignore = "requires an explicitly authorized IEC61850_TEST_INTERFACE; sends GOOSE for at most five seconds"]
fn real_mms_write_updates_the_shared_goose_dataset() {
    let interface = std::env::var("IEC61850_TEST_INTERFACE").expect("authorized interface");
    let backend = std::env::var("IEC61850_TEST_RUNTIME").expect("IEC61850_TEST_RUNTIME");
    let dir = tempfile::tempdir().unwrap();
    let specification = dir.path().join("subscriber.json");
    fs::write(
        &specification,
        serde_json::to_vec(&json!({"schema_version":1,"interface":interface,
        "gocb_ref":"DEMOLD0/LLN0$GO$gcbWrite","mac":"01-0C-CD-01-00-01","appid":4096,
        "dataset":"DEMOLD0/LLN0$dsWrite","conf_rev":1}))
        .unwrap(),
    )
    .unwrap();
    let log = dir.path().join("events.jsonl");
    let mut receiver = Receiver(
        Command::new(env!("CARGO_BIN_EXE_iec61850"))
            .args([
                "goose",
                "subscribe",
                "--spec",
                specification.to_str().unwrap(),
                "--runtime",
                &backend,
                "--duration",
                "10",
                "--events",
                log.to_str().unwrap(),
                "--json",
            ])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !events(&log).iter().any(|v| v["event"] == "ready") {
        assert!(receiver.0.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let server = Server::start_with(true, Some(&interface));
    let deadline = Instant::now() + Duration::from_secs(4);
    while !events(&log)
        .iter()
        .any(|v| v["event"] == "goose" && v["data"]["values"]["value"][0]["value"] == 10.0)
    {
        assert!(Instant::now() < deadline, "initial GOOSE value absent");
        thread::sleep(Duration::from_millis(20));
    }
    let (code, written) = server.request(
        "write",
        "DEMOLD0/GGIO1.Setpoint.setMag.f",
        "SP",
        &["--value", "33.25"],
    );
    assert_eq!(code, 0, "{written}");
    loop {
        let records = events(&log);
        if let Some(frame) = records
            .iter()
            .find(|v| v["event"] == "goose" && v["data"]["values"]["value"][0]["value"] == 33.25)
        {
            assert_eq!(frame["data"]["valid"], true);
            assert!(frame["data"]["st_num"].as_u64().unwrap() > 1);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "MMS write did not reach the GOOSE DataSet"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
