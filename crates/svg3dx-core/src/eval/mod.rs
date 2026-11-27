mod condition;
mod loop_eval;
mod path;

pub use path::PathCtx;

use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};
use crate::geometry::primitives::{filter_params, is_meta_attr, known_attrs};
use crate::geometry::Mat4x3;
use crate::instruction::{
    BoolOpKind, EvaluationResult, GeometryInstruction, MaterialDesc, ModelDesc, ModelUse,
    PrimitiveKind,
};
use crate::params::{
    resolve_attr, resolve_attr_no_ref, resolve_attr_with, ParamScope, Value,
};
use crate::parser::{Document, Element, Namespace, Node as DomNode, S3DX_NS_0_1};

pub struct EvalOptions {
    pub external: HashMap<String, Value>,
    /// Override active model by id (external `model=...`).
    pub model: Option<String>,
    /// Keep skipped `if` / `if-not` groups marked `inactive` (default false).
    /// Live evaluation omits them; hosts that preview false branches opt in.
    pub retain_inactive: bool,
    /// Extra named defs `<g>` ids to evaluate/emit beyond the active model's
    /// reachable set (on-demand preview). Empty by default.
    pub extra_defs: Vec<String>,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            external: HashMap::new(),
            model: None,
            retain_inactive: false,
            extra_defs: Vec::new(),
        }
    }
}

pub fn evaluate_document(doc: &Document, opts: EvalOptions) -> Result<EvaluationResult> {
    if !doc.svg3d {
        return Err(Error::Svg3dDisabled);
    }

    let mut warnings = Vec::new();
    if doc.s3dx_ns == S3DX_NS_0_1 && document_uses_point_features(&doc.root) {
        warnings.push(
            "document uses urn:s3dx:draft:0.1 with s3dx:point or ref(#id); prefer urn:s3dx:draft:0.2"
                .into(),
        );
    }
    let mut scope = ParamScope::new();
    let (external, rejected) = crate::params::filter_external_params(doc, opts.external);
    for key in rejected {
        warnings.push(format!(
            "external param '{key}' ignored: only direct :root literals may be passed externally"
        ));
    }
    scope.set_external(external);

    let mut class_map = HashMap::new();
    for (class, props) in &doc.class_rules {
        let mut parsed = IndexMap::new();
        for (k, v) in props {
            parsed.insert(k.clone(), Value::parse_literal(v));
        }
        class_map.insert(class.clone(), parsed);
    }
    scope.set_class_rules(class_map);

    let models = collect_models(doc, &mut scope, &mut warnings, opts.model.as_deref());
    // Live eval: only groups referenced by the active model, plus any
    // host-requested `extra_defs` (on-demand preview of unused definitions).
    let reachable = reachable_group_ids(models.iter().filter(|m| m.active));
    let extra: HashSet<String> = opts.extra_defs.iter().cloned().collect();

    let mut group_cache: HashMap<String, Vec<GeometryInstruction>> = HashMap::new();
    let mut visiting = HashSet::new();

    push_root_scope(&mut scope, &doc.root);

    for el in doc.root.child_elements() {
        if el.is_svg("defs") {
            push_element_scope(&mut scope, el);
            for child in el.child_elements() {
                if child.is_svg("g") {
                    if let Some(id) = child.id() {
                        if reachable.is_empty()
                            || reachable.contains(id)
                            || extra.contains(id)
                        {
                            let mut path = PathCtx::named(id);
                            let inst = eval_group(
                                child,
                                doc,
                                &mut scope,
                                Mat4x3::identity(),
                                Mat4x3::identity(),
                                &mut path,
                                &mut warnings,
                                &mut group_cache,
                                &mut visiting,
                            );
                            group_cache.insert(id.to_string(), inst);
                        }
                    }
                }
            }
            scope.pop_node();
        }
    }

    let mut instructions = Vec::new();
    let mut emitted = HashSet::new();
    // Emit unique groups used by the active model (library). Host applies each
    // `<s3dx:use>` transform when composing the view.
    for model in models.iter().filter(|m| m.active) {
        for u in &model.uses {
            if emitted.insert(u.group_id.clone()) {
                if let Some(geom) = group_cache.get(&u.group_id) {
                    let styles = group_styles(doc, &u.group_id, &scope);
                    let local_xf =
                        named_group_local_xf(doc, &u.group_id, &mut scope, &mut warnings);
                    instructions.push(GeometryInstruction::Group {
                        node_id: u.group_id.clone(),
                        id: Some(u.group_id.clone()),
                        styles,
                        children: geom.clone(),
                        transform: local_xf,
                        inactive: false,
                    });
                } else if let Some(el) = doc.find_by_id(&u.group_id) {
                    let mut path = PathCtx::named(&u.group_id);
                    let geom = eval_group(
                        el,
                        doc,
                        &mut scope,
                        Mat4x3::identity(),
                                Mat4x3::identity(),
                        &mut path,
                        &mut warnings,
                        &mut group_cache,
                        &mut visiting,
                    );
                    group_cache.insert(u.group_id.clone(), geom.clone());
                    let styles = {
                        push_element_scope(&mut scope, el);
                        let s = scope.resolved_styles();
                        scope.pop_node();
                        s
                    };
                    let local_xf =
                        named_group_local_xf(doc, &u.group_id, &mut scope, &mut warnings);
                    instructions.push(GeometryInstruction::Group {
                        node_id: u.group_id.clone(),
                        id: Some(u.group_id.clone()),
                        styles,
                        children: geom,
                        transform: local_xf,
                        inactive: false,
                    });
                } else {
                    warnings.push(format!("model references missing group '{}'", u.group_id));
                }
            }
        }
    }

    let mut extras: Vec<String> = extra.into_iter().filter(|id| !emitted.contains(id)).collect();
    extras.sort();
    for id in extras {
        let Some(geom) = group_cache.get(&id) else {
            if doc.find_by_id(&id).is_none() {
                warnings.push(format!("extra defs group '{id}' not found"));
            }
            continue;
        };
        let styles = group_styles(doc, &id, &scope);
        let local_xf = named_group_local_xf(doc, &id, &mut scope, &mut warnings);
        emitted.insert(id.clone());
        instructions.push(GeometryInstruction::Group {
            node_id: id.clone(),
            id: Some(id),
            styles,
            children: geom.clone(),
            transform: local_xf,
            inactive: false,
        });
    }

    if models.is_empty() {
        let mut root_path = PathCtx::root();
        for el in doc.root.child_elements() {
            if el.is_svg("g") {
                let (node_id, mut path) = if let Some(id) = el.id() {
                    (id.to_string(), PathCtx::named(id))
                } else {
                    let id = root_path.next("g");
                    (id.clone(), PathCtx::named(&id))
                };
                let geom = eval_group(
                    el,
                    doc,
                    &mut scope,
                    Mat4x3::identity(),
                                Mat4x3::identity(),
                    &mut path,
                    &mut warnings,
                    &mut group_cache,
                    &mut visiting,
                );
                let (styles, local_xf) = {
                    push_element_scope(&mut scope, el);
                    let s = scope.resolved_styles();
                    let xf = parse_transform_attr(el, &scope, &mut warnings);
                    scope.pop_node();
                    (s, xf)
                };
                instructions.push(GeometryInstruction::Group {
                    node_id,
                    id: el.id().map(|s| s.to_string()),
                    styles,
                    children: geom,
                    transform: local_xf,
                    inactive: false,
                });
            }
        }
    }

    scope.pop_node();

    let active_model = models
        .iter()
        .find(|m| m.active)
        .map(|m| m.id.clone())
        .or_else(|| models.first().map(|m| m.id.clone()));

    let materials = collect_materials(doc, &mut warnings);
    let animations = crate::animation::collect_animations(doc, &mut warnings);

    if !opts.retain_inactive {
        for inst in &mut instructions {
            strip_inactive_groups(inst);
        }
    }

    Ok(EvaluationResult {
        instructions,
        models,
        materials,
        active_model,
        warnings,
        animations,
    })
}

