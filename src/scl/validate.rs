use super::{
    document::{SclDocument, children, is, path},
    model::{Fcda, LnTarget, logical_nodes},
};
use crate::report::Diagnostic;
use roxmltree::Node;
use std::collections::{BTreeMap, BTreeSet};

type TypeIndex<'a, 'i> = BTreeMap<(String, String), Node<'a, 'i>>;

fn diagnostic(node: Node<'_, '_>, code: &str, message: impl Into<String>) -> Diagnostic {
    let position = node.document().text_pos_at(node.range().start);
    Diagnostic {
        code: code.into(),
        severity: "error".into(),
        message: message.into(),
        path: path(node),
        line: position.row,
        column: position.col,
    }
}

fn required(node: Node<'_, '_>, attr: &str, errors: &mut Vec<Diagnostic>) {
    if node.attribute(attr).is_none_or(str::is_empty) {
        errors.push(diagnostic(
            node,
            "SCL_REQUIRED_ATTRIBUTE",
            format!("missing {attr}"),
        ));
    }
}

fn type_target(node: Node<'_, '_>) -> Option<&'static str> {
    match node.tag_name().name() {
        "DO" | "SDO" => Some("DOType"),
        "DA" | "BDA" => match node.attribute("bType") {
            Some("Struct") => Some("DAType"),
            Some("Enum") => Some("EnumType"),
            _ => None,
        },
        _ => None,
    }
}

fn named<'a, 'i>(parent: Node<'a, 'i>, tag: &str, name: &str) -> Option<Node<'a, 'i>> {
    parent
        .children()
        .find(|n| is(*n, tag) && n.attribute("name") == Some(name))
}

fn resolve_type<'a, 'i>(index: &TypeIndex<'a, 'i>, kind: &str, id: &str) -> Option<Node<'a, 'i>> {
    index.get(&(kind.into(), id.into())).copied()
}

fn check_cycle(
    key: &(String, String),
    index: &TypeIndex<'_, '_>,
    active: &mut BTreeSet<(String, String)>,
    done: &mut BTreeSet<(String, String)>,
    errors: &mut Vec<Diagnostic>,
) {
    if done.contains(key) {
        return;
    }
    let Some(node) = index.get(key).copied() else {
        return;
    };
    if active.contains(key) || active.len() >= 128 {
        errors.push(diagnostic(
            node,
            "SCL_TYPE_CYCLE",
            "recursive type reference or type depth exceeds 128",
        ));
        return;
    }
    active.insert(key.clone());
    for child in node
        .children()
        .filter(|n| n.is_element() && n.tag_name().namespace() == Some(super::document::SCL_NS))
    {
        if let (Some(kind), Some(id)) = (type_target(child), child.attribute("type")) {
            check_cycle(&(kind.into(), id.into()), index, active, done, errors);
        }
    }
    active.remove(key);
    done.insert(key.clone());
}

