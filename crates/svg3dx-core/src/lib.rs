//! svg3dx-core — kernel-agnostic parser & evaluator for the svg3dx format.
//!
//! Pipeline: XML → DOM → parameter scopes → loop/condition expansion →
//! geometry instruction sequence. No meshing, no rendering, no CAD kernel.

pub mod diff;
pub mod error;
pub mod eval;
pub mod geometry;
pub mod instruction;
pub mod params;
pub mod parser;
pub mod session;

pub use diff::{ChangeFields, NodeChange, NodeChangeKind};
pub use error::{Error, Result, Warning};
pub use eval::{evaluate_document, EvalOptions};
pub use instruction::{
    BoolOpKind, EvaluationResult, GeometryInstruction, ModelDesc, ModelUse, PrimitiveKind,
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
        assert!(!json.contains("\"length\": 9") && !json.contains("\"length\":9"));
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
    fn extrude_with_profile() {
        let svg = r##"
        <svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs>
            <g id="g">
              <s3dx:extrude length="60" dir="0 0 1">
                <s3dx:profile type="polygon"
                              points="0 100 0  100 100 0  100 0 0  0 0 0"/>
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
        assert!(json.contains("\"profile\""));
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
