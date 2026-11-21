use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::diff::{all_added, diff_instructions, NodeChange};
use crate::error::{Error, Result};
use crate::eval::{evaluate_document, EvalOptions};
use crate::instruction::EvaluationResult;
use crate::params::{filter_external_params, is_direct_param_value, Value};
use crate::parser::{parse_xml, Document};

/// Declared custom property discovered in the document (class rules / inline).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParamDesc {
    pub name: String,
    pub default: Option<Value>,
    /// Class names / `:root` that declare this property (if any).
    pub sources: Vec<String>,
    /// True only for direct `:root` literals — the only keys hosts may pass externally.
    #[serde(default)]
    pub external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UpdateResult {
    pub revision: u64,
    pub result: EvaluationResult,
    pub changes: Vec<NodeChange>,
    pub changed_params: Vec<String>,
    /// Current external parameter snapshot (normalized keys without `--`).
    pub params: IndexMap<String, Value>,
}

/// Parsed document session: mutate external params / model and re-evaluate with diffs.
pub struct DocumentSession {
    source: String,
    doc: Document,
    external: HashMap<String, Value>,
    model: Option<String>,
    retain_inactive: bool,
    last: Option<EvaluationResult>,
    revision: u64,
}

impl DocumentSession {
    /// Parse and evaluate. First update lists every node as Added.
    pub fn open(input: &str) -> Result<(Self, UpdateResult)> {
        let doc = parse_xml(input)?;
        let mut session = Self {
            source: input.to_string(),
            doc,
            external: HashMap::new(),
            model: None,
            retain_inactive: false,
            last: None,
            revision: 0,
        };
        let update = session.evaluate_internal(Vec::new(), true)?;
        Ok((session, update))
    }

    pub fn open_with(
        input: &str,
        params: HashMap<String, Value>,
        model: Option<String>,
    ) -> Result<(Self, UpdateResult)> {
        let doc = parse_xml(input)?;
        let mut session = Self {
            source: input.to_string(),
            doc,
            external: normalize_param_map(params),
            model,
            retain_inactive: false,
            last: None,
            revision: 0,
        };
        let update = session.evaluate_internal(Vec::new(), true)?;
        Ok((session, update))
    }

