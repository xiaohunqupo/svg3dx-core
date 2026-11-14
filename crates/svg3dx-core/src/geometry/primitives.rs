use indexmap::IndexMap;

use crate::instruction::PrimitiveKind;
use crate::params::types::Value;

/// Known attribute names for each primitive (spec §3.2 — lowerCamelCase, case-sensitive).
pub fn known_attrs(kind: PrimitiveKind) -> &'static [&'static str] {
    match kind {
        PrimitiveKind::Box => &["length", "width", "height", "lengthVec", "widthVec"],
        PrimitiveKind::Cylinder => &["diameter", "bottomCenterPt", "topCenterPt"],
        PrimitiveKind::Sphere => &["radius", "center"],
        PrimitiveKind::Torus => &[
            "diameter1",
            "diameter2",
            "angle",
            "torusCenterPt",
            "normal",
        ],
        PrimitiveKind::Cone => &[
            "bottomDiameter",
            "topDiameter",
            "bottomCenterPt",
            "topCenterPt",
        ],
        PrimitiveKind::Wedge => &[
            "length",
            "width",
            "height",
            "lengthVec",
            "widthVec",
            "topLength",
        ],
        PrimitiveKind::Pyramid => &[
            "length",
            "width",
            "height",
            "lengthVec",
            "widthVec",
            "topLength",
            "topWidth",
        ],
        PrimitiveKind::Prism => &["points", "height", "normal"],
        PrimitiveKind::CutCylinder => &["diameter", "bottomCenterPt", "topCenterPt", "angle"],
        PrimitiveKind::Extrude => &["dir", "length", "profile"],
        PrimitiveKind::Revolve => &["axisOrigin", "axisDir", "angle", "profile"],
        PrimitiveKind::Loft => &["solid", "ruled", "profiles"],
        PrimitiveKind::Sweep => &["orient", "refDir", "solid", "path", "profile"],
        PrimitiveKind::Pipe => &["radius", "innerRadius", "path"],
        PrimitiveKind::Mesh => &[
            "mode",
            "position",
            "normal",
            "texcoord",
            "color",
            "colorSize",
            "index",
            "indexBase",
        ],
        PrimitiveKind::Triangles => &["point", "normal", "index"],
        PrimitiveKind::Other => &[],
    }
}

/// Attributes that are never geometry params (SVG / host / 3D transform).
pub fn is_meta_attr(name: &str) -> bool {
    matches!(
        name,
        "transform" | "s3dx:transform" | "id" | "class" | "style" | "href"
    ) || name.starts_with("s3dx:")
        || (name.starts_with('{') && name.contains('}'))
}

pub fn filter_params(
    kind: PrimitiveKind,
    attrs: IndexMap<String, Value>,
    warnings: &mut Vec<String>,
) -> IndexMap<String, Value> {
    if kind == PrimitiveKind::Other {
        return attrs;
    }
    let known = known_attrs(kind);
    let mut out = IndexMap::new();
    for (k, v) in attrs {
        if known.iter().any(|n| *n == k) || k == "kind_name" {
            out.insert(k, v);
        } else if !is_meta_attr(&k) {
            warnings.push(format!(
                "unknown attribute '{k}' on s3dx:{} — ignored",
                kind.as_str()
            ));
        }
    }
    out
}