/// Drop skipped condition wrappers from a live evaluation tree.
fn strip_inactive_groups(inst: &mut GeometryInstruction) {
    match inst {
        GeometryInstruction::Group { children, .. } => {
            children.retain(|c| !c.is_inactive());
            for child in children.iter_mut() {
                strip_inactive_groups(child);
            }
        }
        GeometryInstruction::BoolOp { operands, .. } => {
            for child in operands.iter_mut() {
                strip_inactive_groups(child);
            }
        }
        GeometryInstruction::Primitive { .. } | GeometryInstruction::Instance { .. } => {}
    }
}

fn collect_materials(doc: &Document, warnings: &mut Vec<String>) -> Vec<MaterialDesc> {
    let mut by_id: IndexMap<String, MaterialDesc> = IndexMap::new();
    for el in doc.root.child_elements() {
        if !el.is_s3dx("materials") {
            continue;
        }
        for child in el.child_elements() {
            if !child.is_s3dx("material") {
                warnings.push(format!(
                    "s3dx:materials ignores unexpected child <{}>",
                    child.name
                ));
                continue;
            }
            let Some(id) = child.id().map(|s| s.to_string()) else {
                warnings.push("s3dx:material missing id — skipped".into());
                continue;
            };
            if by_id.contains_key(&id) {
                warnings.push(format!(
                    "duplicate s3dx:material id '{id}' — later definition wins"
                ));
            }
            let color = parse_material_color(child.attr("color"));
            let metalness = parse_material_scalar(child.attr("metalness"), 0.0);
            let roughness = parse_material_scalar(child.attr("roughness"), 0.5);
            by_id.insert(
                id.clone(),
                MaterialDesc {
                    id,
                    color,
                    metalness,
                    roughness,
                },
            );
        }
    }
    by_id.into_values().collect()
}

fn parse_material_color(raw: Option<&str>) -> [f64; 4] {
    let Some(raw) = raw else {
        return [1.0, 1.0, 1.0, 1.0];
    };
    match Value::parse_literal(raw) {
        Value::Vector(v) if v.len() >= 4 => [v[0], v[1], v[2], v[3]],
        Value::Vector(v) if v.len() == 3 => [v[0], v[1], v[2], 1.0],
        Value::Number(n) => [n, n, n, 1.0],
        _ => [1.0, 1.0, 1.0, 1.0],
    }
}

