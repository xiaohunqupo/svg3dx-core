use crate::error::Warning;
use crate::params::{evaluate, ParamScope};
use crate::parser::Element;

/// Evaluate `test` on `s3dx:if` / `s3dx:if-not`. Errors → false + warning.
/// Returns whether the element's children should participate.
pub fn should_include_branch(el: &Element, scope: &ParamScope, warnings: &mut Vec<String>) -> bool {
    let Some(cond) = el.attr("test") else {
        warnings.push(format!(
            "s3dx:{} missing required attribute test — skipping",
            el.name
        ));
        return false;
    };
    let ok = eval_bool(cond, scope, warnings);
    match el.name.as_str() {
        "if" => ok,
        "if-not" => !ok,
        other => {
            warnings.push(format!("unknown condition element s3dx:{other} — skipped"));
            false
        }
    }
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
