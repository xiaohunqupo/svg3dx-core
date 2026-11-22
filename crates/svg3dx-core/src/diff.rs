use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::instruction::GeometryInstruction;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeChangeKind {
    Added,
    Removed,
    Updated,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeFields {
    pub params: bool,
    pub transform: bool,
    pub styles: bool,
    pub structure: bool,
}

impl ChangeFields {
    pub fn any(self) -> bool {
        self.params || self.transform || self.styles || self.structure
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeChange {
    pub node_id: String,
    pub kind: NodeChangeKind,
    /// Present for Added / Updated; None for Removed.
    pub instruction: Option<GeometryInstruction>,
    pub fields: ChangeFields,
}

/// Diff two instruction forests by stable `node_id`.
pub fn diff_instructions(
    prev: &[GeometryInstruction],
    next: &[GeometryInstruction],
) -> Vec<NodeChange> {
    let prev_map = index_by_id(prev);
    let next_map = index_by_id(next);
    let mut changes = Vec::new();

    for (id, old) in &prev_map {
        match next_map.get(id) {
            None => changes.push(NodeChange {
                node_id: (*id).to_string(),
                kind: NodeChangeKind::Removed,
                instruction: None,
                fields: ChangeFields::default(),
            }),
            Some(new) => {
                if let Some(fields) = compare_nodes(old, new) {
                    changes.push(NodeChange {
                        node_id: (*id).to_string(),
                        kind: NodeChangeKind::Updated,
                        instruction: Some((*new).clone()),
                        fields,
                    });
                }
            }
        }
    }

    for (id, new) in &next_map {
        if !prev_map.contains_key(id) {
            changes.push(NodeChange {
                node_id: (*id).to_string(),
                kind: NodeChangeKind::Added,
                instruction: Some((*new).clone()),
                fields: ChangeFields {
                    params: true,
                    transform: true,
                    styles: true,
                    structure: true,
                },
            });
        }
    }

    changes
}

/// Treat every node in `next` as Added (first frame / reload).
pub fn all_added(next: &[GeometryInstruction]) -> Vec<NodeChange> {
    let map = index_by_id(next);
    map.into_iter()
        .map(|(id, inst)| NodeChange {
            node_id: id.to_string(),
            kind: NodeChangeKind::Added,
            instruction: Some(inst.clone()),
            fields: ChangeFields {
                params: true,
                transform: true,
                styles: true,
                structure: true,
            },
        })
        .collect()
}

fn index_by_id(roots: &[GeometryInstruction]) -> HashMap<&str, &GeometryInstruction> {
    let mut map = HashMap::new();
    for root in roots {
        for node in root.walk() {
            map.insert(node.node_id(), node);
        }
    }
    map
}

fn compare_nodes(old: &GeometryInstruction, new: &GeometryInstruction) -> Option<ChangeFields> {
    use GeometryInstruction::*;
    let mut fields = ChangeFields::default();

    if old.styles() != new.styles() {
        fields.styles = true;
    }

    match (old, new) {
        (
            Primitive {
                kind: k0,
                params: p0,
                transform: t0,
                ..
            },
            Primitive {
                kind: k1,
                params: p1,
                transform: t1,
                ..
            },
        ) => {
            if k0 != k1 {
                fields.structure = true;
            }
            if p0 != p1 {
                fields.params = true;
            }
            if t0 != t1 {
                fields.transform = true;
            }
        }
        (
            BoolOp {
                op: o0,
                operands: a0,
                transform: t0,
                ..
            },
            BoolOp {
                op: o1,
                operands: a1,
                transform: t1,
                ..
            },
        ) => {
            if o0 != o1 {
                fields.structure = true;
            }
            // Child identity changes are reported on the children themselves;
            // mark structure if operand id sets differ.
            let ids0: Vec<_> = a0.iter().map(|c| c.node_id()).collect();
            let ids1: Vec<_> = a1.iter().map(|c| c.node_id()).collect();
            if ids0 != ids1 {
                fields.structure = true;
            }
            if t0 != t1 {
                fields.transform = true;
            }
        }
        (
            Instance {
                group_id: g0,
                transform: t0,
                ..
            },
            Instance {
                group_id: g1,
                transform: t1,
                ..
            },
        ) => {
            if g0 != g1 {
                fields.structure = true;
            }
            if t0 != t1 {
                fields.transform = true;
            }
        }
        (
            Group {
                id: i0,
                children: c0,
                transform: t0,
                ..
            },
            Group {
                id: i1,
                children: c1,
                transform: t1,
                ..
            },
        ) => {
            if i0 != i1 {
                fields.structure = true;
            }
            if t0 != t1 {
                fields.transform = true;
            }
            let ids0: Vec<_> = c0.iter().map(|c| c.node_id()).collect();
            let ids1: Vec<_> = c1.iter().map(|c| c.node_id()).collect();
            if ids0 != ids1 {
                fields.structure = true;
            }
        }
        _ => {
            fields.structure = true;
            fields.params = true;
            fields.transform = true;
            fields.styles = true;
        }
    }

    if fields.any() {
        Some(fields)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Mat4x3;
    use crate::instruction::PrimitiveKind;
    use indexmap::IndexMap;

    fn box_at(id: &str, tx: f64) -> GeometryInstruction {
        let mut m = Mat4x3::identity();
        m.m[9] = tx;
        GeometryInstruction::Primitive {
            node_id: id.into(),
            kind: PrimitiveKind::Box,
            params: IndexMap::new(),
            styles: IndexMap::new(),
            transform: m,
        }
    }

    #[test]
    fn detects_added_removed_updated() {
        let prev = vec![box_at("a/box:0", 0.0), box_at("a/box:1", 10.0)];
        let next = vec![box_at("a/box:0", 5.0), box_at("a/box:2", 0.0)];
        let changes = diff_instructions(&prev, &next);
        assert!(changes.iter().any(|c| {
            c.node_id == "a/box:0" && c.kind == NodeChangeKind::Updated && c.fields.transform
        }));
        assert!(changes
            .iter()
            .any(|c| c.node_id == "a/box:1" && c.kind == NodeChangeKind::Removed));
        assert!(changes
            .iter()
            .any(|c| c.node_id == "a/box:2" && c.kind == NodeChangeKind::Added));
    }
}