fn parse_material_scalar(raw: Option<&str>, default: f64) -> f64 {
    let Some(raw) = raw else {
        return default;
    };
    match Value::parse_literal(raw) {
        Value::Number(n) => n,
        _ => default,
    }
}

fn reachable_group_ids<'a>(
    models: impl IntoIterator<Item = &'a ModelDesc>,
) -> HashSet<String> {
    let mut set = HashSet::new();
    for m in models {
        for u in &m.uses {
            set.insert(u.group_id.clone());
        }
    }
    set
}

fn collect_models(
    doc: &Document,
    scope: &mut ParamScope,
    warnings: &mut Vec<String>,
    external_model: Option<&str>,
) -> Vec<ModelDesc> {
    let mut models = Vec::new();
    for el in doc.root.child_elements() {
        if el.is_s3dx("model") {
            let id = match el.attr("id") {
                Some(id) => id.to_string(),
                None => {
                    warnings.push("s3dx:model missing id — skipped".into());
                    continue;
                }
            };
            let mode = el.attr("mode").unwrap_or("solid").to_string();
            let active_attr = el
                .attr("active")
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            let transform = parse_transform_attr(el, scope, warnings);
            let mut uses = Vec::new();
            for child in el.child_elements() {
                if child.is_s3dx("use") {
                    if let Some(href) = child.attr("href") {
                        let gid = href.trim_start_matches('#').trim();
                        // Empty href (href="" / href="#") is an unset reference —
                        // do not treat it as a reachable group id (would be "").
                        if gid.is_empty() {
                            continue;
                        }
                        let t = parse_transform_attr(child, scope, warnings);
                        uses.push(ModelUse {
                            group_id: gid.to_string(),
                            transform: t,
                        });
                    }
                }
            }
            models.push(ModelDesc {
                id,
                mode,
                active: active_attr,
                transform,
                uses,
            });
        }
    }

    if let Some(want) = external_model {
        for m in &mut models {
            m.active = m.id == want;
        }
    } else if !models.iter().any(|m| m.active) {
        if let Some(first) = models.first_mut() {
            first.active = true;
        }
    }
    models
}

fn push_root_scope(scope: &mut ParamScope, el: &Element) {
    // `:root { ... }` applies to the document root (same cascade level as root class rules;
    // root inline `style` still wins via lookup order: inline → classes → presentation).
    let mut classes = vec![":root".to_string()];
    classes.extend(el.classes());
    let inline = parse_inline_styles(el.attr("style").unwrap_or(""));
    let presentation = presentation_styles(el);
    scope.push_node(classes, inline, presentation);
}

fn push_element_scope(scope: &mut ParamScope, el: &Element) {
    let classes = el.classes();
    let inline = parse_inline_styles(el.attr("style").unwrap_or(""));
    let presentation = presentation_styles(el);
    scope.push_node(classes, inline, presentation);
}

/// Presentation attrs for appearance (`fill`, `opacity`, `material` → `--material`).
fn presentation_styles(el: &Element) -> IndexMap<String, Value> {
    let mut out = IndexMap::new();
    if let Some(fill) = el.attr("fill") {
        out.insert("fill".to_string(), Value::parse_literal(fill));
    }
    if let Some(opacity) = el.attr("opacity") {
        out.insert("opacity".to_string(), Value::parse_literal(opacity));
    }
    if let Some(material) = el.attr("material") {
        out.insert("--material".to_string(), Value::parse_literal(material));
    }
    out
}

/// Parse element `style="..."` declarations (custom props + presentation).
fn parse_inline_styles(style: &str) -> IndexMap<String, Value> {
    let mut out = IndexMap::new();
    for decl in style.split(';') {
        let decl = decl.trim();
        if let Some((name, value)) = decl.split_once(':') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            out.insert(name.to_string(), Value::parse_literal(value.trim()));
        }
    }
    out
}

fn parse_transform_attr(el: &Element, scope: &ParamScope, warnings: &mut Vec<String>) -> Mat4x3 {
    let Some(raw) = el.s3dx_attr("transform") else {
        return Mat4x3::identity();
    };
    match resolve_attr(raw, scope) {
        Ok(v) => match Mat4x3::from_value(&v) {
            Ok(m) => m,
            Err(w) => match Mat4x3::parse(&v.expand_for_attr()) {
                Ok(m) => m,
                Err(_) => {
                    warnings.push(w.message);
                    Mat4x3::identity()
                }
            },
        },
        Err(w) => match expand_then_parse_matrix(raw, scope) {
            Ok(m) => m,
            Err(msg) => {
                warnings.push(format!("s3dx:transform: {} ({msg})", w.message));
                Mat4x3::identity()
            }
        },
    }
}

fn expand_then_parse_matrix(raw: &str, scope: &ParamScope) -> std::result::Result<Mat4x3, String> {
    use crate::params::expr::resolve_attr as ra;
    match ra(raw, scope) {
        Ok(v) => Mat4x3::parse(&v.expand_for_attr()).map_err(|w| w.message),
        Err(w) => Err(w.message),
    }
}

