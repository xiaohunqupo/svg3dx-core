use std::collections::HashMap;

use svg3dx_core::{
    DocumentSession, GeometryInstruction, NodeChangeKind, PrimitiveKind, Value,
};

fn sample_svg(loop_end: &str) -> String {
    format!(
        r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>
            :root {{ --D: 200; }}
            .tanks {{ --R: 50; }}
          </style>
          <defs>
            <g id="group-tanks" class="tanks">
              <s3dx:cylinder diameter="var(--D)" bottomCenterPt="0 0 0" topCenterPt="0 0 100"/>
            </g>
            <g id="group-pillars">
              <s3dx:loop-range start="0" end="{loop_end}" step="1">
                <s3dx:box length="20" width="20" height="200"
                          lengthVec="1 0 0" widthVec="0 1 0"
                          s3dx:transform="matrix(1 0 0 0 1 0 0 0 1 calc(var(--loop-value) * 50) 0 0)"/>
              </s3dx:loop-range>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true">
            <s3dx:use href="#group-tanks"/>
            <s3dx:use href="#group-pillars"/>
          </s3dx:model>
        </svg>"##
    )
}

fn flatten(inst: &GeometryInstruction) -> Vec<&GeometryInstruction> {
    inst.walk().collect()
}

fn find_primitive<'a>(
    result: &'a svg3dx_core::EvaluationResult,
    kind: PrimitiveKind,
) -> Option<&'a GeometryInstruction> {
    result
        .instructions
        .iter()
        .flat_map(flatten)
        .find(|i| matches!(i, GeometryInstruction::Primitive { kind: k, .. } if *k == kind))
}

#[test]
fn open_assigns_node_ids_and_revision() {
    let (session, update) = DocumentSession::open(&sample_svg("4")).unwrap();
    assert_eq!(update.revision, 1);
    assert_eq!(session.revision(), 1);
    assert!(!update.changes.is_empty());
    assert!(update
        .changes
        .iter()
        .all(|c| c.kind == NodeChangeKind::Added));
    let cyl = find_primitive(&update.result, PrimitiveKind::Cylinder).unwrap();
    assert!(cyl.node_id().contains("cylinder"));
}

#[test]
fn patch_diameter_marks_params_updated() {
    let (mut session, first) = DocumentSession::open(&sample_svg("4")).unwrap();
    let cyl_id = find_primitive(&first.result, PrimitiveKind::Cylinder)
        .unwrap()
        .node_id()
        .to_string();

    let mut patch = HashMap::new();
    patch.insert("D".into(), Value::Number(180.0));
    let update = session.patch_params(patch).unwrap();
    assert_eq!(update.revision, 2);
    assert!(update.changed_params.iter().any(|p| p == "D"));

    let change = update
        .changes
        .iter()
        .find(|c| c.node_id == cyl_id)
        .expect("cylinder should appear in changes");
    assert_eq!(change.kind, NodeChangeKind::Updated);
    assert!(change.fields.params);

    match find_primitive(&update.result, PrimitiveKind::Cylinder).unwrap() {
        GeometryInstruction::Primitive { params, .. } => {
            assert_eq!(params.get("diameter"), Some(&Value::Number(180.0)));
        }
        _ => unreachable!(),
    }
}

#[test]
fn loop_end_change_adds_and_removes_boxes() {
    let (mut session, first) = DocumentSession::open(&sample_svg("3")).unwrap();
    let boxes_before = first
        .result
        .instructions
        .iter()
        .flat_map(flatten)
        .filter(|i| {
            matches!(
                i,
                GeometryInstruction::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            )
        })
        .count();
    assert_eq!(boxes_before, 3);

    let update = session.reload(&sample_svg("5")).unwrap();
    assert!(update.revision >= 2);
    // reload treats as all Added
    assert!(update
        .changes
        .iter()
        .all(|c| c.kind == NodeChangeKind::Added));
    let boxes_after = update
        .result
        .instructions
        .iter()
        .flat_map(flatten)
        .filter(|i| {
            matches!(
                i,
                GeometryInstruction::Primitive {
                    kind: PrimitiveKind::Box,
                    ..
                }
            )
        })
        .count();
    assert_eq!(boxes_after, 5);
}

