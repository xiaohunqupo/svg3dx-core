use std::path::{Path, PathBuf};

use svg3dx_core::{parse_and_evaluate, BoolOpKind, GeometryInstruction, PrimitiveKind};

fn flatten<'a>(
    inst: &'a GeometryInstruction,
) -> Box<dyn Iterator<Item = &'a GeometryInstruction> + 'a> {
    match inst {
        GeometryInstruction::Group { children, .. }
        | GeometryInstruction::BoolOp {
            operands: children, ..
        } => Box::new(std::iter::once(inst).chain(children.iter().flat_map(|c| flatten(c)))),
        _ => Box::new(std::iter::once(inst)),
    }
}

/// Official format repo root (`svg3dx`), not vendored into this library.
fn svg3dx_root() -> PathBuf {
    if let Ok(p) = std::env::var("SVG3DX_ROOT") {
        return PathBuf::from(p);
    }
    // workspace: <parent>/svg3dx-core/crates/svg3dx-core → <parent>/svg3dx
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../svg3dx")
}

fn sample_path() -> PathBuf {
    svg3dx_root().join("samples/draft-0.1-example.svg3dx")
}

#[test]
fn evaluate_draft_0_1_example() {
    let path = sample_path();
    if !path.is_file() {
        eprintln!(
            "skip: official sample not found at {} (set SVG3DX_ROOT to the svg3dx format repo)",
            path.display()
        );
        return;
    }

    let text = std::fs::read_to_string(&path).expect("sample file");
    let result = parse_and_evaluate(&text).expect("evaluate");

    assert_eq!(result.active_model.as_deref(), Some("solid-view"));
    assert_eq!(result.models.len(), 2);

    let all: Vec<_> = result.instructions.iter().flat_map(|i| flatten(i)).collect();

    assert!(
        result.warnings.is_empty(),
        "unexpected warnings: {:?}",
        result.warnings
    );

    let boxes = all
        .iter()
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
    assert!(boxes >= 4, "expected several boxes, got {boxes}");

    let has_subtract = all.iter().any(|i| {
        matches!(
            i,
            GeometryInstruction::BoolOp {
                op: BoolOpKind::Subtract,
                ..
            }
        )
    });
    assert!(has_subtract, "expected bool-subtract in tanks group");

    let has_extrude = all.iter().any(|i| {
        matches!(
            i,
            GeometryInstruction::Primitive {
                kind: PrimitiveKind::Extrude,
                ..
            }
        )
    });
    assert!(has_extrude, "expected extrude in group-3");

    let json = svg3dx_core::to_json(&result).unwrap();
    assert!(json.contains("solid-view"));
    assert!(json.contains("s3dx") || json.contains("\"m\"") || json.contains("length"));
}
