use iec61850_toolkit::scl::{
    SclDocument, diff,
    edit::{self, Patch},
    generate::{self, DeviceSpec},
    query, validate,
};
use serde_json::{Value, json};

fn source() -> String {
    let spec: DeviceSpec = serde_json::from_str(include_str!("../examples/device.json")).unwrap();
    generate::create(&spec).unwrap()
}

fn target() -> Value {
    json!({"ied":"DEMO", "access_point":"AP1", "ld_inst":"LD0", "ln_class":"LLN0"})
}

fn patch(value: Value) -> Patch {
    serde_json::from_value(value).unwrap()
}

fn with_goose() -> String {
    edit::prepare(
        &source(),
        &serde_json::from_str(include_str!("../examples/add-goose.json")).unwrap(),
    )
    .unwrap()
}

fn replacement() -> Value {
    json!({"op":"replace_dataset", "target":target(), "name":"dsStatus", "members":[
        {"ld_inst":"LD0","ln_class":"GGIO","ln_inst":"1","do_name":"AnIn1","da_name":"mag.f","fc":"MX"},
        {"ld_inst":"LD0","ln_class":"GGIO","ln_inst":"1","do_name":"Ind1","da_name":"stVal","fc":"ST"}
    ]})
}

#[test]
fn generated_model_resolves_and_generation_is_deterministic() {
    let xml = source();
    assert_eq!(xml, source());
    let doc = SclDocument::parse(&xml).unwrap();
    assert!(validate::references(&doc).is_empty());
    let view = query::inspect(&doc, Some("DEMO")).unwrap();
    assert_eq!(view["logical_nodes"].as_array().unwrap().len(), 3);
    assert!(query::inspect(&doc, Some("missing")).is_err());
}

#[test]
fn dataset_order_change_updates_goose_revision_once_and_is_idempotent() {
    let original = with_goose();
    let request = patch(json!({"schema_version":1,"operations":[replacement()]}));
    let changed = edit::prepare(&original, &request).unwrap();
    assert!(changed.contains("confRev=\"2\""));
    assert_eq!(edit::prepare(&changed, &request).unwrap(), changed);
    let old = SclDocument::parse(&original).unwrap();
    let new = SclDocument::parse(&changed).unwrap();
    assert!(validate::references(&new).is_empty());
    let differences = diff::semantic(&old, &new);
    assert!(differences.iter().any(|d| d.path.contains("FCDA")));
    assert!(differences.iter().any(|d| d.path.contains("GSEControl")));
}

#[test]
fn rejects_reference_errors_and_type_cycles() {
    let xml = with_goose()
        .replace("doName=\"Ind1\"", "doName=\"Missing\"")
        .replace("lnType=\"TK_LPHD\"", "lnType=\"Missing\"");
    let doc = SclDocument::parse(&xml).unwrap();
    let errors = validate::references(&doc);
    assert!(errors.iter().any(|d| d.code == "SCL_FCDA_REFERENCE"));
    assert!(errors.iter().any(|d| d.code == "SCL_LN_TYPE"));
    assert!(errors.iter().all(|d| d.line > 0 && !d.path.is_empty()));
    let cyclic = source().replace(
        "<BDA name=\"f\" bType=\"FLOAT32\"/>",
        "<BDA name=\"f\" bType=\"Struct\" type=\"TK_ANALOG\"/>",
    );
    assert!(
        validate::references(&SclDocument::parse(&cyclic).unwrap())
            .iter()
            .any(|d| d.code == "SCL_TYPE_CYCLE")
    );
}

#[test]
fn rejects_wrong_fc_missing_dataset_and_duplicate_ln() {
    let original = with_goose();
    for (xml, code) in [
        (original.replace("doName=\"AnIn1\" fc=\"MX\"", "doName=\"AnIn1\" fc=\"ST\""), "SCL_FCDA_REFERENCE"),
        (original.replace("datSet=\"dsStatus\"", "datSet=\"missing\""), "SCL_CONTROL_DATASET"),
        (original.replace("<LN lnClass=\"LPHD\" inst=\"1\" lnType=\"TK_LPHD\"/>", "<LN lnClass=\"LPHD\" inst=\"1\" lnType=\"TK_LPHD\"/><LN lnClass=\"LPHD\" inst=\"1\" lnType=\"TK_LPHD\"/>"), "SCL_DUPLICATE_LN"),
    ] {
        assert_ne!(xml, original, "negative fixture must differ from the input");
        assert!(validate::references(&SclDocument::parse(&xml).unwrap()).iter().any(|d| d.code == code), "expected {code}");
    }
}

