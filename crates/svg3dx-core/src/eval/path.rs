use std::collections::HashMap;

/// Deterministic node-id path builder for evaluation.
#[derive(Debug, Clone, Default)]
pub struct PathCtx {
    prefix: String,
    counters: HashMap<String, usize>,
}

impl PathCtx {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn named(name: &str) -> Self {
        Self {
            prefix: name.to_string(),
            counters: HashMap::new(),
        }
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Allocate `kind:n` under this context and return the full node_id.
    pub fn next(&mut self, kind: &str) -> String {
        let n = self.counters.entry(kind.to_string()).or_insert(0);
        let id = if self.prefix.is_empty() {
            format!("{kind}:{n}")
        } else {
            format!("{}/{kind}:{n}", self.prefix)
        };
        *n += 1;
        id
    }

    /// Child context whose prefix is an absolute path segment (e.g. group id or loop slot).
    pub fn child_prefix(&self, segment: &str) -> Self {
        let prefix = if self.prefix.is_empty() {
            segment.to_string()
        } else {
            format!("{}/{}", self.prefix, segment)
        };
        Self {
            prefix,
            counters: HashMap::new(),
        }
    }

    /// Child context rooted at an already-allocated node_id.
    pub fn under(&self, node_id: &str) -> Self {
        Self {
            prefix: node_id.to_string(),
            counters: HashMap::new(),
        }
    }
}