#[test]
fn loop_range_shrink_via_external_not_needed_but_transform_diff_works() {
    // Same document, patch a param that only affects transforms of loop boxes:
    // use two evaluates with identical doc — compare box transforms after loop values.
    let svg = r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>:root { --S: 50; }</style>
          <defs>
            <g id="pillars">
              <s3dx:loop-range start="0" end="2" step="1">
                <s3dx:box length="1" width="1" height="1"
                          lengthVec="1 0 0" widthVec="0 1 0"
                          s3dx:transform="matrix(1 0 0 0 1 0 0 0 1 calc(var(--loop-value) * var(--S)) 0 0)"/>
              </s3dx:loop-range>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true"><s3dx:use href="#pillars"/></s3dx:model>
        </svg>"##;
    let (mut session, first) = DocumentSession::open(svg).unwrap();
    let box_ids: Vec<_> = first
        .result
        .instructions
        .iter()
        .flat_map(flatten)
        .filter_map(|i| match i {
            GeometryInstruction::Primitive {
                kind: PrimitiveKind::Box,
                node_id,
                ..
            } => Some(node_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(box_ids.len(), 2);

    let mut patch = HashMap::new();
    patch.insert("S".into(), Value::Number(100.0));
    let update = session.patch_params(patch).unwrap();
    // loop-value=0 → tx stays 0 (no change); loop-value=1 → tx 50→100.
    let id1 = box_ids
        .iter()
        .find(|id| id.contains("loop-range:1"))
        .expect("loop-range:1 box");
    let c = update
        .changes
        .iter()
        .find(|c| c.node_id == *id1)
        .unwrap_or_else(|| panic!("missing change for {id1}"));
    assert_eq!(c.kind, NodeChangeKind::Updated);
    assert!(c.fields.transform, "{id1} fields={:?}", c.fields);
}

#[test]
fn declared_params_from_class_rules() {
    let (session, _) = DocumentSession::open(&sample_svg("2")).unwrap();
    let decls = session.declared_params();
    let d = decls.iter().find(|p| p.name == "D").expect("D");
    assert!(d.external, "D is a direct :root literal");
    let r = decls.iter().find(|p| p.name == "R").expect("R");
    assert!(!r.external, "R is class-only and not externalizable");
}

#[test]
fn external_rejects_non_root_and_derived() {
    let svg = r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>
            :root { --H: 100; --D: calc(var(--H) - 20); }
            .tanks { --R: 50; }
          </style>
          <defs>
            <g id="g" class="tanks">
              <s3dx:sphere radius="var(--R)" center="0 0 0"/>
              <s3dx:box length="1" width="1" height="var(--H)"
                        lengthVec="1 0 0" widthVec="0 1 0"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true"><s3dx:use href="#g"/></s3dx:model>
        </svg>"##;
    let (mut session, _) = DocumentSession::open(svg).unwrap();
    let mut patch = HashMap::new();
    patch.insert("H".into(), Value::Number(80.0));
    patch.insert("R".into(), Value::Number(9.0));
    patch.insert("D".into(), Value::Number(1.0));
    let update = session.patch_params(patch).unwrap();
    assert!(session.params().contains_key("H"));
    assert!(!session.params().contains_key("R"));
    assert!(!session.params().contains_key("D"));
    assert!(update
        .result
        .warnings
        .iter()
        .any(|w| w.contains("external param 'R'")));
    assert!(update
        .result
        .warnings
        .iter()
        .any(|w| w.contains("external param 'D'")));
    let json = svg3dx_core::to_json(&update.result).unwrap();
    assert!(json.contains("\"height\": 80") || json.contains("\"height\":80"));
    assert!(json.contains("\"radius\": 50") || json.contains("\"radius\":50"));
}

#[test]
fn declared_params_include_root_rule() {
    let svg = r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>:root { --H: 100; } .tanks { --D: 200; }</style>
          <defs>
            <g id="g" class="tanks">
              <s3dx:box length="1" width="1" height="var(--H)"
                        lengthVec="1 0 0" widthVec="0 1 0"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true"><s3dx:use href="#g"/></s3dx:model>
        </svg>"##;
    let (session, update) = DocumentSession::open(svg).unwrap();
    let decls = session.declared_params();
    let h = decls.iter().find(|p| p.name == "H").expect("H from :root");
    assert!(h.sources.iter().any(|s| s == ":root"));
    assert!(update.result.warnings.is_empty(), "{:?}", update.result.warnings);
}

#[test]
fn nodes_carry_resolved_styles() {
    let svg = r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>
            .tank { --D: 200; fill: #4a90d9; opacity: 0.85; }
          </style>
          <defs>
            <g id="g" class="tank" style="stroke: #123456">
              <s3dx:cylinder diameter="var(--D)" bottomCenterPt="0 0 0" topCenterPt="0 0 100"/>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true"><s3dx:use href="#g"/></s3dx:model>
        </svg>"##;
    let (_session, update) = DocumentSession::open(svg).unwrap();
    let cyl = find_primitive(&update.result, PrimitiveKind::Cylinder).unwrap();
    let styles = cyl.styles();
    assert_eq!(styles.get("--D"), Some(&Value::Number(200.0)));
    assert_eq!(
        styles.get("fill"),
        Some(&Value::String("#4a90d9".into()))
    );
    assert_eq!(styles.get("opacity"), Some(&Value::Number(0.85)));
    assert_eq!(
        styles.get("stroke"),
        Some(&Value::String("#123456".into()))
    );
}
