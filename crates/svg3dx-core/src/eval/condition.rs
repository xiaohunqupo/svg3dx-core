use crate::error::Warning;
use crate::params::{evaluate, ParamScope};
use crate::parser::Element;

/// Returns true if the `<g>` should participate. Errors → false + warning.
pub fn should_include(el: &Element, scope: &ParamScope, warnings: &mut Vec<String>) -> bool {
    let has_if = el.attr("if").is_some();
    let has_ifnot = el.attr("ifnot").is_some();
    if has_if && has_ifnot {
        warnings.push(
            "both if and ifnot on <g> — forbidden in draft 0.1; skipping group".into(),
        );
        return false;
    }
    if let Some(cond) = el.attr("if") {
        return eval_bool(cond, scope, warnings);
    }
    if let Some(cond) = el.attr("ifnot") {
        return !eval_bool(cond, scope, warnings);
    }
    true
}

fn eval_bool(expr: &str, scope: &ParamScope, warnings: &mut Vec<String>) -> bool {
    match evaluate(expr, scope) {
        Ok(v) => v.as_bool().unwrap_or_else(|| {
            warnings.push(format!("condition did not yield bool: {expr}"));
            false
        }),
        Err(Warning { message, .. }) => {
            warnings.push(format!("condition error ({message}) — treated as false"));
            false
        }
    }
}
