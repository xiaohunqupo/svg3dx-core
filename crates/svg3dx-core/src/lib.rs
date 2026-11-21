//! svg3dx-core — kernel-agnostic parser & evaluator for the svg3dx format.
//!
//! Pipeline: XML → DOM → parameter scopes → loop/condition expansion →
//! geometry instruction sequence. No meshing, no rendering, no CAD kernel.

pub mod diff;
pub mod error;
pub mod eval;
pub mod geometry;
pub mod instruction;
pub mod package;
pub mod params;
pub mod parser;
pub mod session;

pub use diff::{ChangeFields, NodeChange, NodeChangeKind};
pub use error::{Error, Result, Warning};
pub use eval::{evaluate_document, EvalOptions};
pub use instruction::{
    BoolOpKind, EvaluationResult, GeometryInstruction, MaterialDesc, ModelDesc, ModelUse,
    PrimitiveKind,
};
pub use package::{
    normalize_package_path, open_document_bytes, open_svg3dxz, write_svg3dxz, Svg3dxPackage,
    MAIN_ENTRY,
};
pub use params::{
    externalizable_root_names, externalizable_root_params, filter_external_params,
    is_direct_param_value, ParamScope, Value,
};
pub use parser::{parse_xml, Document};
pub use session::{update_to_json, DocumentSession, ParamDesc, UpdateResult};

use std::collections::HashMap;

/// Parse an svg3dx document and evaluate it to a geometry instruction sequence.
pub fn parse_and_evaluate(input: &str) -> Result<EvaluationResult> {
    parse_and_evaluate_with(input, EvalOptions::default())
}

/// Parse and evaluate with external parameters / model override.
pub fn parse_and_evaluate_with(input: &str, opts: EvalOptions) -> Result<EvaluationResult> {
    let doc = parse_xml(input)?;
    evaluate_document(&doc, opts)
}

/// Convenience: external params as string map (values parsed per spec §2.4).
pub fn parse_and_evaluate_with_params(
    input: &str,
    params: HashMap<String, String>,
    model: Option<String>,
) -> Result<EvaluationResult> {
    let external = params
        .into_iter()
        .map(|(k, v)| (k, Value::parse_literal(&v)))
        .collect();
    parse_and_evaluate_with(
        input,
        EvalOptions {
            external,
            model,
        },
    )
}

