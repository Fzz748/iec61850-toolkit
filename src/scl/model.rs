use super::document::{SclDocument, children, is};
use crate::{Error, Result};
use roxmltree::Node;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LnTarget {
    pub ied: String,
    pub access_point: String,
    pub ld_inst: String,
    #[serde(default)]
    pub prefix: String,
    pub ln_class: String,
    #[serde(default)]
    pub ln_inst: String,
}

impl LnTarget {
    pub fn find<'a, 'i>(&self, doc: &'a SclDocument<'i>) -> Result<Node<'a, 'i>> {
        let found: Vec<_> = logical_nodes(doc)
            .into_iter()
            .filter(|n| Self::of(*n) == *self)
            .collect();
        match found.as_slice() {
            [node] => Ok(*node),
            [] => Err(Error::Edit(format!("logical node not found: {self:?}"))),
            _ => Err(Error::Edit(format!("ambiguous logical node: {self:?}"))),
        }
    }

    pub fn of(node: Node<'_, '_>) -> Self {
        let ancestor = |tag, attr| {
            node.ancestors()
                .find(|n| is(*n, tag))
                .and_then(|n| n.attribute(attr))
                .unwrap_or("")
                .to_owned()
        };
        Self {
            ied: ancestor("IED", "name"),
            access_point: ancestor("AccessPoint", "name"),
            ld_inst: ancestor("LDevice", "inst"),
            prefix: node.attribute("prefix").unwrap_or("").into(),
            ln_class: node.attribute("lnClass").unwrap_or("").into(),
            ln_inst: node.attribute("inst").unwrap_or("").into(),
        }
    }
}

/// Traverse only the actual IED model, never lookalike nodes inside Private extensions.
pub fn logical_nodes<'a, 'i>(doc: &'a SclDocument<'i>) -> Vec<Node<'a, 'i>> {
    let mut result = Vec::new();
    for ied in children(doc.xml.root_element(), "IED") {
        for ap in children(ied, "AccessPoint") {
            for server in children(ap, "Server") {
                for ld in children(server, "LDevice") {
                    result.extend(ld.children().filter(|n| is(*n, "LN0") || is(*n, "LN")));
                }
            }
        }
    }
    result
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fcda {
    pub ld_inst: String,
    #[serde(default)]
    pub prefix: String,
    pub ln_class: String,
    #[serde(default)]
    pub ln_inst: String,
    pub do_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub da_name: Option<String>,
    pub fc: String,
}

impl Fcda {
    pub fn from_node(node: Node<'_, '_>) -> Self {
        let attr = |a| node.attribute(a).unwrap_or("").to_string();
        Self {
            ld_inst: attr("ldInst"),
            prefix: attr("prefix"),
            ln_class: attr("lnClass"),
            ln_inst: attr("lnInst"),
            do_name: attr("doName"),
            da_name: node.attribute("daName").map(str::to_owned),
            fc: attr("fc"),
        }
    }
}
