//! Serializable simulation inputs. SCL and native runtime checks complete the validation.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, net::IpAddr, path::PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationSpec {
    pub schema_version: u32,
    pub scl: PathBuf,
    pub ied: String,
    pub access_point: String,
    pub source_sha256: Option<String>,
    pub mms: Option<MmsServer>,
    pub goose: Option<GoosePublisher>,
    #[serde(default)]
    pub initial: Vec<ValueUpdate>,
    #[serde(default)]
    pub timeline: Vec<Step>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MmsServer {
    pub listen: IpAddr,
    pub port: u16,
    #[serde(default)]
    pub writable: Vec<WritableAttribute>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WritableAttribute {
    pub reference: String,
    pub fc: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoosePublisher {
    pub interface: String,
    pub controls: Vec<GooseControl>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GooseControl {
    pub ld_inst: String,
    pub name: String,
    /// Explicit override; when absent, read the matching SCL Communication/GSE.
    pub address: Option<GooseAddress>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GooseAddress {
    pub mac: String,
    pub appid: u16,
    pub vlan_id: u16,
    pub vlan_priority: u8,
    pub vlan_tagged: bool,
    pub min_time_ms: u32,
    pub max_time_ms: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValueUpdate {
    pub reference: String,
    pub value: Value,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub at_ms: u32,
    pub values: Vec<ValueUpdate>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionSpec {
    pub schema_version: u32,
    pub interface: String,
    pub gocb_ref: String,
    pub mac: String,
    pub appid: u16,
    pub dataset: String,
    pub conf_rev: u32,
}

pub fn nonempty(value: &str, field: &str) -> Result<()> {
    if value.is_empty() || value.contains('\0') {
        return Err(Error::Input(format!(
            "{field} must be nonempty and contain no NUL"
        )));
    }
    Ok(())
}

pub fn mac_bytes(value: &str) -> Result<[u8; 6]> {
    let bytes = value
        .split(['-', ':'])
        .map(|part| {
            if part.len() != 2 {
                return Err(());
            }
            u8::from_str_radix(part, 16).map_err(|_| ())
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::Input("invalid MAC address".into()))?;
    let mac: [u8; 6] = bytes
        .try_into()
        .map_err(|_| Error::Input("MAC address needs six octets".into()))?;
    if mac[0] & 1 == 0 || mac == [255; 6] {
        return Err(Error::Input(
            "GOOSE destination must be a multicast MAC, excluding broadcast".into(),
        ));
    }
    Ok(mac)
}

impl GooseAddress {
    pub fn validate(&self) -> Result<()> {
        mac_bytes(&self.mac)?;
        if self.appid > 0x3fff || self.vlan_id > 4094 || self.vlan_priority > 7 {
            return Err(Error::Input(
                "GOOSE APPID/VLAN parameters are out of range".into(),
            ));
        }
        if self.min_time_ms == 0
            || self.min_time_ms > self.max_time_ms
            || self.max_time_ms > i32::MAX as u32 / 3
        {
            return Err(Error::Input(
                "GOOSE timing requires 0 < min_time_ms <= max_time_ms and representable TTL".into(),
            ));
        }
        Ok(())
    }
}

impl SimulationSpec {
    pub fn validate(&self, duration_ms: Option<u64>) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::Input("simulation requires schema_version=1".into()));
        }
        nonempty(&self.ied, "ied")?;
        nonempty(&self.access_point, "access_point")?;
        if self.mms.is_none() && self.goose.is_none() {
            return Err(Error::Input("enable mms and/or goose".into()));
        }
        if self.mms.as_ref().is_some_and(|m| m.port == 0) {
            return Err(Error::Input("MMS port must be nonzero".into()));
        }
        if let Some(mms) = &self.mms {
            let mut seen = BTreeSet::new();
            for attribute in &mms.writable {
                nonempty(&attribute.reference, "writable reference")?;
                if !matches!(attribute.fc.as_str(), "SP" | "SV" | "CF" | "DC" | "BL") {
                    return Err(Error::Input("writable FC must be SP, SV, CF, DC or BL; controls and setting-group editing use separate services".into()));
                }
                if !seen.insert(&attribute.reference) {
                    return Err(Error::Input("duplicate writable reference".into()));
                }
            }
        }
        if let Some(goose) = &self.goose {
            nonempty(&goose.interface, "goose.interface")?;
            if goose.controls.is_empty() {
                return Err(Error::Input("goose.controls is empty".into()));
            }
            let mut seen = BTreeSet::new();
            for control in &goose.controls {
                nonempty(&control.ld_inst, "ld_inst")?;
                nonempty(&control.name, "GOOSE name")?;
                if !seen.insert((&control.ld_inst, &control.name)) {
                    return Err(Error::Input("duplicate GOOSE control selection".into()));
                }
                if let Some(address) = &control.address {
                    address.validate()?;
                }
            }
        }
        validate_updates(&self.initial)?;
        let mut previous = None;
        for step in &self.timeline {
            if previous.is_some_and(|p| step.at_ms <= p)
                || duration_ms.is_some_and(|d| u64::from(step.at_ms) >= d)
            {
                return Err(Error::Input(
                    "timeline times must strictly increase and precede duration".into(),
                ));
            }
            if step.values.is_empty() {
                return Err(Error::Input("timeline step has no values".into()));
            }
            validate_updates(&step.values)?;
            previous = Some(step.at_ms);
        }
        Ok(())
    }
}

pub fn validate_write_value(value: &Value) -> Result<()> {
    if value.is_null() || value.is_array() || value.is_object() {
        return Err(Error::Input(
            "mms write requires a JSON scalar; structure/array writes are not supported".into(),
        ));
    }
    if value.as_str().is_some_and(|text| text.contains('\0')) {
        return Err(Error::Input("MMS string values cannot contain NUL".into()));
    }
    Ok(())
}

fn validate_updates(updates: &[ValueUpdate]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for update in updates {
        nonempty(&update.reference, "reference")?;
        if !seen.insert(&update.reference) {
            return Err(Error::Input("duplicate reference in value batch".into()));
        }
        if update.value.is_null() || update.value.is_array() || update.value.is_object() {
            return Err(Error::Input(
                "simulation updates require scalar values; address individual leaf attributes"
                    .into(),
            ));
        }
    }
    Ok(())
}

impl SubscriptionSpec {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::Input(
                "subscription requires schema_version=1".into(),
            ));
        }
        for (name, value) in [
            ("interface", &self.interface),
            ("gocb_ref", &self.gocb_ref),
            ("dataset", &self.dataset),
        ] {
            nonempty(value, name)?;
        }
        mac_bytes(&self.mac)?;
        if self.appid > 0x3fff {
            return Err(Error::Input("GOOSE APPID is out of range".into()));
        }
        Ok(())
    }
}