/// Serialize evaluation result to JSON (FFI / debugging).
pub fn to_json(result: &EvaluationResult) -> Result<String> {
    serde_json::to_string_pretty(result).map_err(|e| Error::Internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_primitive() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="g1">
              <s3dx:box length="1" width="2" height="3"
                        lengthVec="1 0 0" widthVec="0 1 0"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g1"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.active_model.as_deref(), Some("m"));
        assert!(!result.instructions.is_empty());
    }

    #[test]
    fn gltf_href_in_params() {
        use crate::instruction::{GeometryInstruction, PrimitiveKind};
        use crate::params::types::Value;
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="g1">
              <s3dx:gltf href="assets/bunny.glb"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g1"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let found = result.instructions.iter().any(|i| match i {
            GeometryInstruction::Group { children, .. } => children.iter().any(|c| matches!(
                c,
                GeometryInstruction::Primitive {
                    kind: PrimitiveKind::Gltf,
                    params,
                    ..
                } if matches!(params.get("href"), Some(Value::String(s)) if s == "assets/bunny.glb")
            )),
            _ => false,
        });
        assert!(found, "expected gltf primitive with href param: {:?}", result.instructions);
    }

    #[test]
    fn root_custom_props_inherit() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>
            :root { --H: 100; }
            .box { --W: 20; }
          </style>
          <defs>
            <g id="g" class="box">
              <s3dx:box length="1" width="1" height="1"
                        lengthVec="1 0 0" widthVec="0 1 0">
                <s3dx:exp length="var(--W)" width="var(--W)" height="var(--H)"/>
              </s3dx:box>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let json = to_json(&result).unwrap();
        assert!(json.contains("\"height\": 100") || json.contains("\"height\":100"));
        assert!(json.contains("\"length\": 20") || json.contains("\"length\":20"));
    }

    #[test]
    fn exp_override_and_condition() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>.tanks { --R: 50; --D: 200; }</style>
          <defs>
            <g id="tanks" class="tanks">
              <s3dx:sphere radius="60" center="0 0 0">
                <s3dx:exp radius="var(--R)"/>
              </s3dx:sphere>
              <s3dx:if test="var(--R) = 50">
                <s3dx:box length="1" width="1" height="1"
                          lengthVec="1 0 0" widthVec="0 1 0"/>
              </s3dx:if>
              <s3dx:if test="var(--R) &lt; 50">
                <s3dx:box length="9" width="9" height="9"
                          lengthVec="1 0 0" widthVec="0 1 0"/>
              </s3dx:if>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#tanks"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        let json = to_json(&result).unwrap();
        assert!(json.contains("\"radius\": 50") || json.contains("\"radius\":50"));
        assert!(json.contains("\"length\": 1") || json.contains("\"length\":1"));
        // False branch is still emitted (inactive) for editor ghost preview.
        assert!(json.contains("\"inactive\":true") || json.contains("\"inactive\": true"));
        assert!(json.contains("\"length\": 9") || json.contains("\"length\":9"));
    }

    #[test]
    fn loop_range_expands() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="pillars">
              <s3dx:loop-range start="0" end="4" step="1">
                <s3dx:box length="20" width="20" height="200"
                          lengthVec="1 0 0" widthVec="0 1 0"
                          s3dx:transform="matrix(1 0 0 0 1 0 0 0 1 calc(var(--loop-value) * 50) 0 0)"/>
              </s3dx:loop-range>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#pillars"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let boxes: Vec<_> = result
            .instructions
            .iter()
            .flat_map(|i| flatten(i))
            .filter(|i| {
                matches!(
                    i,
                    GeometryInstruction::Primitive {
                        kind: PrimitiveKind::Box,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(boxes.len(), 4);
    }

    #[test]
    fn if_transform_stays_on_wrapper_not_children() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="main">
              <s3dx:if test="1 = 1" s3dx:transform="matrix(1 0 0 0 1 0 0 0 1 50 0 0)">
                <s3dx:box length="10" width="10" height="10"
                          lengthVec="1 0 0" widthVec="0 1 0"/>
              </s3dx:if>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#main"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let root = result.instructions.first().expect("main group");
        let GeometryInstruction::Group { children, .. } = root else {
            panic!("expected group");
        };
        let GeometryInstruction::Group {
            transform: if_xf,
            children: if_kids,
            ..
        } = children.first().expect("if wrapper")
        else {
            panic!("expected if group");
        };
        assert!(
            (if_xf.m[9] - 50.0).abs() < 1e-9,
            "if wrapper tx {:?}",
            if_xf.m
        );
        let prim = if_kids.iter().find_map(|c| match c {
            GeometryInstruction::Primitive { transform, .. } => Some(transform),
            _ => None,
        });
        let prim_xf = prim.expect("box under if");
        assert!(
            prim_xf.m[9].abs() < 1e-9,
            "child must stay local-identity, got {:?}",
            prim_xf.m
        );
    }

    #[test]
    fn named_group_transform_stays_on_wrapper() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="main" s3dx:transform="matrix(1 0 0 0 1 0 0 0 1 25 0 0)">
              <s3dx:box length="10" width="10" height="10"
                        lengthVec="1 0 0" widthVec="0 1 0"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#main"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let GeometryInstruction::Group {
            transform,
            children,
            ..
        } = result.instructions.first().expect("main")
        else {
            panic!("expected group");
        };
        assert!((transform.m[9] - 25.0).abs() < 1e-9, "{:?}", transform.m);
        let prim = children.iter().find_map(|c| match c {
            GeometryInstruction::Primitive { transform, .. } => Some(transform),
            _ => None,
        });
        assert!(
            prim.expect("box").m[9].abs() < 1e-9,
            "box must not bake group tx"
        );
    }

    #[test]
    fn s3dx_transform_trs_with_params() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>:root { --x: 100; --yaw: 0; }</style>
          <defs>
            <g id="main">
              <s3dx:box length="20" width="20" height="20"
                        lengthVec="1 0 0" widthVec="0 1 0"
                        s3dx:transform="translate(var(--x) 0 0) rotate(0 var(--yaw) 0) scale(1 1 1)"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#main"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let prim = result
            .instructions
            .iter()
            .flat_map(|i| flatten(i))
            .find(|i| {
                matches!(
                    i,
                    GeometryInstruction::Primitive {
                        kind: PrimitiveKind::Box,
                        ..
                    }
                )
            })
            .expect("box");
        let xf = prim.transform().unwrap();
        assert!((xf.m[9] - 100.0).abs() < 1e-9, "{:?}", xf.m);
    }

    #[test]
    fn trs_translate_var_equals_matrix() {
        let mk = |xf: &str| {
            format!(
                r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg" xmlns:s3dx="urn:s3dx:draft:0.1">
              <style>:root {{ --gap: 40; }}</style>
              <defs><g id="main">
                <s3dx:box length="10" width="10" height="10" lengthVec="1 0 0" widthVec="0 1 0"
                  s3dx:transform="{xf}"/>
              </g></defs>
              <s3dx:model id="m" mode="solid" active="true"><s3dx:use href="#main"/></s3dx:model>
            </svg>"##,
                xf = xf
            )
        };
        let box_xf = |result: &EvaluationResult| {
            result
                .instructions
                .iter()
                .flat_map(|i| flatten(i))
                .find_map(|i| match i {
                    GeometryInstruction::Primitive {
                        kind: PrimitiveKind::Box,
                        transform,
                        ..
                    } => Some(*transform),
                    _ => None,
                })
                .expect("box")
        };
        let a = parse_and_evaluate(&mk(
            "translate(var(--gap) var(--gap) var(--gap))",
        ))
        .unwrap();
        let b = parse_and_evaluate(&mk(
            "matrix(1 0 0 0 1 0 0 0 1 var(--gap) var(--gap) var(--gap))",
        ))
        .unwrap();
        assert!(a.warnings.is_empty(), "{:?}", a.warnings);
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        let ta = box_xf(&a);
        let tb = box_xf(&b);
        for i in 0..12 {
            assert!(
                (ta.m[i] - tb.m[i]).abs() < 1e-9,
                "m[{i}] {:?} vs {:?}",
                ta.m,
                tb.m
            );
        }
        assert!((ta.m[9] - 40.0).abs() < 1e-9, "{:?}", ta.m);
    }

    #[test]
    fn s3dx_transform_not_svg_transform() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="g1" transform="translate(100,0)"
               s3dx:transform="matrix(1 0 0 0 1 0 0 0 1 10 20 30)">
              <s3dx:box length="1" width="1" height="1"
                        lengthVec="1 0 0" widthVec="0 1 0"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g1"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let json = to_json(&result).unwrap();
        // 3D matrix translation should be 10,20,30 — not confused with SVG translate.
        assert!(json.contains("10") && json.contains("20") && json.contains("30"));
    }

    #[test]
    fn bool_subtract_tree() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="g">
              <s3dx:bool-subtract>
                <s3dx:sphere radius="60" center="0 0 0"/>
                <s3dx:sphere radius="20" center="0 0 0"/>
              </s3dx:bool-subtract>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        let has_bool = flatten(&result.instructions[0]).any(|i| {
            matches!(
                i,
                GeometryInstruction::BoolOp {
                    op: BoolOpKind::Subtract,
                    ..
                }
            )
        });
        assert!(has_bool);
    }

    #[test]
    fn path_with_line_arc_segments() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="g">
              <s3dx:path>
                <s3dx:line from="0 0 0" to="200 0 0"/>
                <s3dx:arc center="200 0 75" normal="1 0 0" radius="75"
                          from="200 0 0" to="200 0 150"/>
              </s3dx:path>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let json = to_json(&result).unwrap();
        assert!(json.contains("\"path\""));
        assert!(json.contains("\"segments\""));
        assert!(json.contains("\"line\""));
        assert!(json.contains("\"arc\""));
    }

    #[test]
    fn extrude_with_section() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="g">
              <s3dx:extrude origin="0 0 0" normal="0 0 1" length="60">
                <s3dx:section>
                  <path d="M 0 100 L 100 100 L 100 0 L 0 0 Z"/>
                </s3dx:section>
              </s3dx:extrude>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let json = to_json(&result).unwrap();
        assert!(json.contains("\"extrude\""));
        assert!(json.contains("\"section\""));
        assert!(json.contains("\"shapes\""));
    }

    #[test]
    fn materials_library_and_fill_styles() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <s3dx:materials>
            <s3dx:material id="paint" color="0.2 0.4 0.8 1" metalness="0" roughness="0.4"/>
          </s3dx:materials>
          <defs>
            <g id="g">
              <s3dx:box length="1" width="1" height="1"
                        lengthVec="1 0 0" widthVec="0 1 0"
                        fill="0.5 0.5 0.5 1" material="url(#paint)"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#g"/>
          </s3dx:model>
        </svg>"##;
        let result = parse_and_evaluate(svg).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert_eq!(result.materials.len(), 1);
        assert_eq!(result.materials[0].id, "paint");
        assert_eq!(result.materials[0].color, [0.2, 0.4, 0.8, 1.0]);
        let prim = flatten(&result.instructions[0])
            .find(|i| matches!(i, GeometryInstruction::Primitive { .. }))
            .expect("primitive");
        let styles = prim.styles();
        assert!(styles.contains_key("fill"), "{styles:?}");
        assert_eq!(
            styles.get("--material").and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            }),
            Some("url(#paint)")
        );
    }

    #[test]
    fn svg_metadata_and_foreign_ns_preserved_but_not_evaluated() {
        use crate::parser::{Namespace, Node};

        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1"
             xmlns:acme="urn:acme:equipment:1.0">
          <metadata>
            <acme:profile version="1.0"/>
          </metadata>
          <defs>
            <g id="vessel">
              <g id="shell">
                <metadata>
                  <acme:equipment tag="V-101"/>
                </metadata>
                <s3dx:cylinder diameter="1200"
                               bottomCenterPt="0 0 0" topCenterPt="0 0 2000"/>
              </g>
              <g id="nozzle-n1"
                 s3dx:transform="matrix(0 0 -1 0 1 0 1 0 0 600 0 1000)">
                <metadata>
                  <acme:nozzle tag="N1" nominalSize="DN100"/>
                </metadata>
                <s3dx:cylinder diameter="100"
                               bottomCenterPt="0 0 0" topCenterPt="0 0 200"/>
              </g>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#vessel"/>
          </s3dx:model>
        </svg>"##;

        let doc = parse_xml(svg).unwrap();
        fn walk<'a>(el: &'a crate::parser::Element, out: &mut Vec<&'a crate::parser::Element>) {
            out.push(el);
            for child in &el.children {
                if let Node::Element(e) = child {
                    walk(e, out);
                }
            }
        }
        let mut all = Vec::new();
        walk(&doc.root, &mut all);
        let metadatas: Vec<_> = all
            .iter()
            .filter(|e| e.ns == Namespace::Svg && e.name == "metadata")
            .collect();
        assert!(
            metadatas.len() >= 3,
            "expected document + shell + nozzle metadata, got {}",
            metadatas.len()
        );
        let foreign = all
            .iter()
            .filter(|e| e.ns == Namespace::Other)
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>();
        assert!(foreign.contains(&"profile"), "{foreign:?}");
        assert!(foreign.contains(&"equipment"), "{foreign:?}");
        assert!(foreign.contains(&"nozzle"), "{foreign:?}");

        let result = evaluate_document(&doc, EvalOptions::default()).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let prims: Vec<_> = result
            .instructions
            .iter()
            .flat_map(flatten)
            .filter(|i| matches!(i, GeometryInstruction::Primitive { .. }))
            .collect();
        assert_eq!(prims.len(), 2, "shell + nozzle cylinders only");
        for p in &prims {
            if let GeometryInstruction::Primitive { kind, .. } = p {
                assert_eq!(*kind, PrimitiveKind::Cylinder);
            }
        }
    }

    fn flatten<'a>(
        inst: &'a GeometryInstruction,
    ) -> Box<dyn Iterator<Item = &'a GeometryInstruction> + 'a> {
        match inst {
            GeometryInstruction::Group { children, .. }
            | GeometryInstruction::BoolOp { operands: children, .. } => {
                Box::new(std::iter::once(inst).chain(children.iter().flat_map(|c| flatten(c))))
            }
            _ => Box::new(std::iter::once(inst)),
        }
    }
}
