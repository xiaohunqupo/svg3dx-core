//! C ABI for svg3dx-core.
//!
//! Memory: Rust allocates, caller frees via `svg3dx_free_result` /
//! `svg3dx_session_free`.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::ptr;
use std::slice;

use svg3dx_core::{
    parse_and_evaluate_with, to_json, update_to_json, DocumentSession, EvalOptions, UpdateResult,
    Value,
};

pub struct Svg3dxResult {
    json: CString,
    warnings_len: usize,
    ok: bool,
}

pub struct Svg3dxSession {
    inner: DocumentSession,
}

/// Parse + evaluate an svg3dx UTF-8 document.
///
/// Returns an opaque pointer; NULL on allocation failure only.
/// Check `svg3dx_result_ok` for evaluation success.
#[no_mangle]
pub extern "C" fn svg3dx_parse(input: *const u8, input_len: usize) -> *mut Svg3dxResult {
    svg3dx_parse_with_params(input, input_len, ptr::null(), ptr::null())
}

/// `params_json`: JSON object of external params; may be null.
/// `model`: optional active model id; may be null.
#[no_mangle]
pub extern "C" fn svg3dx_parse_with_params(
    input: *const u8,
    input_len: usize,
    params_json: *const c_char,
    model: *const c_char,
) -> *mut Svg3dxResult {
    if input.is_null() && input_len != 0 {
        return ptr::null_mut();
    }
    let bytes = if input_len == 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(input, input_len) }
    };
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => return box_err(format!("invalid UTF-8: {e}")),
    };

    let mut external = HashMap::new();
    if !params_json.is_null() {
        let raw = unsafe { CStr::from_ptr(params_json) };
        match raw.to_str() {
            Ok(s) if !s.is_empty() => match serde_json::from_str::<serde_json::Value>(s) {
                Ok(serde_json::Value::Object(map)) => {
                    for (k, v) in map {
                        external.insert(k, json_to_value(v));
                    }
                }
                Ok(_) => return box_err("params_json must be a JSON object".into()),
                Err(e) => return box_err(format!("params_json: {e}")),
            },
            Ok(_) => {}
            Err(e) => return box_err(format!("params_json UTF-8: {e}")),
        }
    }

    let model_id = if model.is_null() {
        None
    } else {
        match unsafe { CStr::from_ptr(model) }.to_str() {
            Ok(s) if !s.is_empty() => Some(s.to_string()),
            Ok(_) => None,
            Err(e) => return box_err(format!("model UTF-8: {e}")),
        }
    };

    let opts = EvalOptions {
        external,
        model: model_id,
    };

    match parse_and_evaluate_with(text, opts) {
        Ok(result) => box_ok(result),
        Err(e) => box_err(e.to_string()),
    }
}

fn json_to_value(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::String(s) => Value::parse_literal(&s),
        serde_json::Value::Array(arr) => {
            let nums: Option<Vec<f64>> = arr
                .iter()
                .map(|x| x.as_f64())
                .collect();
            match nums {
                Some(v) => Value::Vector(v),
                None => Value::String(serde_json::Value::Array(arr).to_string()),
            }
        }
        other => Value::parse_literal(&other.to_string()),
    }
}

fn box_ok(result: svg3dx_core::EvaluationResult) -> *mut Svg3dxResult {
    let warnings_len = result.warnings.len();
    match to_json(&result) {
        Ok(json) => match CString::new(json) {
            Ok(c) => Box::into_raw(Box::new(Svg3dxResult {
                json: c,
                warnings_len,
                ok: true,
            })),
            Err(_) => box_err("JSON contained NUL byte".into()),
        },
        Err(e) => box_err(e.to_string()),
    }
}

fn box_update_ok(update: UpdateResult) -> *mut Svg3dxResult {
    let warnings_len = update.result.warnings.len();
    match update_to_json(&update) {
        Ok(json) => match CString::new(json) {
            Ok(c) => Box::into_raw(Box::new(Svg3dxResult {
                json: c,
                warnings_len,
                ok: true,
            })),
            Err(_) => box_err("JSON contained NUL byte".into()),
        },
        Err(e) => box_err(e.to_string()),
    }
}

fn box_err(msg: String) -> *mut Svg3dxResult {
    let json = CString::new(format!(
        r#"{{"error":{}}}"#,
        serde_json::to_string(&msg).unwrap()
    ))
    .unwrap_or_default();
    Box::into_raw(Box::new(Svg3dxResult {
        json,
        warnings_len: 0,
        ok: false,
    }))
}

fn read_utf8(input: *const u8, input_len: usize) -> std::result::Result<String, String> {
    if input.is_null() && input_len != 0 {
        return Err("null input with non-zero length".into());
    }
    let bytes = if input_len == 0 {
        &[][..]
    } else {
        unsafe { slice::from_raw_parts(input, input_len) }
    };
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .map_err(|e| format!("invalid UTF-8: {e}"))
}

