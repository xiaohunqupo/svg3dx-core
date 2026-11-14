//! wasm-bindgen exports for browser / three.js hosts.

use std::collections::HashMap;

use svg3dx_core::{
    parse_and_evaluate_with, to_json, update_to_json, DocumentSession, EvalOptions, UpdateResult,
    Value,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn parse_svg3dx(input: &str) -> Result<JsValue, JsValue> {
    let result = parse_and_evaluate_with(input, EvalOptions::default())
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[wasm_bindgen]
pub fn parse_svg3dx_json(input: &str) -> Result<String, JsValue> {
    let result = parse_and_evaluate_with(input, EvalOptions::default())
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    to_json(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// `params` is a JSON object of string → string/number/bool.
/// `model` optionally selects `<s3dx:model id="...">`.
#[wasm_bindgen]
pub fn parse_svg3dx_with_params(
    input: &str,
    params_json: &str,
    model: Option<String>,
) -> Result<JsValue, JsValue> {
    let external = parse_params_json(params_json)?;
    let result = parse_and_evaluate_with(input, EvalOptions { external, model })
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    serde_wasm_bindgen::to_value(&result).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Persistent document session for param edits + local update diffs.
#[wasm_bindgen]
pub struct Svg3dxSession {
    inner: DocumentSession,
    last_update: Option<UpdateResult>,
}

#[wasm_bindgen]
impl Svg3dxSession {
    #[wasm_bindgen(constructor)]
    pub fn new(input: &str) -> Result<Svg3dxSession, JsValue> {
        let (inner, update) =
            DocumentSession::open(input).map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self {
            inner,
            last_update: Some(update),
        })
    }

    #[wasm_bindgen(js_name = openWith)]
    pub fn open_with(
        input: &str,
        params_json: &str,
        model: Option<String>,
    ) -> Result<Svg3dxSession, JsValue> {
        let params = parse_params_json(params_json)?;
        let (inner, update) = DocumentSession::open_with(input, params, model)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self {
            inner,
            last_update: Some(update),
        })
    }

    #[wasm_bindgen(js_name = setParams)]
    pub fn set_params(&mut self, params_json: &str) -> Result<JsValue, JsValue> {
        let params = parse_params_json(params_json)?;
        let update = self
            .inner
            .set_params(params)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.store_update(update)
    }

    #[wasm_bindgen(js_name = patchParams)]
    pub fn patch_params(&mut self, params_json: &str) -> Result<JsValue, JsValue> {
        let params = parse_params_json(params_json)?;
        let update = self
            .inner
            .patch_params(params)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.store_update(update)
    }

    #[wasm_bindgen(js_name = clearParam)]
    pub fn clear_param(&mut self, name: &str) -> Result<JsValue, JsValue> {
        let update = self
            .inner
            .clear_param(name)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.store_update(update)
    }

    #[wasm_bindgen(js_name = setModel)]
    pub fn set_model(&mut self, model: Option<String>) -> Result<JsValue, JsValue> {
        let update = self
            .inner
            .set_model(model)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.store_update(update)
    }

    pub fn reload(&mut self, input: &str) -> Result<JsValue, JsValue> {
        let update = self
            .inner
            .reload(input)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.store_update(update)
    }

    pub fn evaluate(&mut self) -> Result<JsValue, JsValue> {
        let update = self
            .inner
            .evaluate()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.store_update(update)
    }

    #[wasm_bindgen(js_name = revision)]
    pub fn revision(&self) -> u64 {
        self.inner.revision()
    }

    /// Last [`UpdateResult`] as a JS object.
    #[wasm_bindgen(js_name = lastUpdate)]
    pub fn last_update(&self) -> Result<JsValue, JsValue> {
        match &self.last_update {
            Some(u) => serde_wasm_bindgen::to_value(u).map_err(|e| JsValue::from_str(&e.to_string())),
            None => Ok(JsValue::NULL),
        }
    }

    /// Last update as pretty JSON (convenient for hosts that prefer strings).
    #[wasm_bindgen(js_name = lastUpdateJson)]
    pub fn last_update_json(&self) -> Result<String, JsValue> {
        match &self.last_update {
            Some(u) => update_to_json(u).map_err(|e| JsValue::from_str(&e.to_string())),
            None => Ok("null".into()),
        }
    }

    /// Full evaluation result JSON from the last update.
    #[wasm_bindgen(js_name = resultJson)]
    pub fn result_json(&self) -> Result<String, JsValue> {
        match self.inner.result() {
            Some(r) => to_json(r).map_err(|e| JsValue::from_str(&e.to_string())),
            None => Ok("null".into()),
        }
    }

    /// Declared `--*` params from class rules / root style.
    #[wasm_bindgen(js_name = declaredParams)]
    pub fn declared_params(&self) -> Result<JsValue, JsValue> {
        serde_wasm_bindgen::to_value(&self.inner.declared_params())
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    fn store_update(&mut self, update: UpdateResult) -> Result<JsValue, JsValue> {
        let js = serde_wasm_bindgen::to_value(&update)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        self.last_update = Some(update);
        Ok(js)
    }
}

fn parse_params_json(params_json: &str) -> Result<HashMap<String, Value>, JsValue> {
    if params_json.trim().is_empty() {
        return Ok(HashMap::new());
    }
    let map: HashMap<String, serde_json::Value> = serde_json::from_str(params_json)
        .map_err(|e| JsValue::from_str(&format!("params JSON: {e}")))?;
    let mut external = HashMap::new();
    for (k, v) in map {
        external.insert(k, json_to_value(v));
    }
    Ok(external)
}

fn json_to_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::String(s) => Value::parse_literal(&s),
        serde_json::Value::Array(arr) => {
            let nums: Option<Vec<f64>> = arr.iter().map(|x| x.as_f64()).collect();
            match nums {
                Some(v) => Value::Vector(v),
                None => Value::String(serde_json::Value::Array(arr).to_string()),
            }
        }
        other => Value::parse_literal(&other.to_string()),
    }
}
