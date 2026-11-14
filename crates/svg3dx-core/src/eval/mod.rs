mod condition;
mod exp;
mod loop_eval;
mod path;

pub use path::PathCtx;

use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};

use crate::error::{Error, Result};
use crate::geometry::primitives::{filter_params, is_meta_attr};
use crate::geometry::Mat4x3;
use crate::instruction::{
    BoolOpKind, EvaluationResult, GeometryInstruction, ModelDesc, ModelUse, PrimitiveKind,
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
                    instructions.push(GeometryInstruction::Group {
                        node_id: u.group_id.clone(),
                        id: Some(u.group_id.clone()),
                        styles,
                        children: geom.clone(),
                        transform: Mat4x3::identity(),
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
                    instructions.push(GeometryInstruction::Group {
                        node_id: u.group_id.clone(),
                        id: Some(u.group_id.clone()),
                        styles,
                        children: geom,
                        transform: Mat4x3::identity(),
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
                let styles = {
                    push_element_scope(&mut scope, el);
                    let s = scope.resolved_styles();
                    scope.pop_node();
                    s
                };
                instructions.push(GeometryInstruction::Group {
                    node_id,
                    id: el.id().map(|s| s.to_string()),
                    styles,
                    children: geom,
                    transform: Mat4x3::identity(),
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

    Ok(EvaluationResult {
        instructions,
        models,
        active_model,
        warnings,
    })
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
                        let gid = href.trim_start_matches('#').to_string();
                        let t = parse_transform_attr(child, scope, warnings);
                        uses.push(ModelUse {
                            group_id: gid,
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
    // root inline `style` still wins via lookup order: inline → classes).
    let mut classes = vec![":root".to_string()];
    classes.extend(el.classes());
    let inline = parse_inline_styles(el.attr("style").unwrap_or(""));
    scope.push_node(classes, inline);
}

fn push_element_scope(scope: &mut ParamScope, el: &Element) {
    let classes = el.classes();
    let inline = parse_inline_styles(el.attr("style").unwrap_or(""));
    scope.push_node(classes, inline);
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

#[allow(clippy::too_many_arguments)]
fn eval_group(
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

    let local_xf = parse_transform_attr(el, scope, warnings);
    let xf = local_xf.then(parent_xf);

    let mut out = Vec::new();
    for child in el.child_elements() {
        out.extend(eval_node(
            child,
            doc,
            scope,
            xf,
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
            // Nested <g>: grouping / param scope / transform; children flatten into parent list.
            // Path uses id or g:n so descendants get stable node_ids.
            let mut child_path = if let Some(id) = el.id() {
                path.child_prefix(id)
            } else {
                let id = path.next("g");
                path.under(&id)
            };
            eval_group(
                el,
                doc,
                scope,
                parent_xf,
                &mut child_path,
                warnings,
                group_cache,
                visiting,
            )
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
    if matches!(el.name.as_str(), "exp" | "model" | "profile" | "path") {
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

    if !condition::should_include_branch(el, scope, warnings) {
        scope.pop_node();
        return Vec::new();
    }

    let local_xf = parse_transform_attr(el, scope, warnings);
    let xf = local_xf.then(parent_xf);
    let mut child_path = {
        let id = path.next(el.name.as_str());
        path.under(&id)
    };

    let mut out = Vec::new();
    for child in el.child_elements() {
        out.extend(eval_node(
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

    scope.pop_node();
    out
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
    let xf = use_xf.then(parent_xf);
    let use_id = path.next("use");

    if !visiting.insert(gid.to_string()) {
        warnings.push(format!("cyclic s3dx:use reference to '{gid}'"));
        return Vec::new();
    }

    push_element_scope(scope, el);
    let use_styles = scope.resolved_styles();

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

    let mut params = IndexMap::new();
    for (name, raw) in &el.attrs {
        if is_meta_attr(name) {
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
                warnings.push(format!("s3dx:{} @{name}: {}", child.name, w.message));
            }
        }
    }
    map
}

fn collect_construct_children(
    kind: PrimitiveKind,
    el: &Element,
    scope: &ParamScope,
    params: &mut IndexMap<String, Value>,
    warnings: &mut Vec<String>,
) {
    let mut profiles = Vec::new();
    let mut path_from_child = None;

    for child in el.child_elements() {
        if child.is_s3dx("profile") {
            profiles.push(Value::Map(eval_child_attrs(child, scope, warnings)));
        } else if child.is_s3dx("path") {
            path_from_child = Some(Value::Map(eval_child_attrs(child, scope, warnings)));
        }
    }

    match kind {
        PrimitiveKind::Extrude | PrimitiveKind::Revolve => {
            if profiles.len() == 1 {
                params.insert("profile".into(), profiles.remove(0));
            } else if profiles.is_empty() {
                warnings.push(format!(
                    "s3dx:{} requires exactly one <s3dx:profile>",
                    kind.as_str()
                ));
            } else {
                warnings.push(format!(
                    "s3dx:{} expects exactly one <s3dx:profile>, got {}",
                    kind.as_str(),
                    profiles.len()
                ));
            }
        }
        PrimitiveKind::Loft => {
            if profiles.len() >= 2 {
                params.insert("profiles".into(), Value::List(profiles));
            } else {
                warnings.push(format!(
                    "s3dx:loft requires at least two <s3dx:profile>, got {}",
                    profiles.len()
                ));
            }
        }
        PrimitiveKind::Sweep => {
            if profiles.len() == 1 {
                params.insert("profile".into(), profiles.remove(0));
            } else if profiles.is_empty() {
                warnings.push("s3dx:sweep requires exactly one <s3dx:profile>".into());
            } else {
                warnings.push(format!(
                    "s3dx:sweep expects exactly one <s3dx:profile>, got {}",
                    profiles.len()
                ));
            }
            if let Some(path) = path_from_child {
                if params.contains_key("path") {
                    warnings.push(
                        "s3dx:sweep has both path attribute and <s3dx:path> — using child".into(),
                    );
                }
                params.insert("path".into(), path);
            } else if !params.contains_key("path") {
                warnings.push("s3dx:sweep requires path attribute or <s3dx:path> child".into());
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
            if !profiles.is_empty() || path_from_child.is_some() {
                warnings.push(format!(
                    "s3dx:{} ignores unexpected profile/path children",
                    kind.as_str()
                ));
            }
        }
    }
}