fn parse_params_cstr(params_json: *const c_char) -> std::result::Result<HashMap<String, Value>, String> {
    let mut external = HashMap::new();
    if params_json.is_null() {
        return Ok(external);
    }
    let raw = unsafe { CStr::from_ptr(params_json) };
    match raw.to_str() {
        Ok(s) if s.is_empty() => Ok(external),
        Ok(s) => match serde_json::from_str::<serde_json::Value>(s) {
            Ok(serde_json::Value::Object(map)) => {
                for (k, v) in map {
                    external.insert(k, json_to_value(v));
                }
                Ok(external)
            }
            Ok(_) => Err("params_json must be a JSON object".into()),
            Err(e) => Err(format!("params_json: {e}")),
        },
        Err(e) => Err(format!("params_json UTF-8: {e}")),
    }
}

fn optional_cstr(p: *const c_char) -> std::result::Result<Option<String>, String> {
    if p.is_null() {
        return Ok(None);
    }
    match unsafe { CStr::from_ptr(p) }.to_str() {
        Ok(s) if s.is_empty() => Ok(None),
        Ok(s) => Ok(Some(s.to_string())),
        Err(e) => Err(format!("UTF-8: {e}")),
    }
}

/// Open a document session. Returns NULL on allocation failure; check update via out param.
/// On success, `*out_update` receives the first UpdateResult JSON (caller frees with svg3dx_free_result).
#[no_mangle]
pub extern "C" fn svg3dx_session_open(
    input: *const u8,
    input_len: usize,
    out_update: *mut *mut Svg3dxResult,
) -> *mut Svg3dxSession {
    if !out_update.is_null() {
        unsafe { *out_update = ptr::null_mut() };
    }
    let text = match read_utf8(input, input_len) {
        Ok(s) => s,
        Err(e) => {
            if !out_update.is_null() {
                unsafe { *out_update = box_err(e) };
            }
            return ptr::null_mut();
        }
    };
    match DocumentSession::open(&text) {
        Ok((inner, update)) => {
            if !out_update.is_null() {
                unsafe { *out_update = box_update_ok(update) };
            }
            Box::into_raw(Box::new(Svg3dxSession { inner }))
        }
        Err(e) => {
            if !out_update.is_null() {
                unsafe { *out_update = box_err(e.to_string()) };
            }
            ptr::null_mut()
        }
    }
}

/// Merge external params JSON object and re-evaluate. Returns UpdateResult JSON.
#[no_mangle]
pub extern "C" fn svg3dx_session_patch_params(
    session: *mut Svg3dxSession,
    params_json: *const c_char,
) -> *mut Svg3dxResult {
    if session.is_null() {
        return box_err("null session".into());
    }
    let params = match parse_params_cstr(params_json) {
        Ok(p) => p,
        Err(e) => return box_err(e),
    };
    match unsafe { &mut *session }.inner.patch_params(params) {
        Ok(u) => box_update_ok(u),
        Err(e) => box_err(e.to_string()),
    }
}

/// Replace external params JSON object and re-evaluate. Returns UpdateResult JSON.
#[no_mangle]
pub extern "C" fn svg3dx_session_set_params(
    session: *mut Svg3dxSession,
    params_json: *const c_char,
) -> *mut Svg3dxResult {
    if session.is_null() {
        return box_err("null session".into());
    }
    let params = match parse_params_cstr(params_json) {
        Ok(p) => p,
        Err(e) => return box_err(e),
    };
    match unsafe { &mut *session }.inner.set_params(params) {
        Ok(u) => box_update_ok(u),
        Err(e) => box_err(e.to_string()),
    }
}

/// Select active s3dx:model by id (NULL clears override). Returns UpdateResult JSON.
#[no_mangle]
pub extern "C" fn svg3dx_session_set_model(
    session: *mut Svg3dxSession,
    model: *const c_char,
) -> *mut Svg3dxResult {
    if session.is_null() {
        return box_err("null session".into());
    }
    let model = match optional_cstr(model) {
        Ok(m) => m,
        Err(e) => return box_err(e),
    };
    match unsafe { &mut *session }.inner.set_model(model) {
        Ok(u) => box_update_ok(u),
        Err(e) => box_err(e.to_string()),
    }
}

/// Re-parse document text and re-evaluate. Returns UpdateResult JSON.
#[no_mangle]
pub extern "C" fn svg3dx_session_reload(
    session: *mut Svg3dxSession,
    input: *const u8,
    input_len: usize,
) -> *mut Svg3dxResult {
    if session.is_null() {
        return box_err("null session".into());
    }
    let text = match read_utf8(input, input_len) {
        Ok(s) => s,
        Err(e) => return box_err(e),
    };
    match unsafe { &mut *session }.inner.reload(&text) {
        Ok(u) => box_update_ok(u),
        Err(e) => box_err(e.to_string()),
    }
}

/// Force re-evaluate with current params/model. Returns UpdateResult JSON.
#[no_mangle]
pub extern "C" fn svg3dx_session_evaluate(session: *mut Svg3dxSession) -> *mut Svg3dxResult {
    if session.is_null() {
        return box_err("null session".into());
    }
    match unsafe { &mut *session }.inner.evaluate() {
        Ok(u) => box_update_ok(u),
        Err(e) => box_err(e.to_string()),
    }
}

