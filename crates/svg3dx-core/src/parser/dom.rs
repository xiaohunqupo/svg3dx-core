use indexmap::IndexMap;

pub const S3DX_NS: &str = "urn:s3dx:draft:0.1";
pub const SVG_NS: &str = "http://www.w3.org/2000/svg";

#[derive(Debug, Clone)]
pub struct Document {
    pub svg3d: bool,
    pub root: Element,
    /// id → element index path into the tree (for <s3dx:use> resolution).
    pub id_index: IndexMap<String, Vec<usize>>,
    pub class_rules: IndexMap<String, IndexMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct Element {
    pub ns: Namespace,
    pub name: String,
    pub attrs: IndexMap<String, String>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Svg,
    S3dx,
    Other,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(|s| s.as_str())
    }

    /// Namespaced s3dx attribute (e.g. `s3dx:transform`), stored as `s3dx:{local}`.
    pub fn s3dx_attr(&self, local: &str) -> Option<&str> {
        self.attr(&format!("s3dx:{local}"))
    }

    pub fn id(&self) -> Option<&str> {
        self.attr("id")
    }

    pub fn classes(&self) -> Vec<String> {
        self.attr("class")
            .map(|c| {
                c.split_whitespace()
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn is_s3dx(&self, local: &str) -> bool {
        self.ns == Namespace::S3dx && self.name == local
    }

    pub fn is_svg(&self, local: &str) -> bool {
        self.ns == Namespace::Svg && self.name == local
    }

    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }
}

impl Document {
    pub fn find_by_id(&self, id: &str) -> Option<&Element> {
        let path = self.id_index.get(id)?;
        let mut cur = &self.root;
        for (i, &idx) in path.iter().enumerate() {
            let child = cur.children.get(idx)?;
            match child {
                Node::Element(e) => {
                    if i + 1 == path.len() {
                        return Some(e);
                    }
                    cur = e;
                }
                _ => return None,
            }
        }
        None
    }
}
