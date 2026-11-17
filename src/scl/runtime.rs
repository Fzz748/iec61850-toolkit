//! Pure projections used to prepare a runtime session. No sockets or file I/O.
use super::{
    SclDocument,
    document::{children, is},
    model::LnTarget,
};
use crate::{
    Error, Result,
    simulation::{GooseAddress, SimulationSpec},
};
use roxmltree::Node;
use serde_json::{Value, json};

fn unique<'a, 'i>(nodes: impl Iterator<Item = Node<'a, 'i>>, label: &str) -> Result<Node<'a, 'i>> {
    let mut nodes = nodes;
    let node = nodes
        .next()
        .ok_or_else(|| Error::Input(format!("missing {label}")))?;
    if nodes.next().is_some() {
        return Err(Error::Input(format!("ambiguous {label}")));
    }
    Ok(node)
}

pub fn project(doc: &SclDocument<'_>, spec: &SimulationSpec) -> Result<Value> {
    let ied = unique(
        children(doc.xml.root_element(), "IED").filter(|n| n.attribute("name") == Some(&spec.ied)),
        "IED",
    )?;
    let ap = unique(
        children(ied, "AccessPoint").filter(|n| n.attribute("name") == Some(&spec.access_point)),
        "AccessPoint",
    )?;
    unique(children(ap, "Server"), "Server")?;
    if spec
        .source_sha256
        .as_ref()
        .is_some_and(|hash| *hash != doc.digest())
    {
        return Err(Error::Input(
            "simulation source_sha256 does not match SCL".into(),
        ));
    }
    let errors = super::validate::references(doc);
    if !errors.is_empty() {
        return Err(Error::Input(format!(
            "SCL reference checks failed: {}",
            serde_json::to_string(&errors)?
        )));
    }
    let mut controls = Vec::new();
    if let Some(goose) = &spec.goose {
        for selection in &goose.controls {
            let target = LnTarget {
                ied: spec.ied.clone(),
                access_point: spec.access_point.clone(),
                ld_inst: selection.ld_inst.clone(),
                prefix: String::new(),
                ln_class: "LLN0".into(),
                ln_inst: String::new(),
            };
            let ln = target.find(doc)?;
            let cb = unique(
                children(ln, "GSEControl").filter(|n| n.attribute("name") == Some(&selection.name)),
                "GSEControl",
            )?;
            if cb.attribute("type").is_some_and(|v| v != "GOOSE") {
                return Err(Error::Input("only GOOSE GSEControl is supported".into()));
            }
            let dataset = cb
                .attribute("datSet")
                .ok_or_else(|| Error::Input("GSEControl has no datSet".into()))?;
            let ds = unique(
                children(ln, "DataSet").filter(|n| n.attribute("name") == Some(dataset)),
                "DataSet",
            )?;
            let address = match &selection.address {
                Some(address) => address.clone(),
                None => communication_address(doc, spec, &selection.ld_inst, &selection.name)?,
            };
            address.validate()?;
            controls.push(
                json!({"ld_inst":selection.ld_inst, "name":selection.name, "dataset":dataset,
                "member_count":children(ds,"FCDA").count(),
                "members":children(ds,"FCDA").map(|member| {
                    let fcda = super::model::Fcda::from_node(member);
                    let mut variable = format!("{}{}{}${}${}",fcda.prefix,fcda.ln_class,fcda.ln_inst,fcda.fc,fcda.do_name.replace('.',"$"));
                    if let Some(attribute) = fcda.da_name { variable.push('$'); variable.push_str(&attribute.replace('.',"$")); }
                    json!({"ld_inst":fcda.ld_inst,"variable":variable})
                }).collect::<Vec<_>>(), "address":address,
                "address_source":if selection.address.is_some() {"override"} else {"scl"}}),
            );
        }
    }
    Ok(
        json!({"source_sha256":doc.digest(), "ied":spec.ied, "access_point":spec.access_point, "controls":controls}),
    )
}

fn communication_address(
    doc: &SclDocument<'_>,
    spec: &SimulationSpec,
    ld: &str,
    cb: &str,
) -> Result<GooseAddress> {
    let candidates = children(doc.xml.root_element(), "Communication")
        .flat_map(|n| children(n, "SubNetwork"))
        .flat_map(|n| children(n, "ConnectedAP"))
        .filter(|n| {
            n.attribute("iedName") == Some(&spec.ied)
                && n.attribute("apName") == Some(&spec.access_point)
        })
        .flat_map(|n| children(n, "GSE"))
        .filter(|n| n.attribute("ldInst") == Some(ld) && n.attribute("cbName") == Some(cb));
    let gse = unique(
        candidates,
        "Communication/GSE (supply an explicit address override when absent)",
    )?;
    let address = unique(children(gse, "Address"), "GSE/Address")?;
    let parameter = |name| -> Result<String> {
        let p = unique(
            children(address, "P").filter(|n| n.attribute("type") == Some(name)),
            name,
        )?;
        Ok(p.text().unwrap_or("").trim().to_owned())
    };
    let hex = |name| -> Result<u16> {
        u16::from_str_radix(&parameter(name)?, 16)
            .map_err(|_| Error::Input(format!("invalid {name}")))
    };
    Ok(GooseAddress {
        mac: parameter("MAC-Address")?,
        appid: hex("APPID")?,
        vlan_id: hex("VLAN-ID")?,
        vlan_priority: parameter("VLAN-PRIORITY")?
            .parse()
            .map_err(|_| Error::Input("invalid VLAN priority".into()))?,
        vlan_tagged: true,
        min_time_ms: time_ms(gse, "MinTime")?,
        max_time_ms: time_ms(gse, "MaxTime")?,
    })
}

fn time_ms(parent: Node<'_, '_>, tag: &str) -> Result<u32> {
    let n = unique(parent.children().filter(|n| is(*n, tag)), tag)?;
    if n.attribute("unit") != Some("s") {
        return Err(Error::Input(format!("{tag} requires unit=s")));
    }
    let factor = match n.attribute("multiplier").unwrap_or("") {
        "" => 1000.0,
        "m" => 1.0,
        "u" => 0.001,
        "n" => 0.000001,
        _ => return Err(Error::Input(format!("unsupported {tag} multiplier"))),
    };
    let value: f64 = n
        .text()
        .unwrap_or("")
        .trim()
        .parse()
        .map_err(|_| Error::Input(format!("invalid {tag}")))?;
    let ms = value * factor;
    if !ms.is_finite() || ms.fract() != 0.0 || ms < 1.0 || ms > f64::from(u32::MAX) {
        return Err(Error::Input(format!(
            "{tag} must be representable as positive whole milliseconds"
        )));
    }
    Ok(ms as u32)
}