    /// When true, skipped if/if-not branches stay in the result as `inactive` groups.
    pub fn set_retain_inactive(&mut self, retain: bool) -> Result<UpdateResult> {
        self.retain_inactive = retain;
        self.evaluate_internal(Vec::new(), true)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn result(&self) -> Option<&EvaluationResult> {
        self.last.as_ref()
    }

    pub fn params(&self) -> &HashMap<String, Value> {
        &self.external
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Replace the entire external parameter map and re-evaluate.
    /// Keys outside the `:root` direct-literal whitelist are ignored (evaluation warnings).
    pub fn set_params(&mut self, params: HashMap<String, Value>) -> Result<UpdateResult> {
        let next = normalize_param_map(params);
        let changed = param_keys_changed(&self.external, &next);
        self.external = next;
        self.evaluate_internal(changed, false)
    }

    /// Merge external parameters (overwrite keys) and re-evaluate.
    /// Keys outside the `:root` direct-literal whitelist are ignored (evaluation warnings).
    pub fn patch_params(&mut self, patch: HashMap<String, Value>) -> Result<UpdateResult> {
        let patch = normalize_param_map(patch);
        let mut changed: Vec<String> = patch.keys().cloned().collect();
        changed.sort();
        for (k, v) in patch {
            self.external.insert(k, v);
        }
        self.evaluate_internal(changed, false)
    }

    pub fn clear_param(&mut self, name: &str) -> Result<UpdateResult> {
        let key = normalize_param_key(name);
        let changed = if self.external.remove(&key).is_some() {
            vec![key]
        } else {
            Vec::new()
        };
        self.evaluate_internal(changed, false)
    }

    pub fn set_model(&mut self, model: Option<String>) -> Result<UpdateResult> {
        self.model = model;
        self.evaluate_internal(Vec::new(), false)
    }

    /// Re-parse document text (studio edits). Changes are all Added vs empty prev.
    pub fn reload(&mut self, input: &str) -> Result<UpdateResult> {
        self.doc = parse_xml(input)?;
        self.source = input.to_string();
        let (kept, _) = filter_external_params(&self.doc, std::mem::take(&mut self.external));
        self.external = kept;
        self.last = None;
        self.evaluate_internal(Vec::new(), true)
    }

    /// Force re-evaluation with current params/model.
    pub fn evaluate(&mut self) -> Result<UpdateResult> {
        self.evaluate_internal(Vec::new(), false)
    }

    /// Custom properties declared in `:root` / class rules (and root inline style if present).
    /// `external == true` only for direct `:root` literals (the host-overridable set).
    pub fn declared_params(&self) -> Vec<ParamDesc> {
        let mut map: IndexMap<String, ParamDesc> = IndexMap::new();
        for (class, props) in &self.doc.class_rules {
            for (css_name, raw) in props {
                let Some(name) = css_name.strip_prefix("--") else {
                    continue;
                };
                let external = class == ":root" && is_direct_param_value(raw);
                let entry = map.entry(name.to_string()).or_insert_with(|| ParamDesc {
                    name: name.to_string(),
                    default: Some(Value::parse_literal(raw)),
                    sources: Vec::new(),
                    external: false,
                });
                if !entry.sources.iter().any(|s| s == class) {
                    entry.sources.push(class.clone());
                }
                if external {
                    entry.external = true;
                    entry.default = Some(Value::parse_literal(raw));
                }
            }
        }
        if let Some(style) = self.doc.root.attr("style") {
            for decl in style.split(';') {
                let decl = decl.trim();
                if let Some((name, value)) = decl.split_once(':') {
                    let name = name.trim();
                    if let Some(prop) = name.strip_prefix("--") {
                        let raw = value.trim();
                        map.entry(prop.to_string()).or_insert_with(|| ParamDesc {
                            name: prop.to_string(),
                            default: Some(Value::parse_literal(raw)),
                            sources: vec!["svg".into()],
                            external: false,
                        });
                    }
                }
            }
        }
        map.into_values().collect()
    }

    fn evaluate_internal(
        &mut self,
        changed_params: Vec<String>,
        force_all_added: bool,
    ) -> Result<UpdateResult> {
        let result = evaluate_document(
            &self.doc,
            EvalOptions {
                external: self.external.clone(),
                model: self.model.clone(),
                retain_inactive: self.retain_inactive,
            },
        )?;
        // Keep only whitelist keys in the session snapshot (rejected keys already warned).
        let (kept, _) = filter_external_params(&self.doc, std::mem::take(&mut self.external));
        self.external = kept;

        let changes = if force_all_added || self.last.is_none() {
            all_added(&result.instructions)
        } else {
            diff_instructions(
                &self.last.as_ref().unwrap().instructions,
                &result.instructions,
            )
        };

        self.revision = self.revision.saturating_add(1);
        self.last = Some(result.clone());

        let mut params = IndexMap::new();
        let mut keys: Vec<_> = self.external.keys().cloned().collect();
        keys.sort();
        for k in keys {
            if let Some(v) = self.external.get(&k) {
                params.insert(k, v.clone());
            }
        }

        Ok(UpdateResult {
            revision: self.revision,
            result,
            changes,
            changed_params,
            params,
        })
    }
}

fn normalize_param_key(name: &str) -> String {
    name.trim()
        .trim_start_matches('-')
        .trim_start_matches('-')
        .to_string()
}

fn normalize_param_map(params: HashMap<String, Value>) -> HashMap<String, Value> {
    params
        .into_iter()
        .map(|(k, v)| (normalize_param_key(&k), v))
        .collect()
}

fn param_keys_changed(
    prev: &HashMap<String, Value>,
    next: &HashMap<String, Value>,
) -> Vec<String> {
    let mut keys: Vec<String> = prev.keys().chain(next.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| prev.get(k) != next.get(k))
        .collect()
}

/// Serialize an [`UpdateResult`] to JSON.
pub fn update_to_json(update: &UpdateResult) -> Result<String> {
    serde_json::to_string_pretty(update).map_err(|e| Error::Internal(e.to_string()))
}
