use indexmap::IndexMap;
use roxmltree::{Document as XmlDoc, Node};

use super::dom::{Document, Element, Namespace, Node as DomNode, S3DX_NS, SVG_NS};
use crate::error::{Error, Result};

pub fn parse_xml(input: &str) -> Result<Document> {
    let xml = XmlDoc::parse(input).map_err(|e| Error::Xml(e.to_string()))?;
    let root_node = xml.root_element();
    if root_node.tag_name().name() != "svg" {
        return Err(Error::NotSvg);
    }

    let svg3d = root_node
        .attribute("svg3d")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let class_rules = collect_class_rules(&root_node);
    let root = convert_element(root_node)?;
    let mut doc = Document {
        svg3d,
        root,
        id_index: IndexMap::new(),
        class_rules,
    };
    index_ids(&doc.root, &mut Vec::new(), &mut doc.id_index);
    Ok(doc)
}

fn convert_element(node: Node<'_, '_>) -> Result<Element> {
    let ns = match node.tag_name().namespace() {
        Some(S3DX_NS) => Namespace::S3dx,
        Some(SVG_NS) | None => Namespace::Svg,
        Some(_) => Namespace::Other,
    };
    let mut attrs = IndexMap::new();
    for attr in node.attributes() {
        // Unprefixed attrs keep local names. s3dx-namespaced attrs (e.g. s3dx:transform)
        // are stored as `s3dx:{local}` so they never collide with SVG's `transform`.
        let key = match attr.namespace() {
            Some(S3DX_NS) => format!("s3dx:{}", attr.name()),
            None => attr.name().to_string(),
            Some(ns) => format!("{{{ns}}}{}", attr.name()),
        };
        attrs.insert(key, attr.value().to_string());
    }
    let mut children = Vec::new();
    for child in node.children() {
        if child.is_element() {
            children.push(DomNode::Element(convert_element(child)?));
        } else if child.is_text() {
            let t = child.text().unwrap_or("").to_string();
            if !t.trim().is_empty() {
                children.push(DomNode::Text(t));
            }
        }
    }
    Ok(Element {
        ns,
        name: node.tag_name().name().to_string(),
        attrs,
        children,
    })
}

fn index_ids(el: &Element, path: &mut Vec<usize>, index: &mut IndexMap<String, Vec<usize>>) {
    if let Some(id) = el.id() {
        index.insert(id.to_string(), path.clone());
    }
    for (i, child) in el.children.iter().enumerate() {
        if let DomNode::Element(e) = child {
            path.push(i);
            index_ids(e, path, index);
            path.pop();
        }
    }
}

/// Simplified CSS subset: `.classname { prop: value; }` (custom props + presentation).
fn collect_class_rules(root: &Node<'_, '_>) -> IndexMap<String, IndexMap<String, String>> {
    let mut rules = IndexMap::new();
    collect_style_recursive(root, &mut rules);
    rules
}

fn collect_style_recursive(
    node: &Node<'_, '_>,
    rules: &mut IndexMap<String, IndexMap<String, String>>,
) {
    if node.is_element() && node.tag_name().name() == "style" {
        if let Some(text) = node.text() {
            parse_style_block(text, rules);
        }
        // also concatenate child text nodes
        let mut buf = String::new();
        for c in node.children() {
            if c.is_text() {
                buf.push_str(c.text().unwrap_or(""));
            }
        }
        if !buf.is_empty() {
            parse_style_block(&buf, rules);
        }
    }
    for child in node.children() {
        if child.is_element() {
            collect_style_recursive(&child, rules);
        }
    }
}

fn parse_style_block(text: &str, rules: &mut IndexMap<String, IndexMap<String, String>>) {
    // strip comments
    let mut cleaned = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(c2) = chars.next() {
                if c2 == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            cleaned.push(c);
        }
    }

    let mut rest = cleaned.as_str();
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].trim();
        rest = &rest[open + 1..];
        let close = match rest.find('}') {
            Some(i) => i,
            None => break,
        };
        let body = &rest[..close];
        rest = &rest[close + 1..];

        // only class selectors
        for sel in selector.split(',') {
            let sel = sel.trim();
            if let Some(class) = sel.strip_prefix('.') {
                if class
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    let props = parse_style_decls(body);
                    let entry = rules.entry(class.to_string()).or_default();
                    for (k, v) in props {
                        entry.insert(k, v);
                    }
                }
            }
        }
    }
}

/// Parse CSS declarations; keep original property names (`--D`, `fill`, …).
fn parse_style_decls(body: &str) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    for decl in body.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let Some((name, value)) = decl.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() {
            continue;
        }
        out.insert(name.to_string(), value.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let doc = parse_xml(
            r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
                 xmlns:s3dx="urn:s3dx:draft:0.1">
               <style>.tanks { --D: 200; --R: 50; }</style>
               <s3dx:box length="1" width="2" height="3"
                         s3dx:transform="matrix(1 0 0 0 1 0 0 0 1 10 0 0)"/>
             </svg>"##,
        )
        .unwrap();
        assert!(doc.svg3d);
        assert_eq!(
            doc.class_rules.get("tanks").unwrap().get("--D").unwrap(),
            "200"
        );
        let box_el = doc.root.child_elements().find(|e| e.is_s3dx("box")).unwrap();
        assert!(box_el.s3dx_attr("transform").is_some());
        assert!(box_el.attr("transform").is_none());
    }
}
