pub mod dom;
pub mod xml;

pub use dom::{Document, Element, Namespace, Node, S3DX_NS};
pub use xml::parse_xml;
