use super::{
    document::{SclDocument, children, is, qname},
    model::{Fcda, LnTarget, logical_nodes},
};
use crate::{Error, Result};
use roxmltree::Node;
use serde::Deserialize;
use std::ops::Range;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub schema_version: u32,
    pub source_sha256: Option<String>,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    SetDescription {
        target: LnTarget,
        description: String,
    },
    AddLogicalNode {
        target: LnTarget,
        ln_type: String,
    },
    AddDataset {
        target: LnTarget,
        name: String,
        members: Vec<Fcda>,
    },
    ReplaceDataset {
        target: LnTarget,
        name: String,
        members: Vec<Fcda>,
    },
    RemoveDataset {
        target: LnTarget,
        name: String,
    },
    AddGoose {
        target: LnTarget,
        name: String,
        dataset: String,
        go_id: String,
    },
    SetGooseDataset {
        target: LnTarget,
        name: String,
        dataset: String,
    },
}

pub fn escape(value: &str) -> String {
    quick_xml::escape::escape(value)
        .replace('\n', "&#10;")
        .replace('\t', "&#9;")
}

fn attr_change(
    doc: &SclDocument<'_>,
    node: Node<'_, '_>,
    attr: &str,
    value: &str,
) -> (Range<usize>, String) {
    if let Some(existing) = node.attribute_node(attr) {
        return (existing.range_value(), escape(value));
    }
    let offset = node.range().start + 1 + qname(doc, node).len();
    (offset..offset, format!(" {attr}=\"{}\"", escape(value)))
}

fn insert(
    doc: &SclDocument<'_>,
    parent: Node<'_, '_>,
    fragment: &str,
    before: &[&str],
) -> (Range<usize>, String) {
    if let Some(next) = parent
        .children()
        .find(|n| before.iter().any(|name| is(*n, name)))
    {
        let offset = next.range().start;
        return (offset..offset, fragment.into());
    }
    let range = parent.range();
    let raw = &doc.source[range.clone()];
    if raw.ends_with("/>") {
        let offset = range.end - 2;
        return (
            offset..range.end,
            format!(">{fragment}</{}>", qname(doc, parent)),
        );
    }
    let closing = raw
        .rfind("</")
        .expect("parsed nonempty element has closing tag");
    let offset = range.start + closing;
    (offset..offset, fragment.into())
}

fn qualified(doc: &SclDocument<'_>, parent: Node<'_, '_>, name: &str) -> String {
    match qname(doc, parent).split_once(':') {
        Some((prefix, _)) => format!("{prefix}:{name}"),
        None => name.into(),
    }
}

fn check_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Edit("name must not be empty".into()));
    }
    Ok(())
}

fn unique_child<'a, 'i>(parent: Node<'a, 'i>, tag: &str, name: &str) -> Result<Node<'a, 'i>> {
    let found: Vec<_> = parent
        .children()
        .filter(|n| is(*n, tag) && n.attribute("name") == Some(name))
        .collect();
    match found.as_slice() {
        [node] => Ok(*node),
        _ => Err(Error::Edit(format!(
            "{tag} {name:?} is missing or ambiguous"
        ))),
    }
}

fn revision_change(doc: &SclDocument<'_>, cb: Node<'_, '_>) -> Result<(Range<usize>, String)> {
    let rev = cb
        .attribute("confRev")
        .and_then(|s| s.parse::<u32>().ok())
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| Error::Edit("confRev is missing, invalid, or would overflow".into()))?;
    Ok(attr_change(doc, cb, "confRev", &rev.to_string()))
}

fn dataset_fragment(
    doc: &SclDocument<'_>,
    parent: Node<'_, '_>,
    name: &str,
    members: &[Fcda],
) -> Result<String> {
    check_name(name)?;
    let ds_tag = qualified(doc, parent, "DataSet");
    let mut xml = format!("<{ds_tag} name=\"{}\">", escape(name));
    for member in members {
        xml.push_str(&member_fragment(doc, parent, member));
    }
    xml.push_str(&format!("</{ds_tag}>"));
    Ok(xml)
}

fn member_fragment(doc: &SclDocument<'_>, parent: Node<'_, '_>, member: &Fcda) -> String {
    let tag = qualified(doc, parent, "FCDA");
    let mut xml = format!(
        "<{tag} ldInst=\"{}\" prefix=\"{}\" lnClass=\"{}\" lnInst=\"{}\" doName=\"{}\" fc=\"{}\"",
        escape(&member.ld_inst),
        escape(&member.prefix),
        escape(&member.ln_class),
        escape(&member.ln_inst),
        escape(&member.do_name),
        escape(&member.fc)
    );
    if let Some(da) = &member.da_name {
        xml.push_str(&format!(" daName=\"{}\"", escape(da)));
    }
    xml.push_str("/>");
    xml
}

