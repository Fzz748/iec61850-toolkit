use iec61850_toolkit::{
    scl::{SclDocument, generate, runtime},
    simulation::{SimulationSpec, SubscriptionSpec},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn source() -> String {
    generate::create(&serde_json::from_str(include_str!("../examples/device.json")).unwrap())
        .unwrap()
}

fn spec() -> Value {
    serde_json::from_str(include_str!("../examples/simulation.json")).unwrap()
}

fn parse(value: Value) -> SimulationSpec {
    serde_json::from_value(value).unwrap()
}

#[test]
fn scenario_checks_order_duplicates_duration_and_unknown_fields() {
    let value = spec();
    parse(value.clone()).validate(Some(2000)).unwrap();
    assert!(parse(value.clone()).validate(Some(1000)).is_err());
    let mut duplicate = value.clone();
    duplicate["initial"]
        .as_array_mut()
        .unwrap()
        .push(value["initial"][0].clone());
    assert!(parse(duplicate).validate(None).is_err());
    let mut unordered = value.clone();
    unordered["timeline"]
        .as_array_mut()
        .unwrap()
        .push(value["timeline"][0].clone());
    assert!(parse(unordered).validate(None).is_err());
    let mut unknown = value;
    unknown["ignore_errors"] = json!(true);
    assert!(serde_json::from_value::<SimulationSpec>(unknown).is_err());
}

#[test]
fn runtime_projection_requires_unambiguous_model_and_matching_hash() {
    let xml = source();
    let doc = SclDocument::parse(&xml).unwrap();
    runtime::project(&doc, &parse(spec())).unwrap();
    let mut stale = spec();
    stale["source_sha256"] = json!("stale");
    assert!(runtime::project(&doc, &parse(stale)).is_err());
    let mut missing = spec();
    missing["access_point"] = json!("missing");
    assert!(runtime::project(&doc, &parse(missing)).is_err());
}

#[test]
fn goose_never_invents_network_parameters() {
    let patch = serde_json::from_str(include_str!("../examples/add-goose.json")).unwrap();
    let xml = iec61850_toolkit::scl::edit::prepare(&source(), &patch).unwrap();
    let doc = SclDocument::parse(&xml).unwrap();
    let mut value = spec();
    value["goose"] =
        json!({"interface":"test-interface","controls":[{"ld_inst":"LD0","name":"gcbStatus"}]});
    assert!(runtime::project(&doc, &parse(value.clone())).is_err());
    value["goose"]["controls"][0]["address"] = json!({"mac":"01-0C-CD-01-00-01","appid":4096,"vlan_id":0,"vlan_priority":4,"vlan_tagged":true,"min_time_ms":4,"max_time_ms":1000});
    let parsed = parse(value.clone());
    parsed.validate(None).unwrap();
    let projected = runtime::project(&doc, &parsed).unwrap();
    assert_eq!(projected["controls"][0]["address_source"], "override");
    value["goose"]["controls"][0]["address"]["mac"] = json!("00-00-00-00-00-01");
    assert!(parse(value).validate(None).is_err());
}

#[test]
fn subscription_rejects_unicast_and_out_of_range_appid() {
    let mut value = json!({"schema_version":1,"interface":"test","gocb_ref":"DEMOLD0/LLN0$GO$gcbStatus","mac":"01-0C-CD-01-00-01","appid":4096,"dataset":"DEMOLD0/LLN0$dsStatus","conf_rev":1});
    serde_json::from_value::<SubscriptionSpec>(value.clone())
        .unwrap()
        .validate()
        .unwrap();
    value["appid"] = json!(65535);
    assert!(
        serde_json::from_value::<SubscriptionSpec>(value)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn goose_projection_resolves_scl_addresses_and_keeps_fcda_order() {
    let patch = serde_json::from_str(include_str!("../examples/add-goose.json")).unwrap();
    let xml = iec61850_toolkit::scl::edit::prepare(&source(), &patch).unwrap();
    let communication = r#"<Communication><SubNetwork name="lab"><ConnectedAP iedName="DEMO" apName="AP1"><GSE ldInst="LD0" cbName="gcbStatus"><Address><P type="MAC-Address">01-0C-CD-01-00-01</P><P type="APPID">1000</P><P type="VLAN-ID">00A</P><P type="VLAN-PRIORITY">4</P></Address><MinTime unit="s" multiplier="m">100</MinTime><MaxTime unit="s">1</MaxTime></GSE></ConnectedAP></SubNetwork></Communication>"#;
    let xml = xml.replacen("  <IED", &format!("{communication}\n  <IED"), 1);
    let doc = SclDocument::parse(&xml).unwrap();
    let mut value = spec();
    value["goose"] = json!({"interface":"test","controls":[{"ld_inst":"LD0","name":"gcbStatus"}]});
    let plan = runtime::project(&doc, &parse(value)).unwrap();
    assert_eq!(plan["controls"][0]["address"]["appid"], 4096);
    assert_eq!(plan["controls"][0]["address"]["vlan_id"], 10);
    assert_eq!(plan["controls"][0]["address"]["max_time_ms"], 1000);
    assert_eq!(
        plan["controls"][0]["members"][0]["variable"],
        "GGIO1$ST$Ind1$stVal"
    );
    assert_eq!(
        plan["controls"][0]["members"][1]["variable"],
        "GGIO1$MX$AnIn1$mag$f"
    );
}

fn cli(args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_iec61850"))
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)));
    (output.status.code().unwrap(), value)
}

