use super::types::Value;
use indexmap::IndexMap;
use std::collections::HashMap;

/// Layered parameter lookup: external → locals → ancestor chain → defaults.
#[derive(Debug, Clone, Default)]
pub struct ParamScope {
    /// Highest priority: host-injected external params (bare names, e.g. `D`).
    external: HashMap<String, Value>,
    /// Stack of local frames (innermost last). Used for loop vars.
    frames: Vec<IndexMap<String, Value>>,
    /// Class-rule style properties: class → (CSS name → value), e.g. `--D`, `fill`.
    class_rules: HashMap<String, IndexMap<String, Value>>,
    /// Ancestor / element inline styles, outer → inner.
    chain: Vec<ScopeNode>,
}

#[derive(Debug, Clone, Default)]
struct ScopeNode {
    classes: Vec<String>,
    /// Presentation attributes (`fill`, `--material` from `material=`, …); lowest on this node.
    presentation: IndexMap<String, Value>,
    /// Inline `style` declarations with CSS property names.
    inline: IndexMap<String, Value>,
}

impl ParamScope {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_external(&mut self, params: HashMap<String, Value>) {
        self.external = params
            .into_iter()
            .map(|(k, v)| (bare_param_key(&k), v))
            .collect();
    }

    pub fn set_class_rules(&mut self, rules: HashMap<String, IndexMap<String, Value>>) {
        self.class_rules = rules;
    }

    /// Clone external params + class rules with an empty element/frame stack
    /// (for resolving `ref(#id)` along a fresh ancestor walk).
    pub fn clone_params_only(&self) -> Self {
        Self {
            external: self.external.clone(),
            frames: Vec::new(),
            class_rules: self.class_rules.clone(),
            chain: Vec::new(),
        }
    }

    pub fn push_node(
        &mut self,
        classes: Vec<String>,
        inline: IndexMap<String, Value>,
        presentation: IndexMap<String, Value>,
    ) {
        self.chain.push(ScopeNode {
            classes,
            presentation,
            inline,
        });
    }

    pub fn pop_node(&mut self) {
        self.chain.pop();
    }

    pub fn push_frame(&mut self) {
        self.frames.push(IndexMap::new());
    }

    pub fn pop_frame(&mut self) {
        self.frames.pop();
    }

    pub fn set_local(&mut self, name: impl Into<String>, value: Value) {
        if self.frames.is_empty() {
            self.frames.push(IndexMap::new());
        }
        self.frames
            .last_mut()
            .unwrap()
            .insert(bare_param_key(&name.into()), value);
    }

    /// Lookup a parameter / custom property per spec §2.3 (`D` or `--D`).
    pub fn lookup(&self, name: &str) -> Option<Value> {
        let key = bare_param_key(name);

        if let Some(v) = self.external.get(&key) {
            return Some(v.clone());
        }

        for frame in self.frames.iter().rev() {
            if let Some(v) = frame.get(&key) {
                return Some(v.clone());
            }
        }

        let dashed = format!("--{key}");
        for node in self.chain.iter().rev() {
            if let Some(v) = node
                .inline
                .get(&dashed)
                .or_else(|| node.inline.get(&key))
            {
                return Some(v.clone());
            }
            for class in node.classes.iter().rev() {
                if let Some(rule) = self.class_rules.get(class) {
                    if let Some(v) = rule.get(&dashed).or_else(|| rule.get(&key)) {
                        return Some(v.clone());
                    }
                }
            }
            if let Some(v) = node
                .presentation
                .get(&dashed)
                .or_else(|| node.presentation.get(&key))
            {
                return Some(v.clone());
            }
        }

        None
    }

    /// Effective CSS styles at the current scope (custom props + presentation).
    /// Custom / injected params use dashed-ident keys (`--D`, `--loop-value`).
    /// Per node: presentation → class rules → inline (CSS beats presentation attrs).
    pub fn resolved_styles(&self) -> IndexMap<String, Value> {
        let mut out = IndexMap::new();

        // Cascade outer → inner so nearer declarations win.
        for node in &self.chain {
            for (k, v) in &node.presentation {
                out.insert(k.clone(), v.clone());
            }
            for class in &node.classes {
                if let Some(rule) = self.class_rules.get(class) {
                    for (k, v) in rule {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            for (k, v) in &node.inline {
                out.insert(k.clone(), v.clone());
            }
        }

        for frame in &self.frames {
            for (k, v) in frame {
                out.insert(format!("--{k}"), v.clone());
            }
        }

        for (k, v) in &self.external {
            out.insert(format!("--{k}"), v.clone());
        }

        out
    }
}

fn bare_param_key(name: &str) -> String {
    name.trim()
        .trim_start_matches('-')
        .trim_start_matches('-')
        .to_string()
}
