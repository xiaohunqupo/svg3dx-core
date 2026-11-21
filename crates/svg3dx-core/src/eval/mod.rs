mod condition;
mod exp;
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
use crate::params::{resolve_attr, ParamScope, Value};
use crate::parser::{Document, Element, Namespace};

pub struct EvalOptions {
    pub external: HashMap<String, Value>,
    /// Override active model by id (external `model=...`).
    pub model: Option<String>,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            external: HashMap::new(),
            model: None,
        }
    }
}

pub fn evaluate_document(doc: &Document, opts: EvalOptions) -> Result<EvaluationResult> {
    if !doc.svg3d {
        return Err(Error::Svg3dDisabled);
    }

    let mut warnings = Vec::new();
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
    // Only groups referenced by the active model(s) are evaluated / emitted.
    let reachable = reachable_group_ids(models.iter().filter(|m| m.active));

    let mut group_cache: HashMap<String, Vec<GeometryInstruction>> = HashMap::new();
    let mut visiting = HashSet::new();

    push_root_scope(&mut scope, &doc.root);

    for el in doc.root.child_elements() {
        if el.is_svg("defs") {
            push_element_scope(&mut scope, el);
            for child in el.child_elements() {
                if child.is_svg("g") {
                    if let Some(id) = child.id() {
                        if reachable.is_empty() || reachable.contains(id) {
                            let mut path = PathCtx::named(id);
                            let inst = eval_group(
                                child,
                                doc,
                                &mut scope,
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

    Ok(EvaluationResult {
        instructions,
        models,
        materials,
        active_model,
        warnings,
    })
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
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    push_element_scope(scope, el);

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
            eval_s3dx(el, doc, scope, parent_xf, path, warnings, group_cache, visiting)
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
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    if matches!(el.name.as_str(), "if" | "if-not") {
        return eval_condition(el, doc, scope, parent_xf, path, warnings, group_cache, visiting);
    }
    if el.name.starts_with("loop-") {
        return eval_loop(el, doc, scope, parent_xf, path, warnings, group_cache, visiting);
    }
    if let Some(op) = BoolOpKind::from_local_name(&el.name) {
        return eval_bool(op, el, doc, scope, parent_xf, path, warnings, group_cache, visiting);
    }
    if el.name == "use" {
        return eval_use(el, doc, scope, parent_xf, path, warnings, group_cache, visiting);
    }
    if matches!(
        el.name.as_str(),
        "exp" | "model" | "section" | "materials" | "material"
    ) {
        return Vec::new();
    }
    eval_primitive(el, scope, parent_xf, path, warnings)
}

#[allow(clippy::too_many_arguments)]
fn eval_condition(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
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
    let mut children = Vec::new();
    for child in el.child_elements() {
        children.extend(eval_node(
            child,
            doc,
            scope,
            Mat4x3::identity(),
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
                for child in el.child_elements() {
                    out.extend(eval_node(
                        child,
                        doc,
                        scope,
                        xf,
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
                for child in el.child_elements() {
                    out.extend(eval_node(
                        child,
                        doc,
                        scope,
                        xf,
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
    path: &mut PathCtx,
    warnings: &mut Vec<String>,
    group_cache: &mut HashMap<String, Vec<GeometryInstruction>>,
    visiting: &mut HashSet<String>,
) -> Vec<GeometryInstruction> {
    let local_xf = parse_transform_attr(el, scope, warnings);
    let xf = local_xf.then(parent_xf);
    let node_id = path.next(op.as_str());
    let mut child_path = path.under(&node_id);

    push_element_scope(scope, el);

    let mut operands = Vec::new();
    for child in el.child_elements() {
        operands.extend(eval_node(
            child,
            doc,
            scope,
            xf,
            &mut child_path,
            warnings,
            group_cache,
            visiting,
        ));
    }
    // Skipped if/if-not branches are preview-only — never boolean operands.
    operands.retain(|o| !o.is_inactive());
    let styles = scope.resolved_styles();
    scope.pop_node();

    if operands.is_empty() {
        return Vec::new();
    }
    if operands.len() == 1 {
        return operands;
    }
    vec![GeometryInstruction::BoolOp {
        node_id,
        op,
        styles,
        operands,
    }]
}

#[allow(clippy::too_many_arguments)]
fn eval_use(
    el: &Element,
    doc: &Document,
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
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
    scope: &mut ParamScope,
    parent_xf: Mat4x3,
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
    scope.push_frame();
    let overrides = exp::apply_exp_overrides(el, scope, warnings);

    let local_xf = parse_transform_attr(el, scope, warnings);
    let xf = local_xf.then(parent_xf);
    let styles = scope.resolved_styles();

    // Keep meta attrs that this kind lists (e.g. gltf/@href); filter_params
    // applies the same rule as a second pass for construct-child merges.
    let known = known_attrs(kind);
    let mut params = IndexMap::new();
    for (name, raw) in &el.attrs {
        let listed = known.iter().any(|n| *n == name.as_str());
        if is_meta_attr(name) && !listed {
            continue;
        }
        if let Some(v) = overrides.get(name) {
            params.insert(name.clone(), v.clone());
            continue;
        }
        match resolve_attr(raw, scope) {
            Ok(v) => {
                params.insert(name.clone(), v);
            }
            Err(w) => {
                warnings.push(format!("s3dx:{} @{name}: {}", el.name, w.message));
            }
        }
    }
    for (k, v) in overrides {
        params.entry(k).or_insert(v);
    }

    collect_construct_children(kind, el, scope, &mut params, warnings);

    let params = filter_params(kind, params, warnings);
    scope.pop_frame();
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
) -> IndexMap<String, Value> {
    let mut map = IndexMap::new();
    for (name, raw) in &child.attrs {
        if is_meta_attr(name) {
            continue;
        }
        match resolve_attr(raw, scope) {
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
    let mut map = eval_child_attrs(el, scope, warnings);
    let xf = parse_transform_attr(el, scope, warnings);
    if xf != Mat4x3::identity() {
        let mut xf_map = IndexMap::new();
        xf_map.insert("m".into(), Value::Vector(xf.m.to_vec()));
        map.insert("transform".into(), Value::Map(xf_map));
    }
    let mut shapes = Vec::new();
    for child in el.child_elements() {
        if child.is_s3dx("exp") {
            continue;
        }
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
        let mut shape = eval_child_attrs(child, scope, warnings);
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
) -> IndexMap<String, Value> {
    let mut map = eval_child_attrs(el, scope, warnings);
    let mut segments = Vec::new();
    for child in el.child_elements() {
        if child.is_s3dx("line") || child.is_s3dx("arc") {
            let mut seg = eval_child_attrs(child, scope, warnings);
            seg.insert("tag".into(), Value::String(child.name.clone()));
            segments.push(Value::Map(seg));
        } else if child.is_s3dx("exp") {
            continue;
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
) {
    let mut sections = Vec::new();
    let mut path_from_child = None;

    for child in el.child_elements() {
        if child.is_s3dx("section") {
            sections.push(Value::Map(eval_section(child, scope, warnings)));
        } else if child.is_s3dx("path") {
            path_from_child = Some(Value::Map(eval_path(child, scope, warnings)));
        }
    }

    if kind == PrimitiveKind::Path {
        // Standalone path: fold segment children into params.
        let evaluated = eval_path(el, scope, warnings);
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