fn apply_changes(source: &str, mut changes: Vec<(Range<usize>, String)>) -> Result<String> {
    changes.sort_by_key(|change| std::cmp::Reverse(change.0.start));
    let mut result = source.to_string();
    let mut previous_start = result.len();
    for (range, replacement) in changes {
        if range.end > previous_start {
            return Err(Error::Edit("overlapping XML changes".into()));
        }
        previous_start = range.start;
        result.replace_range(range, &replacement);
    }
    Ok(result)
}

fn update_member(doc: &SclDocument<'_>, node: Node<'_, '_>, member: &Fcda) -> Result<String> {
    let mut edits = Vec::new();
    for (name, value) in [
        ("ldInst", &member.ld_inst),
        ("prefix", &member.prefix),
        ("lnClass", &member.ln_class),
        ("lnInst", &member.ln_inst),
        ("doName", &member.do_name),
        ("fc", &member.fc),
    ] {
        if node.attribute(name).unwrap_or("") != value {
            edits.push(attr_change(doc, node, name, value));
        }
    }
    match &member.da_name {
        Some(value) if node.attribute("daName") != Some(value) => {
            edits.push(attr_change(doc, node, "daName", value))
        }
        None => {
            if let Some(attribute) = node.attribute_node("daName") {
                edits.push((attribute.range(), String::new()));
            }
        }
        _ => {}
    }
    let start = node.range().start;
    let local = edits
        .into_iter()
        .map(|(r, v)| (r.start - start..r.end - start, v))
        .collect();
    apply_changes(&doc.source[node.range()], local)
}

fn replace_members(
    doc: &SclDocument<'_>,
    ds: Node<'_, '_>,
    members: &[Fcda],
) -> Result<Vec<(Range<usize>, String)>> {
    let old: Vec<_> = children(ds, "FCDA").collect();
    let mut used = vec![false; old.len()];
    let mut assignments = Vec::new();
    // Move exact matches intact, including unknown attributes and local namespaces.
    for member in members {
        let matched = old
            .iter()
            .enumerate()
            .position(|(i, n)| !used[i] && Fcda::from_node(*n) == *member);
        if let Some(i) = matched {
            used[i] = true;
        }
        assignments.push(matched);
    }
    for slot in &mut assignments {
        if slot.is_none() {
            if let Some(i) = used.iter().position(|used| !used) {
                used[i] = true;
                *slot = Some(i);
            }
        }
    }
    let replacements: Vec<_> = members
        .iter()
        .zip(assignments)
        .map(|(member, old_index)| match old_index {
            Some(i) => update_member(doc, old[i], member),
            None => Ok(member_fragment(doc, ds, member)),
        })
        .collect::<Result<_>>()?;
    if old.is_empty() {
        return Ok(vec![insert(doc, ds, &replacements.concat(), &[])]);
    }
    let mut edits: Vec<_> = old
        .iter()
        .enumerate()
        .map(|(i, node)| {
            (
                node.range(),
                replacements.get(i).cloned().unwrap_or_default(),
            )
        })
        .collect();
    if replacements.len() > old.len() {
        edits
            .last_mut()
            .expect("nonempty old members")
            .1
            .push_str(&replacements[old.len()..].concat());
    }
    Ok(edits)
}

