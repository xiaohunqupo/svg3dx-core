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
        PrimitiveKind::SectorCylinder => &["diameter", "bottomCenterPt", "topCenterPt", "angle"],
        PrimitiveKind::Line => &["from", "to"],
        PrimitiveKind::Arc => &[
            "form",
            "center",
            "normal",
            "radius",
            "xDir",
            "startAngle",
            "angle",
            "from",
            "to",
        ],
        PrimitiveKind::Path => &["form", "points", "segments"],
        PrimitiveKind::Extrude => &["origin", "xDir", "normal", "dir", "length", "section"],
        // Axis = body +Z; section pose/transform on child <s3dx:section>.
        PrimitiveKind::Revolve => &["angle", "solid", "section"],
        PrimitiveKind::Loft => &["solid", "blend", "sections"],
        // Trajectory is always a child `<s3dx:path>` (no `form` / inline `path`).
        PrimitiveKind::Sweep => &[
            "orient",
            "refDir",
            "solid",
            "outerCorner",
            "innerCorner",
            "outerCornerRadius",
            "innerCornerRadius",
            "miterLimit",
            "section",
        ],
        PrimitiveKind::Pipe => &[
            "form",
            "radius",
            "innerRadius",
            "outerCorner",
            "innerCorner",
            "outerCornerRadius",
            "innerCornerRadius",
            "miterLimit",
            "path",
        ],
        // `href` is otherwise a meta attr (s3dx:use); listed here so filter_params keeps it.
        PrimitiveKind::Gltf => &["href"],
        PrimitiveKind::Other => &[],
    }
}

/// Attributes that are never geometry params (SVG / host / 3D transform / appearance).
pub fn is_meta_attr(name: &str) -> bool {
    matches!(
        name,
        "transform"
            | "s3dx:transform"
            | "id"
            | "class"
            | "style"
            | "href"
            | "fill"
            | "material"
            | "opacity"
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
        let listed = known.iter().any(|n| *n == k);
        // Meta attrs are skipped unless this kind lists them (e.g. gltf/@href).
        if is_meta_attr(&k) && !listed {
            continue;
        }
        if !(listed || k == "kind_name") {
            // Keep unknown attrs so newer documents still work with older
            // known_attrs lists (viewer may understand them); only warn.
            warnings.push(format!(
                "unknown attribute '{k}' on s3dx:{} — kept",
                kind.as_str()
            ));
        }
        out.insert(k, v);
    }
    out
}
