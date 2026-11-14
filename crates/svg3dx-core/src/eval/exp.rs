use indexmap::IndexMap;

use crate::error::Warning;
use crate::params::{resolve_attr, ParamScope, Value};
use crate::parser::Element;

/// Apply child `<s3dx:exp>` overrides onto a new local frame.
/// Returns attribute name → evaluated value.
pub fn apply_exp_overrides(
    primitive: &Element,
    scope: &mut ParamScope,
    warnings: &mut Vec<String>,
) -> IndexMap<String, Value> {
    let mut overrides = IndexMap::new();
    for child in primitive.child_elements() {
        if child.is_s3dx("exp") {
            for (name, raw) in &child.attrs {
                match resolve_attr(raw, scope) {
                    Ok(v) => {
                        overrides.insert(name.clone(), v);
                    }
                    Err(Warning { message, .. }) => {
                        warnings.push(format!("s3dx:exp {name}: {message}"));
                    }
                }
            }
        }
    }
    for (k, v) in &overrides {
        scope.set_local(k.clone(), v.clone());
    }
    overrides
}