fn check_fcda(
    fcda: Node<'_, '_>,
    owner: Node<'_, '_>,
    nodes: &[Node<'_, '_>],
    index: &TypeIndex<'_, '_>,
) -> std::result::Result<(), String> {
    let member = Fcda::from_node(fcda);
    if fcda.attribute("ix").is_some() {
        return Err("array-indexed FCDA is outside this version's reference-check scope".into());
    }
    if member.ld_inst.is_empty()
        || member.ln_class.is_empty()
        || member.do_name.is_empty()
        || member.fc.is_empty()
    {
        return Err(
            "FCDA requires explicit ldInst, lnClass, doName, and fc for reference checking".into(),
        );
    }
    let base = LnTarget::of(owner);
    let target = LnTarget {
        ied: base.ied,
        access_point: base.access_point,
        ld_inst: member.ld_inst,
        prefix: member.prefix,
        ln_class: member.ln_class,
        ln_inst: member.ln_inst,
    };
    let candidates: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|n| LnTarget::of(*n) == target)
        .collect();
    let [ln] = candidates.as_slice() else {
        return Err(format!(
            "FCDA logical node is missing or ambiguous: {target:?}"
        ));
    };
    let ln_type = resolve_type(index, "LNodeType", ln.attribute("lnType").unwrap_or(""))
        .ok_or("FCDA logical node has unresolved lnType")?;
    let mut do_parts = member.do_name.split('.');
    let object = named(ln_type, "DO", do_parts.next().unwrap_or(""))
        .ok_or("FCDA data object does not exist")?;
    if object.attribute("count").is_some() {
        return Err("array data objects are not supported by this reference checker".into());
    }
    let mut object_type = resolve_type(index, "DOType", object.attribute("type").unwrap_or(""))
        .ok_or("FCDA DO type is unresolved")?;
    for part in do_parts {
        let sdo =
            named(object_type, "SDO", part).ok_or("FCDA structured data object does not exist")?;
        if sdo.attribute("count").is_some() {
            return Err("array SDO is not supported by this reference checker".into());
        }
        object_type = resolve_type(index, "DOType", sdo.attribute("type").unwrap_or(""))
            .ok_or("FCDA SDO type is unresolved")?;
    }
    if let Some(da_name) = member.da_name {
        let mut parts = da_name.split('.');
        let mut attribute = named(object_type, "DA", parts.next().unwrap_or(""))
            .ok_or("FCDA data attribute does not exist")?;
        if attribute.attribute("fc") != Some(member.fc.as_str()) {
            return Err("FCDA functional constraint does not match its data attribute".into());
        }
        if attribute.attribute("count").is_some() {
            return Err("array DA is not supported by this reference checker".into());
        }
        for part in parts {
            if attribute.attribute("bType") != Some("Struct") {
                return Err("FCDA traverses a non-structured data attribute".into());
            }
            let da_type = resolve_type(index, "DAType", attribute.attribute("type").unwrap_or(""))
                .ok_or("FCDA DAType is unresolved")?;
            attribute =
                named(da_type, "BDA", part).ok_or("FCDA basic data attribute does not exist")?;
            if attribute.attribute("count").is_some() {
                return Err("array BDA is not supported by this reference checker".into());
            }
        }
    } else if !contains_fc(object_type, &member.fc, index, &mut BTreeSet::new()) {
        return Err("FCDA functional constraint does not exist in its data object".into());
    }
    Ok(())
}

fn contains_fc(
    node: Node<'_, '_>,
    fc: &str,
    index: &TypeIndex<'_, '_>,
    visited: &mut BTreeSet<String>,
) -> bool {
    let id = node.attribute("id").unwrap_or("").to_string();
    if visited.len() >= 128 || !visited.insert(id) {
        return false;
    }
    children(node, "DA").any(|n| n.attribute("fc") == Some(fc))
        || children(node, "SDO").any(|n| {
            resolve_type(index, "DOType", n.attribute("type").unwrap_or(""))
                .is_some_and(|n| contains_fc(n, fc, index, visited))
        })
}

