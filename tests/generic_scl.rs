use iec61850_toolkit::scl::{
    SclDocument,
    edit::{self, Patch},
    generate, validate,
};
use serde_json::{Value, json};

fn source() -> String {
    generate::create(&serde_json::from_str(include_str!("../examples/device.json")).unwrap())
        .unwrap()
}

fn target() -> Value {
    json!({"ied":"DEMO","access_point":"AP1","ld_inst":"LD0","ln_class":"LLN0"})
}

fn patch(operations: Value) -> Patch {
    serde_json::from_value(json!({"schema_version":1,"operations":operations})).unwrap()
}

#[test]
fn ordinary_edits_preserve_schema_labels_without_a_version_whitelist() {
    for attributes in [
        "version=\"2003\"",
        "version=\"2007\" revision=\"B\"",
        "version=\"2007\" revision=\"B\" release=\"4\"",
        "version=\"2007\" revision=\"C\" release=\"5\"",
        "",
    ] {
        let input = source().replace("version=\"2007\" revision=\"B\" release=\"4\"", attributes);
        let edited = edit::prepare(
            &input,
            &patch(json!([{"op":"set_description","target":target(),"description":"updated"}])),
        )
        .unwrap();
        assert_eq!(edited.replacen(" desc=\"updated\"", "", 1), input);
    }
}

#[test]
fn reference_resolution_follows_sdo_da_bda_paths_in_both_fcda_forms() {
    let input = source().replace("<LNodeType id=\"TK_GGIO\" lnClass=\"GGIO\">", "<LNodeType id=\"TK_GGIO\" lnClass=\"GGIO\"><DO name=\"PhV\" type=\"TestWYE\"/>")
        .replace("</DataTypeTemplates>", "<DOType id=\"TestWYE\" cdc=\"WYE\"><SDO name=\"phsA\" type=\"TestCMV\"/></DOType><DOType id=\"TestCMV\" cdc=\"CMV\"><DA name=\"cVal\" bType=\"Struct\" type=\"TestVector\" fc=\"MX\"/></DOType><DAType id=\"TestVector\"><BDA name=\"mag\" bType=\"Struct\" type=\"TK_ANALOG\"/></DAType></DataTypeTemplates>");
    let input = edit::prepare(&input, &patch(json!([{"op":"add_dataset","target":target(),"name":"dsVector","members":[
        {"ld_inst":"LD0","ln_class":"GGIO","ln_inst":"1","do_name":"PhV","da_name":"phsA.cVal.mag.f","fc":"MX"},
        {"ld_inst":"LD0","ln_class":"GGIO","ln_inst":"1","do_name":"PhV.phsA","da_name":"cVal.mag.f","fc":"MX"}
    ]}]))).unwrap();
    let errors = validate::references(&SclDocument::parse(&input).unwrap());
    assert!(errors.is_empty(), "{errors:?}");
    let invalid = input.replace("phsA.cVal.mag.f", "phsA.cVal.missing.f");
    assert!(
        validate::references(&SclDocument::parse(&invalid).unwrap())
            .iter()
            .any(|d| d.code == "SCL_FCDA_REFERENCE")
    );
}

#[test]
fn dataset_reordering_keeps_metadata_and_updates_report_revisions() {
    let mut add: Value = serde_json::from_str(include_str!("../examples/add-goose.json")).unwrap();
    add["operations"].as_array_mut().unwrap().pop();
    let input = edit::prepare(&source(), &serde_json::from_value(add.clone()).unwrap()).unwrap()
        .replace("<DataSet name=\"dsStatus\">", "<DataSet name=\"dsStatus\" desc=\"keep me\" xmlns:v=\"urn:vendor\" v:flag=\"yes\"><Private type=\"vendor\"><v:settings>keep exactly</v:settings></Private><!-- keep comment -->")
        .replace("</DataSet>", "</DataSet><ReportControl name=\"rcb\" datSet=\"dsStatus\" confRev=\"7\"/>");
    let operation = &mut add["operations"][0];
    operation["op"] = json!("replace_dataset");
    operation["members"].as_array_mut().unwrap().reverse();
    let edited = edit::prepare(&input, &serde_json::from_value(add).unwrap()).unwrap();
    assert!(edited.contains("desc=\"keep me\" xmlns:v=\"urn:vendor\" v:flag=\"yes\""));
    assert!(edited.contains("<Private type=\"vendor\"><v:settings>keep exactly</v:settings></Private><!-- keep comment -->"));
    assert!(edited.contains("confRev=\"8\""));
    assert!(edited.find("doName=\"AnIn1\"").unwrap() < edited.find("doName=\"Ind1\"").unwrap());
}

#[test]
fn patch_operation_count_is_not_artificially_capped() {
    let operations: Vec<_> = (0..257).map(|n|json!({"op":"set_description","target":target(),"description":format!("update {n}")})).collect();
    let edited = edit::prepare(&source(), &patch(json!(operations))).unwrap();
    assert!(edited.contains("desc=\"update 256\""));
}