/// Local `s3dx:transform` of a named `<g id>` (for library wrappers).
fn named_group_local_xf(
    doc: &Document,
    group_id: &str,
    scope: &mut ParamScope,
    warnings: &mut Vec<String>,
) -> Mat4x3 {
    let Some(el) = doc.find_by_id(group_id) else {
        return Mat4x3::identity();
    };
    push_element_scope(scope, el);
    let xf = parse_transform_attr(el, scope, warnings);
    scope.pop_node();
    xf
}

#[allow(clippy::too_many_arguments)]
fn eval_group(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    _parent_xf: Mat4x3,
    world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    push_element_scope(scope, el);

    // Align with `point_world_position`: named groups include their own
    // `s3dx:transform` (and ancestors) so `ref(#id)` inverts in the same frame.
    // Instruction transforms still stay on the wrapper Group (gizmo-friendly).
    let world_xf = if let Some(id) = el.id() {
        element_world_xf(doc, id, scope).unwrap_or(world_xf)
    } else {
        world_xf
    };

    // Children stay in this group's local frame. The library / nested wrapper
    // Group carries `s3dx:transform` so the editor gizmo can edit it without
    // snap-back (baking into children left the wrapper at identity).
    let mut out = Vec::new();
    for child in el.child_elements() {
        out.extend(eval_node(
            child,
            doc,
            scope,
            Mat4x3::identity(),
            world_xf,
            path,
            warnings,
            group_cache,
            visiting,
        ));
    }

    scope.pop_node();
    out
}