/// Deliberately bounded coverage: type references, LN identities, FCDA paths, local CB/DataSet links.
pub fn references(doc: &SclDocument<'_>) -> Vec<Diagnostic> {
    let mut errors = Vec::new();
    let root = doc.xml.root_element();
    let mut index = TypeIndex::new();
    let templates: Vec<_> = children(root, "DataTypeTemplates").collect();
    if templates.len() != 1 {
        errors.push(diagnostic(
            root,
            "SCL_TYPES_SECTION",
            "expected exactly one DataTypeTemplates section",
        ));
    }
    for section in templates {
        for node in section.children().filter(|n| {
            ["LNodeType", "DOType", "DAType", "EnumType"]
                .iter()
                .any(|t| is(*n, t))
        }) {
            required(node, "id", &mut errors);
            let key = (
                node.tag_name().name().to_string(),
                node.attribute("id").unwrap_or("").into(),
            );
            if index.insert(key, node).is_some() {
                errors.push(diagnostic(
                    node,
                    "SCL_DUPLICATE_TYPE",
                    "duplicate type identifier within type category",
                ));
            }
            let mut names = BTreeSet::new();
            for child in node
                .children()
                .filter(|n| ["DO", "SDO", "DA", "BDA"].iter().any(|t| is(*n, t)))
            {
                required(child, "name", &mut errors);
                if !names.insert(child.attribute("name")) {
                    errors.push(diagnostic(
                        child,
                        "SCL_DUPLICATE_MEMBER",
                        "duplicate type member name",
                    ));
                }
            }
        }
    }
    for node in index.values() {
        for child in node
            .children()
            .filter(|n| n.is_element() && n.tag_name().namespace() == Some(super::document::SCL_NS))
        {
            if let Some(kind) = type_target(child) {
                let id = child.attribute("type").unwrap_or("");
                if resolve_type(&index, kind, id).is_none() {
                    errors.push(diagnostic(
                        child,
                        "SCL_TYPE_REFERENCE",
                        format!("unresolved {kind} {id:?}"),
                    ));
                }
            }
        }
    }
    let mut done = BTreeSet::new();
    for key in index.keys() {
        check_cycle(key, &index, &mut BTreeSet::new(), &mut done, &mut errors);
    }
    let ieds: Vec<_> = children(root, "IED").collect();
    if ieds.is_empty() {
        errors.push(diagnostic(
            root,
            "SCL_NO_IED",
            "reference checking requires at least one IED",
        ));
    }
    let mut ied_names = BTreeSet::new();
    for ied in ieds {
        required(ied, "name", &mut errors);
        if !ied_names.insert(ied.attribute("name")) {
            errors.push(diagnostic(ied, "SCL_DUPLICATE_IED", "duplicate IED name"));
        }
        let mut ap_names = BTreeSet::new();
        for ap in children(ied, "AccessPoint") {
            required(ap, "name", &mut errors);
            if !ap_names.insert(ap.attribute("name")) {
                errors.push(diagnostic(
                    ap,
                    "SCL_DUPLICATE_AP",
                    "duplicate AccessPoint name",
                ));
            }
            for server in children(ap, "Server") {
                let mut ld_names = BTreeSet::new();
                for ld in children(server, "LDevice") {
                    required(ld, "inst", &mut errors);
                    if !ld_names.insert(ld.attribute("inst")) {
                        errors.push(diagnostic(ld, "SCL_DUPLICATE_LD", "duplicate LDevice inst"));
                    }
                    if children(ld, "LN0").count() != 1 {
                        errors.push(diagnostic(
                            ld,
                            "SCL_LN0_COUNT",
                            "LDevice must contain exactly one LN0",
                        ));
                    }
                }
            }
        }
    }
    let nodes = logical_nodes(doc);
    let mut identities = BTreeSet::new();
    for ln in &nodes {
        let target = LnTarget::of(*ln);
        if !identities.insert((
            target.ied,
            target.access_point,
            target.ld_inst,
            target.prefix,
            target.ln_class,
            target.ln_inst,
        )) {
            errors.push(diagnostic(
                *ln,
                "SCL_DUPLICATE_LN",
                "duplicate logical node identity",
            ));
        }
        let id = ln.attribute("lnType").unwrap_or("");
        match resolve_type(&index, "LNodeType", id) {
            None => errors.push(diagnostic(
                *ln,
                "SCL_LN_TYPE",
                format!("unresolved LNodeType {id:?}"),
            )),
            Some(t) if t.attribute("lnClass") != ln.attribute("lnClass") => errors.push(
                diagnostic(*ln, "SCL_LN_CLASS", "lnClass differs from LNodeType"),
            ),
            _ => {}
        }
        let mut datasets = BTreeSet::new();
        for ds in children(*ln, "DataSet") {
            required(ds, "name", &mut errors);
            if !datasets.insert(ds.attribute("name").unwrap_or("")) {
                errors.push(diagnostic(
                    ds,
                    "SCL_DUPLICATE_DATASET",
                    "duplicate DataSet name",
                ));
            }
            if children(ds, "FCDA").next().is_none() {
                errors.push(diagnostic(
                    ds,
                    "SCL_EMPTY_DATASET",
                    "DataSet must contain at least one FCDA",
                ));
            }
            for fcda in children(ds, "FCDA") {
                if let Err(message) = check_fcda(fcda, *ln, &nodes, &index) {
                    errors.push(diagnostic(fcda, "SCL_FCDA_REFERENCE", message));
                }
            }
        }
        let mut controls = BTreeSet::new();
        for cb in ln.children().filter(|n| {
            [
                "GSEControl",
                "ReportControl",
                "SampledValueControl",
                "LogControl",
            ]
            .iter()
            .any(|t| is(*n, t))
        }) {
            if !controls.insert((cb.tag_name().name(), cb.attribute("name"))) {
                errors.push(diagnostic(
                    cb,
                    "SCL_DUPLICATE_CONTROL",
                    "duplicate control block name",
                ));
            }
            if let Some(dataset) = cb.attribute("datSet") {
                if !datasets.contains(dataset) {
                    errors.push(diagnostic(
                        cb,
                        "SCL_CONTROL_DATASET",
                        format!("local DataSet {dataset:?} does not exist"),
                    ));
                }
            } else if is(cb, "GSEControl") {
                required(cb, "datSet", &mut errors);
            }
            if is(cb, "GSEControl")
                && cb
                    .attribute("confRev")
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_none()
            {
                errors.push(diagnostic(
                    cb,
                    "SCL_CONF_REV",
                    "GSEControl confRev must be an unsigned 32-bit integer",
                ));
            }
        }
    }
    errors.sort_by(|a, b| (a.line, a.column, &a.code).cmp(&(b.line, b.column, &b.code)));
    errors
}