/// Current session revision (increments on successful re-evaluate).
#[no_mangle]
pub extern "C" fn svg3dx_session_revision(session: *const Svg3dxSession) -> u64 {
    if session.is_null() {
        return 0;
    }
    unsafe { (*session).inner.revision() }
}

/// Free a session opened by `svg3dx_session_open`.
#[no_mangle]
pub extern "C" fn svg3dx_session_free(session: *mut Svg3dxSession) {
    if session.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(session));
    }
}

/// True if the result JSON is a successful EvaluationResult / UpdateResult.
#[no_mangle]
pub extern "C" fn svg3dx_result_ok(result: *const Svg3dxResult) -> bool {
    if result.is_null() {
        return false;
    }
    unsafe { (*result).ok }
}

/// NUL-terminated JSON bytes (EvaluationResult, UpdateResult, or `{"error":"..."}`).
#[no_mangle]
pub extern "C" fn svg3dx_result_json(result: *const Svg3dxResult) -> *const c_char {
    if result.is_null() {
        return ptr::null();
    }
    unsafe { (*result).json.as_ptr() }
}

/// Byte length of JSON without trailing NUL.
#[no_mangle]
pub extern "C" fn svg3dx_result_len(result: *const Svg3dxResult) -> usize {
    if result.is_null() {
        return 0;
    }
    unsafe { (*result).json.as_bytes().len() }
}

/// Number of evaluation warnings associated with a successful result.
#[no_mangle]
pub extern "C" fn svg3dx_result_warning_count(result: *const Svg3dxResult) -> usize {
    if result.is_null() {
        return 0;
    }
    unsafe { (*result).warnings_len }
}

/// Free a result returned by parse / session APIs.
#[no_mangle]
pub extern "C" fn svg3dx_free_result(result: *mut Svg3dxResult) {
    if result.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(result));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn parse_minimal() {
        let svg = br##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs><g id="g1"><s3dx:sphere radius="10" center="0 0 0"/></g></defs>
          <s3dx:model id="m" mode="solid" active="true"><s3dx:use href="#g1"/></s3dx:model>
        </svg>"##;
        let result = svg3dx_parse(svg.as_ptr(), svg.len());
        assert!(!result.is_null());
        unsafe {
            assert!(svg3dx_result_ok(result));
            let json = CStr::from_ptr(svg3dx_result_json(result))
                .to_str()
                .unwrap();
            assert!(json.contains("sphere") || json.contains("radius"));
            svg3dx_free_result(result);
        }
    }

    #[test]
    fn parse_with_model_override() {
        let svg = br##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <defs><g id="g1"><s3dx:box length="1" width="1" height="1"
                    lengthVec="1 0 0" widthVec="0 1 0"/></g></defs>
          <s3dx:model id="solid-view" mode="solid" active="true">
            <s3dx:use href="#g1"/>
          </s3dx:model>
          <s3dx:model id="wireframe-view" mode="wireframe">
            <s3dx:use href="#g1"/>
          </s3dx:model>
        </svg>"##;
        let model = CString::new("wireframe-view").unwrap();
        let params = CString::new("{}").unwrap();
        let result = svg3dx_parse_with_params(
            svg.as_ptr(),
            svg.len(),
            params.as_ptr(),
            model.as_ptr(),
        );
        assert!(!result.is_null());
        unsafe {
            assert!(svg3dx_result_ok(result));
            let json = CStr::from_ptr(svg3dx_result_json(result))
                .to_str()
                .unwrap();
            assert!(json.contains("wireframe-view"));
            svg3dx_free_result(result);
        }
    }

    #[test]
    fn session_patch_params() {
        let svg = br##"<svg svg3d="true" xmlns="http://www.w3.org/2000/svg"
             xmlns:s3dx="urn:s3dx:draft:0.1">
          <style>.t { --D: 100; }</style>
          <defs>
            <g id="g" class="t">
              <s3dx:cylinder diameter="60" bottomCenterPt="0 0 0" topCenterPt="0 0 100">
                <s3dx:exp diameter="var(--D)"/>
              </s3dx:cylinder>
            </g>
          </defs>
          <s3dx:model id="m" mode="solid" active="true"><s3dx:use href="#g"/></s3dx:model>
        </svg>"##;
        let mut update_ptr: *mut Svg3dxResult = ptr::null_mut();
        let session = svg3dx_session_open(svg.as_ptr(), svg.len(), &mut update_ptr);
        assert!(!session.is_null());
        assert!(!update_ptr.is_null());
        assert!(svg3dx_result_ok(update_ptr));
        svg3dx_free_result(update_ptr);
        let patch = CString::new(r#"{"D":180}"#).unwrap();
        let update = svg3dx_session_patch_params(session, patch.as_ptr());
        assert!(!update.is_null());
        assert!(svg3dx_result_ok(update));
        let json = unsafe { CStr::from_ptr(svg3dx_result_json(update)) }
            .to_str()
            .unwrap();
        assert!(json.contains("\"changes\""));
        assert!(json.contains("180") || json.contains("\"D\""));
        assert!(svg3dx_session_revision(session) >= 2);
        svg3dx_free_result(update);
        svg3dx_session_free(session);
    }
}