fn apply_one(doc: &SclDocument<'_>, operation: &Operation) -> Result<String> {
    let mut changes = Vec::new();
    match operation {
        Operation::SetDescription {
            target,
            description,
        } => changes.push(attr_change(doc, target.find(doc)?, "desc", description)),
        Operation::AddLogicalNode { target, ln_type } => {
            if target.ln_class.is_empty() || target.ln_class == "LLN0" || target.ln_inst.is_empty()
            {
                return Err(Error::Edit(
                    "add_logical_node requires an LN class and instance; LN0 is a separate element"
                        .into(),
                ));
            }
            if ln_type.is_empty() {
                return Err(Error::Edit(
                    "ln_type must reference an existing nonempty type identifier".into(),
                ));
            }
            let types: Vec<_> = children(doc.xml.root_element(), "DataTypeTemplates")
                .flat_map(|section| children(section, "LNodeType"))
                .filter(|node| node.attribute("id") == Some(ln_type))
                .collect();
            let [ln_type_node] = types.as_slice() else {
                return Err(Error::Edit("LNodeType is missing or ambiguous".into()));
            };
            if ln_type_node.attribute("lnClass") != Some(&target.ln_class) {
                return Err(Error::Edit(
                    "new logical node class does not match its LNodeType".into(),
                ));
            }
            if logical_nodes(doc)
                .into_iter()
                .any(|n| LnTarget::of(n) == *target)
            {
                return Err(Error::Edit("logical node already exists".into()));
            }
            let mut matches = Vec::new();
            for ied in children(doc.xml.root_element(), "IED")
                .filter(|n| n.attribute("name") == Some(&target.ied))
            {
                for ap in children(ied, "AccessPoint")
                    .filter(|n| n.attribute("name") == Some(&target.access_point))
                {
                    for server in children(ap, "Server") {
                        matches.extend(
                            children(server, "LDevice")
                                .filter(|n| n.attribute("inst") == Some(&target.ld_inst)),
                        );
                    }
                }
            }
            let [ld] = matches.as_slice() else {
                return Err(Error::Edit("LDevice is missing or ambiguous".into()));
            };
            let tag = qualified(doc, *ld, "LN");
            changes.push(insert(
                doc,
                *ld,
                &format!(
                    "<{tag} prefix=\"{}\" lnClass=\"{}\" inst=\"{}\" lnType=\"{}\"/>",
                    escape(&target.prefix),
                    escape(&target.ln_class),
                    escape(&target.ln_inst),
                    escape(ln_type)
                ),
                &["AccessControl"],
            ));
        }
        Operation::AddDataset {
            target,
            name,
            members,
        } => {
            let ln = target.find(doc)?;
            if children(ln, "DataSet").any(|n| n.attribute("name") == Some(name)) {
                return Err(Error::Edit("DataSet already exists".into()));
            }
            changes.push(insert(
                doc,
                ln,
                &dataset_fragment(doc, ln, name, members)?,
                &[
                    "ReportControl",
                    "LogControl",
                    "Inputs",
                    "Log",
                    "DOI",
                    "GSEControl",
                    "SampledValueControl",
                    "SettingControl",
                ],
            ));
        }
        Operation::ReplaceDataset {
            target,
            name,
            members,
        } => {
            let ln = target.find(doc)?;
            let ds = unique_child(ln, "DataSet", name)?;
            let old: Vec<_> = children(ds, "FCDA").map(Fcda::from_node).collect();
            if old == *members {
                return Ok(doc.source.into());
            }
            for cb in ln
                .children()
                .filter(|n| n.is_element() && n.attribute("datSet") == Some(name))
            {
                if ["GSEControl", "ReportControl", "SampledValueControl"]
                    .iter()
                    .any(|tag| is(cb, tag))
                {
                    changes.push(revision_change(doc, cb)?);
                }
            }
            changes.extend(replace_members(doc, ds, members)?);
        }
        Operation::RemoveDataset { target, name } => {
            let ln = target.find(doc)?;
            let ds = unique_child(ln, "DataSet", name)?;
            if ln
                .children()
                .any(|n| n.is_element() && n.attribute("datSet") == Some(name))
            {
                return Err(Error::Edit(
                    "cannot remove a DataSet referenced by a control block".into(),
                ));
            }
            changes.push((ds.range(), String::new()));
        }
        Operation::AddGoose {
            target,
            name,
            dataset,
            go_id,
        } => {
            check_name(name)?;
            if go_id.is_empty() {
                return Err(Error::Edit("GOOSE go_id must not be empty".into()));
            }
            let ln = target.find(doc)?;
            if !is(ln, "LN0") {
                return Err(Error::Edit("GSEControl must be added to LN0".into()));
            }
            unique_child(ln, "DataSet", dataset)?;
            if children(ln, "GSEControl").any(|n| n.attribute("name") == Some(name)) {
                return Err(Error::Edit("GSEControl already exists".into()));
            }
            let tag = qualified(doc, ln, "GSEControl");
            changes.push(insert(
                doc,
                ln,
                &format!(
                    "<{tag} name=\"{}\" type=\"GOOSE\" appID=\"{}\" datSet=\"{}\" confRev=\"1\"/>",
                    escape(name),
                    escape(go_id),
                    escape(dataset)
                ),
                &["SampledValueControl", "SettingControl"],
            ));
        }
        Operation::SetGooseDataset {
            target,
            name,
            dataset,
        } => {
            let ln = target.find(doc)?;
            let cb = unique_child(ln, "GSEControl", name)?;
            unique_child(ln, "DataSet", dataset)?;
            if cb.attribute("datSet") == Some(dataset) {
                return Ok(doc.source.into());
            }
            changes.push(attr_change(doc, cb, "datSet", dataset));
            changes.push(revision_change(doc, cb)?);
        }
    }
    apply_changes(doc.source, changes)
}

pub fn prepare(source: &str, patch: &Patch) -> Result<String> {
    if patch.schema_version != 1 {
        return Err(Error::Edit("patch requires schema_version=1".into()));
    }
    let doc = SclDocument::parse(source)?;
    if patch
        .source_sha256
        .as_ref()
        .is_some_and(|expected| expected != &doc.digest())
    {
        return Err(Error::Edit(
            "source_sha256 does not match the input file".into(),
        ));
    }
    let mut candidate = source.to_owned();
    for (index, op) in patch.operations.iter().enumerate() {
        // Reparse after each operation so no stale byte ranges survive an insertion.
        let current = SclDocument::parse(&candidate)?;
        candidate = apply_one(&current, op)
            .map_err(|e| Error::Edit(format!("operation {}: {e}", index + 1)))?;
    }
    SclDocument::parse(&candidate)?;
    Ok(candidate)
}
