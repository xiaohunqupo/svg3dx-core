use super::types::Value;
use crate::parser::Document;
use std::collections::{HashMap, HashSet};

/// Whether a CSS custom-property declaration is a direct literal
/// (no `var()` / `calc()`), eligible as an externalizable `:root` param.
pub fn is_direct_param_value(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    !lower.contains("var(") && !lower.contains("calc(")
}

/// Bare names of `:root` custom properties that accept external overrides,
/// mapped to their declared default literals.
pub fn externalizable_root_params(doc: &Document) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    let Some(props) = doc.class_rules.get(":root") else {
        return out;
    };
    for (css_name, raw) in props {
        let Some(name) = css_name.strip_prefix("--") else {
            continue;
        };
        if is_direct_param_value(raw) {
            out.insert(name.to_string(), Value::parse_literal(raw));
        }
    }
    out
}

pub fn externalizable_root_names(doc: &Document) -> HashSet<String> {
    externalizable_root_params(doc).into_keys().collect()
}

/// Keep only externalizable keys; return rejected bare names (sorted).
pub fn filter_external_params(
    doc: &Document,
    params: HashMap<String, Value>,
) -> (HashMap<String, Value>, Vec<String>) {
    let allowed = externalizable_root_names(doc);
    let mut kept = HashMap::new();
    let mut rejected = Vec::new();
    for (k, v) in params {
        let key = bare_key(&k);
        if allowed.contains(&key) {
            kept.insert(key, v);
        } else {
            rejected.push(key);
        }
    }
    rejected.sort();
    rejected.dedup();
    (kept, rejected)
}

fn bare_key(name: &str) -> String {
    name.trim()
        .trim_start_matches('-')
        .trim_start_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_xml;

    #[test]
    fn only_root_literals_are_externalizable() {
        let doc = parse_xml(
            r##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
                 xmlns:s3dx="urn:s3dx:draft:0.1">
               <style>
                 :root {
                   --H: 100;
                   --D: calc(var(--H) - 20);
                   --Label: hello;
                 }
                 .tanks { --R: 50; }
               </style>
             </svg>"##,
        )
        .unwrap();
        let names = externalizable_root_names(&doc);
        assert!(names.contains("H"));
        assert!(names.contains("Label"));
        assert!(!names.contains("D"));
        assert!(!names.contains("R"));
    }
}