#[allow(clippy::too_many_arguments)]
fn eval_node(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
    world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    match el.ns {
        Namespace::Svg if el.name == "g" => {
            // Nested <g>: emit a Group so `s3dx:transform` stays on the wrapper
            // (same as if / named library groups) for editor gizmos.
            push_element_scope(scope, el);
            let local_xf = parse_transform_attr(el, scope, warnings);
            let child_world = local_xf.then(world_xf);
            let (node_id, mut child_path) = if let Some(id) = el.id() {
                (id.to_string(), path.child_prefix(id))
            } else {
                let id = path.next("g");
                (id.clone(), path.under(&id))
            };
            let mut children = Vec::new();
            for child in el.child_elements() {
                children.extend(eval_node(
                    child,
                    doc,
                    scope,
                    Mat4x3::identity(),
                    child_world,
                    &mut child_path,
                    warnings,
                    group_cache,
                    visiting,
                ));
            }
            let styles = scope.resolved_styles();
            scope.pop_node();
            vec![GeometryInstruction::Group {
                node_id,
                id: el.id().map(|s| s.to_string()),
                styles,
                children,
                transform: local_xf.then(parent_xf),
                inactive: false,
            }]
        }
        Namespace::S3dx => {
            eval_s3dx(el, doc, scope, parent_xf, world_xf, path, warnings, group_cache, visiting)
        }
        _ => Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_s3dx(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
    world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    if matches!(el.name.as_str(), "if" | "if-not") {
        return eval_condition(el, doc, scope, parent_xf, world_xf, path, warnings, group_cache, visiting);
    }
    if el.name.starts_with("loop-") {
        return eval_loop(el, doc, scope, parent_xf, world_xf, path, warnings, group_cache, visiting);
    }
    if let Some(op) = BoolOpKind::from_local_name(&el.name) {
        return eval_bool(op, el, doc, scope, parent_xf, world_xf, path, warnings, group_cache, visiting);
    }
    if el.name == "use" {
        return eval_use(el, doc, scope, parent_xf, world_xf, path, warnings, group_cache, visiting);
    }
    if matches!(
        el.name.as_str(),
        "model" | "section" | "materials" | "material" | "animate" | "animateTransform"
    ) {
        return Vec::new();
    }
    eval_primitive(el, doc, scope, parent_xf, world_xf, path, warnings)
}

fn document_uses_point_features(el: &Element) -> bool {
    if el.is_s3dx("point") {
        return true;
    }
    for (_, v) in &el.attrs {
        if v.contains("ref(") {
            return true;
        }
    }
    el.child_elements().any(document_uses_point_features)
}


/// World transform of the element with `id` (ancestors · self), same frame as point_world.
fn element_world_xf(
    doc: &Document,
    id: &str,
    base_scope: &ParamScope,
) -> std::result::Result<Mat4x3, crate::error::Warning> {
    use crate::error::Warning;
    let path = doc
        .id_index
        .get(id)
        .ok_or_else(|| Warning::new(format!("id '{id}' not found")))?;

    let mut scope = base_scope.clone_params_only();
    push_root_scope(&mut scope, &doc.root);

    let mut world_xf = Mat4x3::identity();
    let mut cur = &doc.root;

    for &idx in path.iter() {
        let child = cur
            .children
            .get(idx)
            .ok_or_else(|| Warning::new(format!("broken id path for '{id}'")))?;
        let DomNode::Element(el) = child else {
            return Err(Warning::new(format!("broken id path for '{id}'")));
        };
        push_element_scope(&mut scope, el);
        let local_xf = match el.s3dx_attr("transform") {
            Some(raw) => match resolve_attr_no_ref(raw, &scope) {
                Ok(v) => Mat4x3::from_value(&v).unwrap_or_else(|_| Mat4x3::identity()),
                Err(_) => Mat4x3::identity(),
            },
            None => Mat4x3::identity(),
        };
        world_xf = local_xf.then(world_xf);
        cur = el;
    }
    Ok(world_xf)
}

/// World position of `<s3dx:point id>` = ancestors·self·at.
fn point_world_position(
    doc: &Document,
    id: &str,
    base_scope: &ParamScope,
) -> std::result::Result<[f64; 3], crate::error::Warning> {
    use crate::error::Warning;
    let path = doc
        .id_index
        .get(id)
        .ok_or_else(|| Warning::new(format!("ref(#{id}): id not found")))?;

    let mut scope = base_scope.clone_params_only();
    push_root_scope(&mut scope, &doc.root);

    let mut world_xf = Mat4x3::identity();
    let mut cur = &doc.root;

    for (depth, &idx) in path.iter().enumerate() {
        let child = cur
            .children
            .get(idx)
            .ok_or_else(|| Warning::new(format!("ref(#{id}): broken id path")))?;
        let DomNode::Element(el) = child else {
            return Err(Warning::new(format!("ref(#{id}): broken id path")));
        };
        push_element_scope(&mut scope, el);
        let local_xf = match el.s3dx_attr("transform") {
            Some(raw) => match resolve_attr_no_ref(raw, &scope) {
                Ok(v) => Mat4x3::from_value(&v).unwrap_or_else(|_| Mat4x3::identity()),
                Err(_) => Mat4x3::identity(),
            },
            None => Mat4x3::identity(),
        };
        world_xf = local_xf.then(world_xf);
        if depth + 1 == path.len() {
            if !el.is_s3dx("point") {
                return Err(Warning::new(format!(
                    "ref(#{id}): target is not s3dx:point"
                )));
            }
            let at = match el.attr("at") {
                Some(raw) => resolve_attr_no_ref(raw, &scope)?,
                None => Value::Vector(vec![0.0, 0.0, 0.0]),
            };
            let p = match at.as_vector() {
                Some(v) if v.len() >= 3 => [v[0], v[1], v[2]],
                _ => {
                    return Err(Warning::new(format!(
                        "s3dx:point #{id} @at must be a 3D point"
                    )));
                }
            };
            return Ok(world_xf.transform_point(p));
        }
        cur = el;
    }
    Err(Warning::new(format!("ref(#{id}): empty id path")))
}

#[allow(clippy::too_many_arguments)]
fn eval_condition(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
    world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    push_element_scope(scope, el);

    let include = condition::should_include_branch(el, scope, warnings);
    let local_xf = parse_transform_attr(el, scope, warnings);
    // Always consume a path slot so sibling if:N indices match markup order,
    // even when the branch is skipped.
    let node_id = path.next(el.name.as_str());
    let mut child_path = path.under(&node_id);

    // Keep `s3dx:transform` on the wrapper Group (editor gizmo target). Children
    // are evaluated in the if-local frame so the Three.js hierarchy matches
    // world = parent * local_if * child — otherwise the gizmo snaps back after
    // commit while only the baked children move.
    //
    // `local_xf` only: parent transform lives on ancestor Groups (named / nested
    // g). `then(parent_xf)` remains for flat parents (e.g. loop iterations).
    let child_world = local_xf.then(world_xf);
    let mut children = Vec::new();
    for child in el.child_elements() {
        children.extend(eval_node(
            child,
            doc,
            scope,
            Mat4x3::identity(),
            child_world,
            &mut child_path,
            warnings,
            group_cache,
            visiting,
        ));
    }
    let styles = scope.resolved_styles();
    scope.pop_node();

    // Wrap so each if occupies one child index under the parent group (stable
    // treePath). Inactive wrappers are hidden in the live view and ghosted when
    // the editor selects that branch.
    vec![GeometryInstruction::Group {
        node_id,
        id: None,
        styles,
        children,
        transform: local_xf.then(parent_xf),
        inactive: !include,
    }]
}

fn inject_loop_vars(scope: &mut ParamScope, index: usize, value: Value) {
    scope.set_local("loop-index", Value::Number(index as f64));
    if let Some(v) = value.as_vector() {
        if v.len() >= 3 {
            scope.set_local("loop-value-x", Value::Number(v[0]));
            scope.set_local("loop-value-y", Value::Number(v[1]));
            scope.set_local("loop-value-z", Value::Number(v[2]));
        }
    }
    scope.set_local("loop-value", value);
}

#[allow(clippy::too_many_arguments)]
fn eval_loop(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
    world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    let Some(iter) = loop_eval::prepare_loop(el, scope, warnings) else {
        return Vec::new();
    };
    let local_xf = parse_transform_attr(el, scope, warnings);
    let xf = local_xf.then(parent_xf);
    let loop_kind = el.name.as_str(); // loop-range / loop-points

    let mut out = Vec::new();
    match iter {
        loop_eval::LoopIter::Points(points) => {
            for (index, value) in points.into_iter().enumerate() {
                scope.push_frame();
                inject_loop_vars(scope, index, value);
                let mut iter_path = path.child_prefix(&format!("{loop_kind}:{index}"));
                let child_world = local_xf.then(world_xf);
                for child in el.child_elements() {
                    out.extend(eval_node(
                        child,
                        doc,
                        scope,
                        xf,
                        child_world,
                        &mut iter_path,
                        warnings,
                        group_cache,
                        visiting,
                    ));
                }
                scope.pop_frame();
            }
        }
        loop_eval::LoopIter::Range { values } => {
            for (index, value) in values.into_iter().enumerate() {
                scope.push_frame();
                inject_loop_vars(scope, index, Value::Number(value));
                let mut iter_path = path.child_prefix(&format!("{loop_kind}:{index}"));
                let child_world = local_xf.then(world_xf);
                for child in el.child_elements() {
                    out.extend(eval_node(
                        child,
                        doc,
                        scope,
                        xf,
                        child_world,
                        &mut iter_path,
                        warnings,
                        group_cache,
                        visiting,
                    ));
                }
                scope.pop_frame();
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn eval_bool(
    op: BoolOpKind,
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
    world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    let local_xf = parse_transform_attr(el, scope, warnings);
    let node_id = path.next(op.as_str());
    let mut child_path = path.under(&node_id);

    push_element_scope(scope, el);

    // Keep `s3dx:transform` on the BoolOp wrapper (editor gizmo target).
    // Operands stay in the bool-local frame — baking into children left the
    // CSG mesh at identity while the attr still held the pose (fly-away on drag).
    let child_world = local_xf.then(world_xf);
    let mut operands = Vec::new();
    for child in el.child_elements() {
        operands.extend(eval_node(
            child,
            doc,
            scope,
            Mat4x3::identity(),
            child_world,
            &mut child_path,
            warnings,
            group_cache,
            visiting,
        ));
    }
    // Skipped if/if-not branches are preview-only — never boolean operands.
    operands.retain(|o| !o.is_inactive());
    let before = operands.len();
    operands.retain(|o| {
        !matches!(
            o,
            GeometryInstruction::Primitive {
                kind: PrimitiveKind::Point,
                ..
            }
        )
    });
    if operands.len() != before {
        warnings.push(format!(
            "s3dx:{} skipped s3dx:point operand(s) (positioning points are not boolean solids)",
            op.as_str()
        ));
    }
    let styles = scope.resolved_styles();
    scope.pop_node();

    if operands.is_empty() {
        return Vec::new();
    }
    vec![GeometryInstruction::BoolOp {
        node_id,
        op,
        styles,
        operands,
        transform: local_xf.then(parent_xf),
    }]
}

#[allow(clippy::too_many_arguments)]
fn eval_use(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
    _world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    let Some(href) = el.attr("href") else {
        warnings.push("s3dx:use missing href".into());
        return Vec::new();
    };
    let gid = href.trim_start_matches('#');
    let use_xf = parse_transform_attr(el, scope, warnings);
    let use_id = path.next("use");

    if !visiting.insert(gid.to_string()) {
        warnings.push(format!("cyclic s3dx:use reference to '{gid}'"));
        return Vec::new();
    }

    push_element_scope(scope, el);
    let use_styles = scope.resolved_styles();
    // Cache stores group-local children; fold the target group's own
    // `s3dx:transform` into this use wrapper (model view applies it on the
    // library Group separately via `named_group_local_xf` at emit time).
    let named_xf = named_group_local_xf(doc, gid, scope, warnings);
    let xf = named_xf.then(use_xf).then(parent_xf);

    let result = if let Some(cached) = group_cache.get(gid) {
        let mut children = cached.clone();
        for c in &mut children {
            c.prefix_node_ids(&use_id);
        }
        vec![GeometryInstruction::Group {
            node_id: use_id,
            id: Some(gid.to_string()),
            styles: use_styles,
            children,
            transform: xf,
            inactive: false,
        }]
    } else if let Some(target) = doc.find_by_id(gid) {
        if !group_has_3d(target) {
            warnings.push(format!(
                "s3dx:use '#{gid}' references a group without 3D content — skipped"
            ));
            scope.pop_node();
            visiting.remove(gid);
            return Vec::new();
        }
        let mut target_path = PathCtx::named(gid);
        let geom = eval_group(
            target,
            doc,
            scope,
            Mat4x3::identity(),
            Mat4x3::identity(),
            &mut target_path,
            warnings,
            group_cache,
            visiting,
        );
        group_cache.insert(gid.to_string(), geom.clone());
        let mut children = geom;
        for c in &mut children {
            c.prefix_node_ids(&use_id);
        }
        vec![GeometryInstruction::Group {
            node_id: use_id,
            id: Some(gid.to_string()),
            styles: use_styles,
            children,
            transform: xf,
            inactive: false,
        }]
    } else {
        warnings.push(format!("s3dx:use references unknown id '{gid}'"));
        Vec::new()
    };
    scope.pop_node();

    visiting.remove(gid);
    result
}

fn group_has_3d(el: &Element) -> bool {
    for child in el.child_elements() {
        if child.ns == Namespace::S3dx {
            // Animation-only children do not count as geometry content.
            if matches!(child.name.as_str(), "animate" | "animateTransform") {
                continue;
            }
            return true;
        }
        if child.is_svg("g") && group_has_3d(child) {
            return true;
        }
    }
    false
}

fn eval_primitive(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
    world_xf: Mat4x3,
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
) -> Vec<GeometryInstruction> {
    let kind = PrimitiveKind::from_local_name(&el.name);
    if kind == PrimitiveKind::Other {
        warnings.push(format!("unknown s3dx:{} — skipped", el.name));
        return Vec::new();
    }

    let node_id = path.next(kind.as_str());

    push_element_scope(scope, el);

    let local_xf = parse_transform_attr(el, scope, warnings);
    let xf = local_xf.then(parent_xf);
    // Full consumer world for live `ref(#id)` (nested groups keep instruction
    // transforms on wrappers, so `parent_xf` is often identity here).
    let consumer_world = local_xf.then(world_xf);
    let styles = scope.resolved_styles();

    let allow_ref = kind != PrimitiveKind::Point;
    let ref_base_scope = scope.clone_params_only();
    let point_ref = |id: &str| -> std::result::Result<Value, crate::error::Warning> {
        let world = point_world_position(doc, id, &ref_base_scope)?;
        let inv = consumer_world.try_inverse().ok_or_else(|| {
            crate::error::Warning::new(format!(
                "ref(#{id}): consumer transform not invertible"
            ))
        })?;
        let local = inv.transform_point(world);
        Ok(Value::Vector(local.to_vec()))
    };

    // Keep meta attrs that this kind lists (e.g. gltf/@href); filter_params
    // applies the same rule as a second pass for construct-child merges.
    let known = known_attrs(kind);
    let mut params = IndexMap::new();
    let mut attr_failed = false;
    for (name, raw) in &el.attrs {
        let listed = known.iter().any(|n| *n == name.as_str());
        if is_meta_attr(name) && !listed {
            continue;
        }
        let resolved = if allow_ref && raw.contains("ref(") {
            resolve_attr_with(raw, scope, Some(&point_ref))
        } else if kind == PrimitiveKind::Point && name == "at" {
            resolve_attr_no_ref(raw, scope)
        } else {
            resolve_attr(raw, scope)
        };
        match resolved {
            Ok(v) => {
                params.insert(name.clone(), v);
            }
            Err(w) => {
                warnings.push(format!("s3dx:{} @{name}: {}", el.name, w.message));
                if raw.contains("ref(") {
                    attr_failed = true;
                }
            }
        }
    }

    if attr_failed {
        scope.pop_node();
        return Vec::new();
    }

    collect_construct_children(kind, el, scope, &mut params, warnings, Some(&point_ref));

    // Default `at` for positioning points.
    if kind == PrimitiveKind::Point && !params.contains_key("at") {
        params.insert("at".into(), Value::Vector(vec![0.0, 0.0, 0.0]));
    }

    let params = filter_params(kind, params, warnings);
    scope.pop_node();

    vec![GeometryInstruction::Primitive {
        node_id,
        kind,
        params,
        styles,
        transform: xf,
    }]
}

fn group_styles(doc: &Document, group_id: &str, scope: &ParamScope) -> IndexMap<String, Value> {
    let Some(el) = doc.find_by_id(group_id) else {
        return IndexMap::new();
    };
    // Approximate: merge class rules for the group's classes + inline style onto a temp scope.
    let mut tmp = scope.clone();
    push_element_scope(&mut tmp, el);
    let styles = tmp.resolved_styles();
    tmp.pop_node();
    styles
}

fn eval_child_attrs(
    child: &Element,
    scope: &ParamScope,
    warnings: &mut Vec<String>,
    point_ref: Option<&dyn Fn(&str) -> std::result::Result<Value, crate::error::Warning>>,
) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for (name, raw) in &child.attrs {
        if is_meta_attr(name) {
            continue;
        }
        let resolved = if raw.contains("ref(") {
            resolve_attr_with(raw, scope, point_ref)
        } else {
            resolve_attr(raw, scope)
        };
        match resolved {
            Ok(v) => {
                map.insert(name.clone(), v);
            }
            Err(w) => {
                warnings.push(format!("{} @{name}: {}", child.name, w.message));
            }
        }
    }
    map
}

/// Evaluate `<s3dx:section>`: pose attrs + optional `s3dx:transform` + 2D SVG shapes.
fn eval_section(
    el: &Element,
    scope: &mut ParamScope,
    warnings: &mut Vec<String>,
) -> IndexMap<String, Value> {
    let mut map = eval_child_attrs(el, scope, warnings, None);
    let xf = parse_transform_attr(el, scope, warnings);
    if xf != Mat4x3::identity() {
        let mut xf_map = IndexMap::new();
        xf_map.insert("m".into(), Value::Vector(xf.m.to_vec()));
        map.insert("transform".into(), Value::Map(xf_map));
    }
    let mut shapes = Vec::new();
    for child in el.child_elements() {
        let ok = child.ns == Namespace::Svg
            && matches!(
                child.name.as_str(),
                "path" | "circle" | "ellipse" | "polygon" | "polyline" | "rect"
            );
        if !ok {
            warnings.push(format!(
                "s3dx:section ignores unexpected <{}> (2D shapes only)",
                if child.ns == Namespace::S3dx {
                    format!("s3dx:{}", child.name)
                } else {
                    child.name.clone()
                }
            ));
            continue;
        }
        let mut shape = eval_child_attrs(child, scope, warnings, None);
        shape.insert("tag".into(), Value::String(child.name.clone()));
        shapes.push(Value::Map(shape));
    }
    if shapes.is_empty() {
        warnings.push(
            "s3dx:section requires at least one SVG shape child (path/circle/polygon/rect/…)"
                .into(),
        );
    }
    map.insert("shapes".into(), Value::List(shapes));
    map
}

/// Evaluate `<s3dx:path>`: `points` and/or `line`/`arc` segment children.
fn eval_path(
    el: &Element,
    scope: &ParamScope,
    warnings: &mut Vec<String>,
    point_ref: Option<&dyn Fn(&str) -> std::result::Result<Value, crate::error::Warning>>,
) -> IndexMap<String, Value> {
    let mut map = eval_child_attrs(el, scope, warnings, point_ref);
    let mut segments = Vec::new();
    for child in el.child_elements() {
        if child.is_s3dx("line") || child.is_s3dx("arc") {
            let mut seg = eval_child_attrs(child, scope, warnings, point_ref);
            seg.insert("tag".into(), Value::String(child.name.clone()));
            segments.push(Value::Map(seg));
        } else {
            warnings.push(format!(
                "s3dx:path ignores unexpected <{}>",
                if child.ns == Namespace::S3dx {
                    format!("s3dx:{}", child.name)
                } else {
                    child.name.clone()
                }
            ));
        }
    }
    if !segments.is_empty() {
        if map.contains_key("points") {
            warnings.push(
                "s3dx:path has both points and line/arc children — using children".into(),
            );
            map.shift_remove("points");
        }
        map.insert("segments".into(), Value::List(segments));
    } else if !map.contains_key("points") {
        warnings.push(
            "s3dx:path requires points attribute or <s3dx:line>/<s3dx:arc> children".into(),
        );
    }
    map
}

fn collect_construct_children(
    kind: PrimitiveKind,
    el: &Element,
    scope: &mut ParamScope,
    params: &mut IndexMap<String, Value>,
    warnings: &mut Vec<String>,
    point_ref: Option<&dyn Fn(&str) -> std::result::Result<Value, crate::error::Warning>>,
) {
    let mut sections = Vec::new();
    let mut path_from_child = None;

    for child in el.child_elements() {
        if child.is_s3dx("section") {
            sections.push(Value::Map(eval_section(child, scope, warnings)));
        } else if child.is_s3dx("path") {
            path_from_child = Some(Value::Map(eval_path(child, scope, warnings, point_ref)));
        }
    }

    if kind == PrimitiveKind::Path {
        // Standalone path: fold segment children into params.
        let evaluated = eval_path(el, scope, warnings, point_ref);
        if evaluated.contains_key("segments") {
            params.shift_remove("points");
        }
        for (k, v) in evaluated {
            params.insert(k, v);
        }
        return;
    }

    match kind {
        PrimitiveKind::Extrude | PrimitiveKind::Revolve => {
            if sections.len() == 1 {
                params.insert("section".into(), sections.remove(0));
            } else if sections.is_empty() {
                warnings.push(format!(
                    "s3dx:{} requires exactly one <s3dx:section>",
                    kind.as_str()
                ));
            } else {
                warnings.push(format!(
                    "s3dx:{} expects exactly one <s3dx:section>, got {}",
                    kind.as_str(),
                    sections.len()
                ));
            }
        }
        PrimitiveKind::Loft => {
            if sections.len() >= 2 {
                params.insert("sections".into(), Value::List(sections));
            } else {
                warnings.push(format!(
                    "s3dx:loft requires at least two <s3dx:section>, got {}",
                    sections.len()
                ));
            }
        }
        PrimitiveKind::Sweep => {
            if sections.len() == 1 {
                params.insert("section".into(), sections.remove(0));
            } else if sections.is_empty() {
                warnings.push("s3dx:sweep requires exactly one <s3dx:section>".into());
            } else {
                warnings.push(format!(
                    "s3dx:sweep expects exactly one <s3dx:section>, got {}",
                    sections.len()
                ));
            }
            if let Some(path) = path_from_child {
                if params.contains_key("path") {
                    warnings.push(
                        "s3dx:sweep ignores path attribute — use child <s3dx:path> only".into(),
                    );
                    params.shift_remove("path");
                }
                params.insert("path".into(), path);
            } else if params.contains_key("path") {
                warnings.push(
                    "s3dx:sweep path attribute is non-normative — prefer child <s3dx:path>".into(),
                );
            } else {
                warnings.push("s3dx:sweep requires a child <s3dx:path>".into());
            }
        }
        PrimitiveKind::Pipe => {
            if let Some(path) = path_from_child {
                if params.contains_key("path") {
                    warnings.push(
                        "s3dx:pipe has both path attribute and <s3dx:path> — using child".into(),
                    );
                }
                params.insert("path".into(), path);
            } else if !params.contains_key("path") {
                warnings.push("s3dx:pipe requires path attribute or <s3dx:path> child".into());
            }
        }
        _ => {
            if !sections.is_empty() || path_from_child.is_some() {
                warnings.push(format!(
                    "s3dx:{} ignores unexpected section/path children",
                    kind.as_str()
                ));
            }
        }
    }
}
