use crate::{Error, Result};
use roxmltree::{Document, Node, ParsingOptions};
use sha2::{Digest, Sha256};

pub const SCL_NS: &str = "http://www.iec.ch/61850/2003/SCL";
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

/// Borrows the complete UTF-8 source: the parsed view is never used to reserialize it.
pub struct SclDocument<'a> {
    pub(crate) source: &'a str,
    pub(crate) xml: Document<'a>,
}

impl<'a> SclDocument<'a> {
    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn xml(&self) -> &Document<'a> {
        &self.xml
    }

    pub fn parse(source: &'a str) -> Result<Self> {
        if source.len() > MAX_BYTES {
            return Err(Error::Input("SCL exceeds the 32 MiB input limit".into()));
        }
        // A byte-preserving editor must not pretend that decoded text retains original offsets.
        let head = source.trim_start_matches('\u{feff}');
        if head.starts_with("<?xml") {
            let declaration = head.split("?>").next().unwrap_or(head);
            if let Some((_, rest)) = declaration.split_once("encoding") {
                let rest = rest
                    .trim_start()
                    .strip_prefix('=')
                    .unwrap_or(rest)
                    .trim_start();
                let quote = rest.chars().next().unwrap_or(' ');
                let encoding = rest
                    .get(1..)
                    .unwrap_or("")
                    .split(quote)
                    .next()
                    .unwrap_or("");
                if !encoding.eq_ignore_ascii_case("utf-8") && !encoding.eq_ignore_ascii_case("utf8")
                {
                    return Err(Error::Input(format!(
                        "unsupported XML encoding {encoding:?}; use UTF-8"
                    )));
                }
            }
        }
        let xml = Document::parse_with_options(
            source,
            ParsingOptions {
                allow_dtd: false,
                nodes_limit: 1_000_000,
                ..ParsingOptions::default()
            },
        )
        .map_err(|e| Error::Input(format!("XML: {e}")))?;
        if !is(xml.root_element(), "SCL") {
            return Err(Error::Input(format!(
                "expected SCL root in namespace {SCL_NS}"
            )));
        }
        if xml
            .descendants()
            .any(|n| n.ancestors().take(130).count() > 128)
        {
            return Err(Error::Input("XML nesting exceeds 128 levels".into()));
        }
        Ok(Self { source, xml })
    }

    pub fn require_edit_profile(&self) -> Result<()> {
        let root = self.xml.root_element();
        if root.attribute("version") != Some("2007")
            || root.attribute("revision") != Some("B")
            || root.attribute("release") != Some("4")
        {
            return Err(Error::Edit("editing requires version=2007, revision=B, release=4; other versions are inspect-only".into()));
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        digest(self.source.as_bytes())
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn is(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element()
        && node.tag_name().namespace() == Some(SCL_NS)
        && node.tag_name().name() == name
}

pub(crate) fn children<'a, 'i>(
    node: Node<'a, 'i>,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(move |n| is(*n, name))
}

pub(crate) fn path(node: Node<'_, '_>) -> String {
    let mut parts: Vec<_> = node
        .ancestors()
        .filter(|n| n.is_element())
        .map(|n| {
            let key = ["name", "id", "inst", "ldInst", "cbName"]
                .into_iter()
                .find_map(|a| n.attribute(a).map(|v| format!("[@{a}={v:?}]")))
                .unwrap_or_default();
            let ln = if is(n, "LN") || is(n, "LN0") {
                format!(
                    "[@lnClass={:?}][@prefix={:?}]",
                    n.attribute("lnClass").unwrap_or(""),
                    n.attribute("prefix").unwrap_or("")
                )
            } else {
                String::new()
            };
            format!("{}{key}{ln}", n.tag_name().name())
        })
        .collect();
    parts.reverse();
    format!("/{}", parts.join("/"))
}

/// XML QName as written, so inserted children inherit the parent's prefix.
pub(crate) fn qname<'a>(doc: &'a SclDocument<'_>, node: Node<'_, '_>) -> &'a str {
    let tail = &doc.source[node.range().start + 1..];
    tail.split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or("")
}

pub(crate) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
