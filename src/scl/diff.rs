use super::document::{SCL_NS, SclDocument, is};
use roxmltree::Node;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct Change {
    pub kind: &'static str,
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

fn snapshot(
    node: Node<'_, '_>,
    source: &str,
    parent: &str,
    ordinal: usize,
    result: &mut BTreeMap<String, Value>,
) {
    let name = format!(
        "{{{}}}{}",
        node.tag_name().namespace().unwrap_or(""),
        node.tag_name().name()
    );
    let identity = if is(node, "LN") || is(node, "LN0") {
        format!(
            "prefix={:?},class={:?},inst={:?}",
            node.attribute("prefix").unwrap_or(""),
            node.attribute("lnClass"),
            node.attribute("inst")
        )
    } else if is(node, "FCDA") || is(node, "Val") {
        format!("position={ordinal}")
    } else {
        ["name", "id", "inst", "ldInst", "cbName", "type", "ord"]
            .into_iter()
            .find_map(|a| node.attribute(a).map(|v| format!("{a}={v:?}")))
            .unwrap_or_else(|| format!("position={ordinal}"))
    };
    let key = format!("{parent}/{name}[{identity}]");
    // Keep colliding identities visible even for invalid documents.
    let key = if result.contains_key(&key) {
        format!("{key}[duplicate={ordinal}]")
    } else {
        key
    };
    if is(node, "Private") || node.tag_name().namespace() != Some(SCL_NS) {
        result.insert(key, json!({"extension_xml": &source[node.range()]}));
        return;
    }
    let attrs: BTreeMap<_, _> = node
        .attributes()
        .map(|a| {
            (
                format!("{{{}}}{}", a.namespace().unwrap_or(""), a.name()),
                a.value(),
            )
        })
        .collect();
    let text: String = node
        .children()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect();
    let text = if text.trim().is_empty() && node.children().any(|n| n.is_element()) {
        ""
    } else {
        &text
    };
    result.insert(key.clone(), json!({"attributes": attrs, "text": text}));
    let mut ordinals = BTreeMap::new();
    for child in node.children().filter(|n| n.is_element()) {
        let ordinal = ordinals
            .entry((child.tag_name().namespace(), child.tag_name().name()))
            .or_insert(0usize);
        *ordinal += 1;
        snapshot(child, source, &key, *ordinal, result);
    }
}

pub fn semantic(before: &SclDocument<'_>, after: &SclDocument<'_>) -> Vec<Change> {
    let mut old = BTreeMap::new();
    let mut new = BTreeMap::new();
    snapshot(before.xml.root_element(), before.source, "", 1, &mut old);
    snapshot(after.xml.root_element(), after.source, "", 1, &mut new);
    let mut changes = Vec::new();
    for (path, value) in &old {
        match new.get(path) {
            None => changes.push(Change {
                kind: "removed",
                path: path.clone(),
                before: Some(value.clone()),
                after: None,
            }),
            Some(other) if value != other => changes.push(Change {
                kind: "modified",
                path: path.clone(),
                before: Some(value.clone()),
                after: Some(other.clone()),
            }),
            _ => {}
        }
    }
    for (path, value) in &new {
        if !old.contains_key(path) {
            changes.push(Change {
                kind: "added",
                path: path.clone(),
                before: None,
                after: Some(value.clone()),
            });
        }
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}
