use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::geometry::transform::Mat4x3;
use crate::params::types::Value;

/// Top-level evaluation output consumed by host renderers / kernels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvaluationResult {
    pub instructions: Vec<GeometryInstruction>,
    pub models: Vec<ModelDesc>,
    pub active_model: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelDesc {
    pub id: String,
    pub mode: String,
    pub active: bool,
    pub transform: Mat4x3,
    /// Group ids referenced by this model's `<s3dx:use>` children (after transform).
    pub uses: Vec<ModelUse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelUse {
    pub group_id: String,
    pub transform: Mat4x3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GeometryInstruction {
    Primitive {
        node_id: String,
        kind: PrimitiveKind,
        params: IndexMap<String, Value>,
        /// Resolved CSS at this node (`--D`, `fill`, …).
        styles: IndexMap<String, Value>,
        transform: Mat4x3,
    },
    BoolOp {
        node_id: String,
        op: BoolOpKind,
        styles: IndexMap<String, Value>,
        operands: Vec<GeometryInstruction>,
    },
    Instance {
        node_id: String,
        group_id: String,
        styles: IndexMap<String, Value>,
        transform: Mat4x3,
    },
    /// A named group expanded in-place (used when emitting defs content).
    Group {
        node_id: String,
        id: Option<String>,
        styles: IndexMap<String, Value>,
        children: Vec<GeometryInstruction>,
        transform: Mat4x3,
    },
}

impl GeometryInstruction {
    pub fn node_id(&self) -> &str {
        match self {
            Self::Primitive { node_id, .. }
            | Self::BoolOp { node_id, .. }
            | Self::Instance { node_id, .. }
            | Self::Group { node_id, .. } => node_id,
        }
    }

    pub fn styles(&self) -> &IndexMap<String, Value> {
        match self {
            Self::Primitive { styles, .. }
            | Self::BoolOp { styles, .. }
            | Self::Instance { styles, .. }
            | Self::Group { styles, .. } => styles,
        }
    }

    pub fn transform(&self) -> Option<&Mat4x3> {
        match self {
            Self::Primitive { transform, .. }
            | Self::Instance { transform, .. }
            | Self::Group { transform, .. } => Some(transform),
            Self::BoolOp { .. } => None,
        }
    }

    pub fn children(&self) -> &[GeometryInstruction] {
        match self {
            Self::Group { children, .. } | Self::BoolOp { operands: children, .. } => children,
            _ => &[],
        }
    }

    /// Prefix every `node_id` in this subtree (used when instantiating via `<s3dx:use>`).
    pub fn prefix_node_ids(&mut self, prefix: &str) {
        match self {
            Self::Primitive { node_id, .. } | Self::Instance { node_id, .. } => {
                *node_id = format!("{prefix}/{node_id}");
            }
            Self::BoolOp {
                node_id, operands, ..
            } => {
                *node_id = format!("{prefix}/{node_id}");
                for c in operands {
                    c.prefix_node_ids(prefix);
                }
            }
            Self::Group {
                node_id, children, ..
            } => {
                *node_id = format!("{prefix}/{node_id}");
                for c in children {
                    c.prefix_node_ids(prefix);
                }
            }
        }
    }

    /// Depth-first flatten including self.
    pub fn walk<'a>(&'a self) -> Box<dyn Iterator<Item = &'a GeometryInstruction> + 'a> {
        Box::new(std::iter::once(self).chain(self.children().iter().flat_map(|c| c.walk())))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum PrimitiveKind {
    Box,
    Cylinder,
    Sphere,
    Torus,
    Cone,
    Wedge,
    Pyramid,
    Prism,
    #[serde(rename = "sector-cylinder")]
    SectorCylinder,
    Line,
    Arc,
    Path,
    Extrude,
    Revolve,
    Loft,
    Sweep,
    Pipe,
    Mesh,
    Triangles,
    /// Unknown / future primitive; params preserved as-is.
    Other,
}

impl PrimitiveKind {
    pub fn from_local_name(name: &str) -> Self {
        match name {
            "box" => Self::Box,
            "cylinder" => Self::Cylinder,
            "sphere" => Self::Sphere,
            "torus" => Self::Torus,
            "cone" => Self::Cone,
            "wedge" => Self::Wedge,
            "pyramid" => Self::Pyramid,
            "prism" => Self::Prism,
            // Spec name is sector-cylinder; cutcylinder kept as a read alias.
            "sector-cylinder" | "cutcylinder" => Self::SectorCylinder,
            "line" => Self::Line,
            "arc" => Self::Arc,
            "path" => Self::Path,
            // Spec: polylinestretch is a compatibility alias for extrude.
            "extrude" | "polylinestretch" => Self::Extrude,
            "revolve" => Self::Revolve,
            "loft" => Self::Loft,
            "sweep" => Self::Sweep,
            "pipe" => Self::Pipe,
            "mesh" => Self::Mesh,
            "triangles" => Self::Triangles,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Cylinder => "cylinder",
            Self::Sphere => "sphere",
            Self::Torus => "torus",
            Self::Cone => "cone",
            Self::Wedge => "wedge",
            Self::Pyramid => "pyramid",
            Self::Prism => "prism",
            Self::SectorCylinder => "sector-cylinder",
            Self::Line => "line",
            Self::Arc => "arc",
            Self::Path => "path",
            Self::Extrude => "extrude",
            Self::Revolve => "revolve",
            Self::Loft => "loft",
            Self::Sweep => "sweep",
            Self::Pipe => "pipe",
            Self::Mesh => "mesh",
            Self::Triangles => "triangles",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum BoolOpKind {
    Union,
    Subtract,
    Intersect,
}

impl BoolOpKind {
    pub fn from_local_name(name: &str) -> Option<Self> {
        match name {
            "bool-union" => Some(Self::Union),
            "bool-subtract" => Some(Self::Subtract),
            "bool-intersect" => Some(Self::Intersect),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Union => "bool-union",
            Self::Subtract => "bool-subtract",
            Self::Intersect => "bool-intersect",
        }
    }
}
