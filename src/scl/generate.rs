use super::{document::identifier, edit::escape};
use crate::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSpec {
    pub schema_version: u32,
    pub template: String,
    pub ied_name: String,
    pub access_point: String,
    pub ld_inst: String,
    #[serde(default)]
    pub description: String,
    pub signals: Vec<Signal>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    pub name: String,
    pub kind: SignalKind,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    Sps,
    Mv,
}

pub fn create(spec: &DeviceSpec) -> Result<String> {
    if spec.schema_version != 1 || spec.template != "minimal-ggio" {
        return Err(Error::Input(
            "device spec requires schema_version=1 and template=minimal-ggio".into(),
        ));
    }
    for value in [&spec.ied_name, &spec.access_point] {
        if !identifier(value) {
            return Err(Error::Input(format!("invalid device identifier {value:?}")));
        }
    }
    if spec.ied_name == "None" || spec.access_point.len() > 32 {
        return Err(Error::Input(
            "IED name None is reserved; AccessPoint names are limited to 32 characters".into(),
        ));
    }
    if spec.ld_inst.is_empty()
        || spec.ld_inst.len() > 64
        || !spec
            .ld_inst
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        || !spec
            .ld_inst
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(Error::Input("ld_inst requires 1..64 ASCII alphanumeric/underscore characters with an alphanumeric first character".into()));
    }
    if spec.signals.is_empty() || spec.signals.len() > 1024 {
        return Err(Error::Input("provide 1..1024 signals".into()));
    }
    let mut seen = BTreeSet::new();
    let mut signals = String::new();
    for signal in &spec.signals {
        let (prefix, type_id) = match signal.kind {
            SignalKind::Sps => ("Ind", "TK_SPS"),
            SignalKind::Mv => ("AnIn", "TK_MV"),
        };
        if !signal.name.strip_prefix(prefix).is_some_and(|n| {
            !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
        }) || signal.name.len() > 12
            || !seen.insert(&signal.name)
        {
            return Err(Error::Input(format!(
                "signal {:?} must be a unique {prefix}<positive integer>, at most 12 characters",
                signal.name
            )));
        }
        signals.push_str(&format!(
            "      <DO name=\"{}\" type=\"{type_id}\" desc=\"{}\"/>\n",
            escape(&signal.name),
            escape(&signal.description)
        ));
    }
    // All substitutions are XML-escaped before insertion; the template is compiled into the binary.
    let template = include_str!("../../templates/minimal-ggio.icd.xml");
    let values = [
        ("SIGNALS", signals),
        ("IED", escape(&spec.ied_name)),
        ("AP", escape(&spec.access_point)),
        ("LD", escape(&spec.ld_inst)),
        ("DESCRIPTION", escape(&spec.description)),
    ];
    let mut result = String::new();
    let mut remaining = template;
    while let Some((head, tail)) = remaining.split_once("{{") {
        result.push_str(head);
        let (key, rest) = tail
            .split_once("}}")
            .ok_or_else(|| Error::Input("invalid embedded template token".into()))?;
        let (_, value) = values
            .iter()
            .find(|(name, _)| *name == key)
            .ok_or_else(|| Error::Input("unknown embedded template token".into()))?;
        result.push_str(value);
        remaining = rest;
    }
    result.push_str(remaining);
    Ok(result)
}