#[test]
fn unavailable_backend_is_an_execution_failure() {
    let (code, result) = cli(&["runtime", "--runtime", "__missing_iec61850_runtime__"]);
    assert_eq!(code, 2, "{result}");
    assert_eq!(result["status"], "fail");
    assert!(
        result["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("RUNTIME_START")
    );
}

struct Session(Child);
impl Drop for Session {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
            self.0.wait().unwrap();
        }
    }
}

fn wait_ready(events: &Path, session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(content) = fs::read_to_string(events) {
            if content
                .lines()
                .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                .any(|e| e["event"] == "ready")
            {
                return;
            }
        }
        assert!(
            session.0.try_wait().unwrap().is_none(),
            "simulation exited before ready"
        );
        assert!(Instant::now() < deadline, "simulation startup timeout");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "requires IEC61850_TEST_RUNTIME and IEC61850_TEST_GENCONFIG; real localhost MMS"]
fn real_mms_simulation_changes_values_and_releases_listener() {
    let backend = std::env::var("IEC61850_TEST_RUNTIME").expect("IEC61850_TEST_RUNTIME");
    let generator = std::env::var("IEC61850_TEST_GENCONFIG").expect("IEC61850_TEST_GENCONFIG");
    let dir = tempfile::tempdir().unwrap();
    let port_guard = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_guard.local_addr().unwrap().port();
    let mut value = spec();
    value["mms"]["port"] = json!(port);
    value["timeline"][0]["at_ms"] = json!(1800);
    fs::write(dir.path().join("device.icd"), source()).unwrap();
    let specification = dir.path().join("simulation.json");
    fs::write(&specification, serde_json::to_vec(&value).unwrap()).unwrap();
    // Check must succeed even when the requested listen port is already bound.
    let common = [
        "--spec",
        specification.to_str().unwrap(),
        "--genconfig",
        &generator,
        "--runtime",
        &backend,
    ];
    let (code, checked) = cli(&[vec!["sim", "check"], common.to_vec()].concat());
    assert_eq!(code, 0, "{checked}");
    assert_eq!(checked["data"]["network_started"], false);
    drop(port_guard);
    let events = dir.path().join("events.jsonl");
    let mut session = Session(
        Command::new(env!("CARGO_BIN_EXE_iec61850"))
            .args(["sim", "run"])
            .args(common)
            .args([
                "--duration",
                "4",
                "--events",
                events.to_str().unwrap(),
                "--json",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    wait_ready(&events, &mut session);
    let port_text = port.to_string();
    let client = [
        "--host",
        "127.0.0.1",
        "--port",
        &port_text,
        "--runtime",
        &backend,
    ];
    let (code, read) = cli(&[
        vec!["mms", "read"],
        client.to_vec(),
        vec!["--reference", "DEMOLD0/GGIO1.AnIn1.mag.f", "--fc", "MX"],
    ]
    .concat());
    assert_eq!(code, 0, "{read}");
    assert_eq!(read["data"]["value"]["value"], 10.0);
    let (code, browsed) = cli(&[vec!["mms", "browse"], client.to_vec()].concat());
    assert_eq!(code, 0, "{browsed}");
    assert_eq!(browsed["data"]["value"][0]["logical_device"], "DEMOLD0");
    let update_deadline = Instant::now() + Duration::from_secs(4);
    while !fs::read_to_string(&events)
        .unwrap()
        .contains("\"event\":\"update\"")
    {
        assert!(
            Instant::now() < update_deadline,
            "missing scenario update event"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let (code, read) = cli(&[
        vec!["mms", "read"],
        client.to_vec(),
        vec!["--reference", "DEMOLD0/GGIO1.AnIn1.mag.f", "--fc", "MX"],
    ]
    .concat());
    assert_eq!(code, 0, "{read}");
    assert_eq!(read["data"]["value"]["value"], 25.5);
    let deadline = Instant::now() + Duration::from_secs(8);
    while session.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    assert!(session.0.wait().unwrap().success());
    assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    assert!(
        fs::read_to_string(events)
            .unwrap()
            .contains("\"event\":\"update\"")
    );
}

#[test]
#[ignore = "requires native dependencies and an explicitly authorized IEC61850_TEST_INTERFACE; sends real GOOSE for five seconds"]
fn real_goose_publisher_and_subscriber_preserve_order_and_state_changes() {
    goose_session(false);
}

#[test]
#[ignore = "requires an explicitly authorized IEC61850_TEST_INTERFACE; combined MMS and five-second GOOSE simulation"]
fn real_goose_and_mms_share_values_and_stop_together() {
    goose_session(true);
}

fn goose_session(with_mms: bool) {
    let backend = std::env::var("IEC61850_TEST_RUNTIME").expect("IEC61850_TEST_RUNTIME");
    let generator = std::env::var("IEC61850_TEST_GENCONFIG").expect("IEC61850_TEST_GENCONFIG");
    let interface =
        std::env::var("IEC61850_TEST_INTERFACE").expect("explicit authorized interface");
    let dir = tempfile::tempdir().unwrap();
    let patch = serde_json::from_str(include_str!("../examples/add-goose.json")).unwrap();
    let model = iec61850_toolkit::scl::edit::prepare(&source(), &patch).unwrap();
    fs::write(dir.path().join("device.icd"), model).unwrap();
    let mut simulation = spec();
    simulation["mms"] = Value::Null;
    let port_guard = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = port_guard.local_addr().unwrap().port();
    if with_mms {
        simulation["mms"] = json!({"listen":"127.0.0.1","port":port});
    }
    drop(port_guard);
    simulation["goose"] = json!({"interface":interface,"controls":[{"ld_inst":"LD0","name":"gcbStatus","address":{
        "mac":"01-0C-CD-01-00-01","appid":4096,"vlan_id":0,"vlan_priority":4,"vlan_tagged":false,"min_time_ms":100,"max_time_ms":1000
    }}]});
    let publisher_spec = dir.path().join("publisher.json");
    fs::write(&publisher_spec, serde_json::to_vec(&simulation).unwrap()).unwrap();
    let subscription = json!({"schema_version":1,"interface":interface,"gocb_ref":"DEMOLD0/LLN0$GO$gcbStatus",
        "mac":"01-0C-CD-01-00-01","appid":4096,"dataset":"DEMOLD0/LLN0$dsStatus","conf_rev":1});
    let receiver_spec = dir.path().join("subscriber.json");
    fs::write(&receiver_spec, serde_json::to_vec(&subscription).unwrap()).unwrap();
    let events = dir.path().join("received.jsonl");
    let receiver_result = dir.path().join("receiver-result.json");
    let mut receiver = Session(
        Command::new(env!("CARGO_BIN_EXE_iec61850"))
            .args([
                "goose",
                "subscribe",
                "--spec",
                receiver_spec.to_str().unwrap(),
                "--runtime",
                &backend,
                "--duration",
                "6",
                "--events",
                events.to_str().unwrap(),
                "--json",
            ])
            .stdout(fs::File::create(&receiver_result).unwrap())
            .spawn()
            .unwrap(),
    );
    wait_ready(&events, &mut receiver);
    let publisher_events = dir.path().join("publisher-events.jsonl");
    let publisher_result = dir.path().join("publisher-result.json");
    let mut publisher = Session(
        Command::new(env!("CARGO_BIN_EXE_iec61850"))
            .args([
                "sim",
                "run",
                "--spec",
                publisher_spec.to_str().unwrap(),
                "--genconfig",
                &generator,
                "--runtime",
                &backend,
                "--duration",
                "5",
                "--events",
                publisher_events.to_str().unwrap(),
                "--json",
            ])
            .stdout(fs::File::create(&publisher_result).unwrap())
            .spawn()
            .unwrap(),
    );
    wait_ready(&publisher_events, &mut publisher);
    if with_mms {
        thread::sleep(Duration::from_millis(1500));
        let port_text = port.to_string();
        let (code, read) = cli(&[
            "mms",
            "read",
            "--host",
            "127.0.0.1",
            "--port",
            &port_text,
            "--runtime",
            &backend,
            "--reference",
            "DEMOLD0/GGIO1.AnIn1.mag.f",
            "--fc",
            "MX",
        ]);
        assert_eq!(code, 0, "{read}");
        assert_eq!(read["data"]["value"]["value"], 25.5);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while receiver.0.try_wait().unwrap().is_none() || publisher.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(20));
    }
    let published: Value = serde_json::from_slice(&fs::read(publisher_result).unwrap()).unwrap();
    assert!(publisher.0.wait().unwrap().success(), "{published}");
    if with_mms {
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
    let result: Value = serde_json::from_slice(&fs::read(&receiver_result).unwrap()).unwrap();
    assert!(receiver.0.wait().unwrap().success(), "{result}");
    let records: Vec<Value> = fs::read_to_string(&events)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let frames: Vec<_> = records
        .iter()
        .filter(|r| r["event"] == "goose")
        .map(|r| &r["data"])
        .collect();
    assert!(frames.len() > 2, "{result}");
    assert!(frames.iter().all(|f| f["valid"] == true));
    let old = frames
        .iter()
        .find(|f| {
            f["values"]["value"][0]["value"] == false && f["values"]["value"][1]["value"] == 10.0
        })
        .expect("initial values in DataSet order");
    let new = frames
        .iter()
        .find(|f| {
            f["values"]["value"][0]["value"] == true && f["values"]["value"][1]["value"] == 25.5
        })
        .expect("updated values in DataSet order");
    assert!(new["st_num"].as_u64().unwrap() > old["st_num"].as_u64().unwrap());
    assert!(frames.iter().any(|f| f["sq_num"].as_u64().unwrap() > 0));
}

#[test]
#[ignore = "requires native dependencies; verifies real localhost failures without sending to devices"]
fn real_mms_rejects_bad_values_busy_ports_and_preserves_event_files() {
    let backend = std::env::var("IEC61850_TEST_RUNTIME").expect("IEC61850_TEST_RUNTIME");
    let generator = std::env::var("IEC61850_TEST_GENCONFIG").expect("IEC61850_TEST_GENCONFIG");
    let dir = tempfile::tempdir().unwrap();
    let guard = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = guard.local_addr().unwrap().port();
    let mut value = spec();
    value["mms"]["port"] = json!(port);
    let model = source();
    fs::write(dir.path().join("device.icd"), &model).unwrap();
    let specification = dir.path().join("scenario.json");
    let args = [
        "sim",
        "run",
        "--spec",
        specification.to_str().unwrap(),
        "--genconfig",
        &generator,
        "--runtime",
        &backend,
        "--duration",
        "2",
    ];
    value["timeline"][0]["values"][0]["value"] = json!("wrong boolean");
    fs::write(&specification, serde_json::to_vec(&value).unwrap()).unwrap();
    let (code, invalid) = cli(&args);
    assert_eq!(code, 2, "{invalid}");
    assert!(
        invalid["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("MODEL_VALUE"),
        "{invalid}"
    );
    value["timeline"][0]["values"][0]["value"] = json!(true);
    fs::write(&specification, serde_json::to_vec(&value).unwrap()).unwrap();
    let (code, busy) = cli(&args);
    assert_eq!(code, 2, "{busy}");
    assert!(
        busy["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("MMS_LISTEN"),
        "{busy}"
    );
    let existing = dir.path().join("keep.jsonl");
    fs::write(&existing, "user content").unwrap();
    let (code, _) = cli(&[args.to_vec(), vec!["--events", existing.to_str().unwrap()]].concat());
    assert_eq!(code, 2);
    assert_eq!(fs::read_to_string(existing).unwrap(), "user content");
    assert_eq!(
        fs::read_to_string(dir.path().join("device.icd")).unwrap(),
        model
    );
}

#[test]
#[ignore = "requires native dependencies; checks owned child cleanup after parent termination"]
fn real_mms_runtime_stops_when_its_cli_parent_dies() {
    let backend = std::env::var("IEC61850_TEST_RUNTIME").expect("IEC61850_TEST_RUNTIME");
    let generator = std::env::var("IEC61850_TEST_GENCONFIG").expect("IEC61850_TEST_GENCONFIG");
    let dir = tempfile::tempdir().unwrap();
    let guard = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = guard.local_addr().unwrap().port();
    let mut value = spec();
    value["mms"]["port"] = json!(port);
    fs::write(dir.path().join("device.icd"), source()).unwrap();
    let specification = dir.path().join("scenario.json");
    fs::write(&specification, serde_json::to_vec(&value).unwrap()).unwrap();
    let events = dir.path().join("events.jsonl");
    drop(guard);
    let mut session = Session(
        Command::new(env!("CARGO_BIN_EXE_iec61850"))
            .args([
                "sim",
                "run",
                "--spec",
                specification.to_str().unwrap(),
                "--genconfig",
                &generator,
                "--runtime",
                &backend,
                "--duration",
                "10",
                "--events",
                events.to_str().unwrap(),
                "--json",
            ])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_ready(&events, &mut session);
    session.0.kill().unwrap();
    session.0.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native child retained its listener after parent exit"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