#[test]
fn local_edit_preserves_bom_crlf_comments_and_vendor_extensions() {
    let extension = "<!-- keep exactly -->\r\n<Private type='vendor'><v:x xmlns:v='urn:vendor' a='  unchanged  '> A &amp; B </v:x></Private>";
    let xml = format!(
        "\u{feff}{}",
        source()
            .replace('\n', "\r\n")
            .replace("  <Header", &format!("{extension}\r\n  <Header"))
    );
    let request = patch(
        json!({"schema_version":1,"operations":[{"op":"set_description","target":target(),"description":"A < B & C \"quoted\"\nnext"}]}),
    );
    let result = edit::prepare(&xml, &request).unwrap();
    assert!(result.starts_with('\u{feff}'));
    assert!(result.contains(extension));
    let inserted = " desc=\"A &lt; B &amp; C &quot;quoted&quot;&#10;next\"";
    assert_eq!(result.replacen(inserted, "", 1), xml);
}

#[test]
fn prefixed_namespace_insertions_resolve_without_changing_existing_nodes() {
    let xml = source().replace("xmlns=", "xmlns:s=");
    let parsed = roxmltree::Document::parse(&xml).unwrap();
    // Prefix every element in the source using original lexical names.
    let mut changes: Vec<_> = parsed
        .descendants()
        .filter(|n| n.is_element())
        .map(|n| (n.range(), n.tag_name().name().to_string()))
        .collect();
    changes.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut prefixed = xml.clone();
    // Replace tag tokens, leaving attributes and text unchanged.
    let names: std::collections::BTreeSet<_> = changes.iter().map(|(_, n)| n.as_str()).collect();
    for name in names {
        prefixed = prefixed
            .replace(&format!("<{name} "), &format!("<s:{name} "))
            .replace(&format!("<{name}>"), &format!("<s:{name}>"))
            .replace(&format!("<{name}/>"), &format!("<s:{name}/>"))
            .replace(&format!("</{name}>"), &format!("</s:{name}>"));
    }
    let request: Patch = serde_json::from_str(include_str!("../examples/add-goose.json")).unwrap();
    let result = edit::prepare(&prefixed, &request).unwrap();
    assert!(result.contains("<s:DataSet"));
    assert!(result.contains("<s:FCDA"));
    assert!(validate::references(&SclDocument::parse(&result).unwrap()).is_empty());
}

