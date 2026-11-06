use super::{
    document::{SclDocument, children, path},
    model::{Fcda, LnTarget, logical_nodes},
};
use crate::{Error, Result};
use serde_json::{Value, json};

pub fn inspect(doc: &SclDocument<'_>, ied_filter: Option<&str>) -> Result<Value> {
    let root = doc.xml.root_element();
    let ieds: Vec<_> = children(root, "IED")
        .filter(|n| ied_filter.is_none_or(|s| n.attribute("name") == Some(s)))
        .map(|n| n.attribute("name").unwrap_or(""))
        .collect();
    if ied_filter.is_some() && ieds.is_empty() {
        return Err(Error::Input("IED filter did not match any IED".into()));
    }
    let nodes: Vec<_> = logical_nodes(doc).into_iter().filter(|n| ied_filter.is_none_or(|s| LnTarget::of(*n).ied == s)).map(|ln| {
        let datasets: Vec<_> = children(ln, "DataSet").map(|ds| json!({
            "name": ds.attribute("name"), "path": path(ds),
            "members": children(ds, "FCDA").map(Fcda::from_node).collect::<Vec<_>>()
        })).collect();
        let goose: Vec<_> = children(ln, "GSEControl").map(|cb| json!({
            "name": cb.attribute("name"), "dataset": cb.attribute("datSet"),
            "conf_rev": cb.attribute("confRev"), "path": path(cb)
        })).collect();
        json!({"target": LnTarget::of(ln), "ln_type": ln.attribute("lnType"), "path": path(ln), "datasets": datasets, "goose": goose})
    }).collect();
    let types: Vec<_> = children(root, "DataTypeTemplates")
        .flat_map(|n| n.children())
        .filter(|n| n.is_element())
        .map(|n| json!({"kind": n.tag_name().name(), "id": n.attribute("id")}))
        .collect();
    Ok(
        json!({"source_sha256": doc.digest(), "scl_version": {"version": root.attribute("version"), "revision": root.attribute("revision"), "release": root.attribute("release")}, "ieds": ieds, "logical_nodes": nodes, "types": types}),
    )
}