#[test]
fn lookalike_ieds_inside_private_are_not_engineering_objects() {
    let xml = source().replace("  <Header", "<Private type=\"vendor\"><IED name=\"DEMO\"><AccessPoint name=\"AP1\"><Server><LDevice inst=\"LD0\"><LN0 lnClass=\"LLN0\" inst=\"\" lnType=\"Missing\"/></LDevice></Server></AccessPoint></IED></Private>\n  <Header");
    let doc = SclDocument::parse(&xml).unwrap();
    assert!(validate::references(&doc).is_empty());
    assert_eq!(
        query::inspect(&doc, None).unwrap()["logical_nodes"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn guarded_operations_reject_stale_inputs_in_use_datasets_and_revision_overflow() {
    let original = with_goose();
    let stale =
        patch(json!({"schema_version":1,"source_sha256":"wrong","operations":[replacement()]}));
    assert!(edit::prepare(&original, &stale).is_err());
    let removal = patch(
        json!({"schema_version":1,"operations":[{"op":"remove_dataset","target":target(),"name":"dsStatus"}]}),
    );
    assert!(edit::prepare(&original, &removal).is_err());
    let overflow = original.replace("confRev=\"1\"", "confRev=\"4294967295\"");
    assert!(
        edit::prepare(
            &overflow,
            &patch(json!({"schema_version":1,"operations":[replacement()]}))
        )
        .is_err()
    );
}

#[test]
fn replacement_never_silently_drops_dataset_extensions() {
    let extended = with_goose().replace(
        "<DataSet name=\"dsStatus\">",
        "<DataSet name=\"dsStatus\"><Private type=\"vendor\">keep</Private>",
    );
    assert!(
        edit::prepare(
            &extended,
            &patch(json!({"schema_version":1,"operations":[replacement()]}))
        )
        .is_err()
    );
}

#[test]
fn diff_ignores_attribute_order_but_exposes_vendor_changes() {
    let before = source().replace(
        "  <Header",
        "<Private type=\"vendor\"><x xmlns=\"urn:vendor\">old</x></Private>\n  <Header",
    );
    let reordered = before.replace("lnClass=\"GGIO\" inst=\"1\"", "inst=\"1\" lnClass=\"GGIO\"");
    assert!(
        diff::semantic(
            &SclDocument::parse(&before).unwrap(),
            &SclDocument::parse(&reordered).unwrap()
        )
        .is_empty()
    );
    let modified = before.replace(">old</x>", ">new</x>");
    let changes = diff::semantic(
        &SclDocument::parse(&before).unwrap(),
        &SclDocument::parse(&modified).unwrap(),
    );
    assert_eq!(changes.len(), 1);
    assert!(changes[0].path.contains("Private"));
}

#[test]
fn generation_escapes_input_and_does_not_expand_tokens_inside_user_text() {
    let mut spec: DeviceSpec =
        serde_json::from_str(include_str!("../examples/device.json")).unwrap();
    spec.description = "<unsafe> & {{IED}}".into();
    spec.signals[0].description = "{{DESCRIPTION}} & \"x\"".into();
    let xml = generate::create(&spec).unwrap();
    let doc = SclDocument::parse(&xml).unwrap();
    let ied = doc
        .xml()
        .descendants()
        .find(|n| n.has_tag_name("IED"))
        .unwrap();
    assert_eq!(ied.attribute("desc"), Some("<unsafe> & {{IED}}"));
    let signal = doc
        .xml()
        .descendants()
        .find(|n| n.has_tag_name("DO") && n.attribute("name") == Some("Ind1"))
        .unwrap();
    assert_eq!(signal.attribute("desc"), Some("{{DESCRIPTION}} & \"x\""));
}

#[test]
fn rejects_dtd_wrong_namespace_encoding_and_unsupported_edit_profile() {
    assert!(SclDocument::parse("<!DOCTYPE SCL [<!ENTITY x 'bad'>]><SCL xmlns='http://www.iec.ch/61850/2003/SCL'>&x;</SCL>").is_err());
    assert!(SclDocument::parse("<SCL xmlns='urn:wrong'/>").is_err());
    assert!(SclDocument::parse(&source().replace("UTF-8", "UTF-16")).is_err());
    let old = source().replace("release=\"4\"", "release=\"5\"");
    let request = patch(
        json!({"schema_version":1,"operations":[{"op":"set_description","target":target(),"description":"x"}]}),
    );
    assert!(edit::prepare(&old, &request).is_err());
}

#[test]
fn adding_node_uses_existing_type_and_removing_unused_dataset_is_supported() {
    let spec = patch(
        json!({"schema_version":1,"operations":[{"op":"add_logical_node","target":{"ied":"DEMO","access_point":"AP1","ld_inst":"LD0","ln_class":"GGIO","ln_inst":"2"},"ln_type":"TK_GGIO"}]}),
    );
    let candidate = edit::prepare(&source(), &spec).unwrap();
    assert!(validate::references(&SclDocument::parse(&candidate).unwrap()).is_empty());
    let mut add: Value = serde_json::from_str(include_str!("../examples/add-goose.json")).unwrap();
    add["operations"].as_array_mut().unwrap().pop();
    let with_dataset = edit::prepare(&source(), &patch(add)).unwrap();
    let removal = patch(
        json!({"schema_version":1,"operations":[{"op":"remove_dataset","target":target(),"name":"dsStatus"}]}),
    );
    assert_eq!(
        edit::prepare(&with_dataset, &removal).unwrap(),
        source().replace(
            "<LN0 lnClass=\"LLN0\" inst=\"\" lnType=\"TK_LLN0\"/>",
            "<LN0 lnClass=\"LLN0\" inst=\"\" lnType=\"TK_LLN0\"></LN0>"
        )
    );
}

#[test]
fn generation_rejects_names_outside_the_supported_schema_profile() {
    for (field, value) in [
        ("ied_name", "_Bad"),
        ("ied_name", "None"),
        (
            "access_point",
            "AccessPointNameThatIsLongerThan32Characters",
        ),
        ("ld_inst", "_Bad"),
    ] {
        let mut spec: Value =
            serde_json::from_str(include_str!("../examples/device.json")).unwrap();
        spec[field] = json!(value);
        assert!(generate::create(&serde_json::from_value(spec).unwrap()).is_err());
    }
    let mut spec: DeviceSpec =
        serde_json::from_str(include_str!("../examples/device.json")).unwrap();
    spec.signals[0].name = "Ind1234567890123".into();
    assert!(generate::create(&spec).is_err());
    spec.signals[0].name = "Ind1".into();
    spec.ld_inst = "0".into();
    assert!(generate::create(&spec).is_ok());
}

#[test]
fn replacement_rejects_namespaced_attributes_and_unknown_text() {
    let source = with_goose().replace("<SCL ", "<SCL xmlns:v=\"urn:vendor\" ");
    for modified in [
        source.replace(
            "<DataSet name=\"dsStatus\">",
            "<DataSet name=\"dsStatus\" v:name=\"keep\">",
        ),
        source.replace(
            "<DataSet name=\"dsStatus\">",
            "<DataSet name=\"dsStatus\">keep this text",
        ),
    ] {
        assert!(
            edit::prepare(
                &modified,
                &patch(json!({"schema_version":1,"operations":[replacement()]}))
            )
            .is_err()
        );
    }
}
