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
    open_document_bytes, open_svg3dxz, parse_and_evaluate_with, to_json, update_to_json,
    write_svg3dxz, DocumentSession, EvalOptions, Svg3dxPackage, UpdateResult, Value,
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
        retain_inactive: false,
        extra_defs: Vec::new(),
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

pub struct Svg3dxPackageHandle {
    inner: Svg3dxPackage,
}

pub struct Svg3dxBuffer {
    data: Vec<u8>,
}

/// Open a `.svg3dxz` ZIP. On failure returns NULL and optionally fills `*out_error`
/// with a malloc'd C string (caller frees via `svg3dx_string_free`).
#[no_mangle]
pub extern "C" fn svg3dx_package_open(
    data: *const u8,
    data_len: usize,
    out_error: *mut *mut c_char,
) -> *mut Svg3dxPackageHandle {
    set_out_error(out_error, None);
    let bytes = match raw_bytes(data, data_len) {
        Ok(b) => b,
        Err(e) => {
            set_out_error(out_error, Some(e));
            return ptr::null_mut();
        }
    };
    match open_svg3dxz(bytes) {
        Ok(inner) => Box::into_raw(Box::new(Svg3dxPackageHandle { inner })),
        Err(e) => {
            set_out_error(out_error, Some(e.to_string()));
            ptr::null_mut()
        }
    }
}

/// Open standalone UTF-8 `.svg3dx` or `.svg3dxz` ZIP.
/// `hint` may be NULL; typically a file name like `model.svg3dxz`.
#[no_mangle]
pub extern "C" fn svg3dx_package_open_document(
    data: *const u8,
    data_len: usize,
    hint: *const c_char,
    out_error: *mut *mut c_char,
) -> *mut Svg3dxPackageHandle {
    set_out_error(out_error, None);
    let bytes = match raw_bytes(data, data_len) {
        Ok(b) => b,
        Err(e) => {
            set_out_error(out_error, Some(e));
            return ptr::null_mut();
        }
    };
    let hint = match optional_cstr(hint) {
        Ok(h) => h,
        Err(e) => {
            set_out_error(out_error, Some(e));
            return ptr::null_mut();
        }
    };
    match open_document_bytes(bytes, hint.as_deref()) {
        Ok(inner) => Box::into_raw(Box::new(Svg3dxPackageHandle { inner })),
        Err(e) => {
            set_out_error(out_error, Some(e.to_string()));
            ptr::null_mut()
        }
    }
}

/// Create an empty package with the given UTF-8 `main.svg3dx` text.
#[no_mangle]
pub extern "C" fn svg3dx_package_create(
    main_xml: *const u8,
    main_xml_len: usize,
    out_error: *mut *mut c_char,
) -> *mut Svg3dxPackageHandle {
    set_out_error(out_error, None);
    let text = match read_utf8(main_xml, main_xml_len) {
        Ok(s) => s,
        Err(e) => {
            set_out_error(out_error, Some(e));
            return ptr::null_mut();
        }
    };
    Box::into_raw(Box::new(Svg3dxPackageHandle {
        inner: Svg3dxPackage::new(text),
    }))
}

/// Add or replace a sidecar asset. `path` is a relative package path (NUL-terminated).
/// Returns false on error (see `out_error`).
#[no_mangle]
pub extern "C" fn svg3dx_package_set_asset(
    package: *mut Svg3dxPackageHandle,
    path: *const c_char,
    data: *const u8,
    data_len: usize,
    out_error: *mut *mut c_char,
) -> bool {
    set_out_error(out_error, None);
    if package.is_null() {
        set_out_error(out_error, Some("null package".into()));
        return false;
    }
    let path = match optional_cstr(path) {
        Ok(Some(p)) => p,
        Ok(None) => {
            set_out_error(out_error, Some("null path".into()));
            return false;
        }
        Err(e) => {
            set_out_error(out_error, Some(e));
            return false;
        }
    };
    let bytes = match raw_bytes(data, data_len) {
        Ok(b) => b.to_vec(),
        Err(e) => {
            set_out_error(out_error, Some(e));
            return false;
        }
    };
    match unsafe { &mut *package }.inner.set_asset(path, bytes) {
        Ok(()) => true,
        Err(e) => {
            set_out_error(out_error, Some(e.to_string()));
            false
        }
    }
}

/// UTF-8 pointer to `main.svg3dx` text (not NUL-terminated; use `svg3dx_package_main_xml_len`).
#[no_mangle]
pub extern "C" fn svg3dx_package_main_xml(package: *const Svg3dxPackageHandle) -> *const u8 {
    if package.is_null() {
        return ptr::null();
    }
    unsafe { (&(*package).inner.main_xml).as_ptr() }
}

#[no_mangle]
pub extern "C" fn svg3dx_package_main_xml_len(package: *const Svg3dxPackageHandle) -> usize {
    if package.is_null() {
        return 0;
    }
    unsafe { (&(*package).inner.main_xml).len() }
}

#[no_mangle]
pub extern "C" fn svg3dx_package_asset_count(package: *const Svg3dxPackageHandle) -> usize {
    if package.is_null() {
        return 0;
    }
    unsafe { (&(*package).inner.assets).len() }
}

/// Relative path bytes at `index` (UTF-8, not NUL-terminated). Length via
/// `svg3dx_package_asset_path_len`.
#[no_mangle]
pub extern "C" fn svg3dx_package_asset_path(
    package: *const Svg3dxPackageHandle,
    index: usize,
) -> *const u8 {
    if package.is_null() {
        return ptr::null();
    }
    let assets = unsafe { &(*package).inner.assets };
    match assets.get_index(index) {
        Some((path, _)) => path.as_ptr(),
        None => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn svg3dx_package_asset_path_len(
    package: *const Svg3dxPackageHandle,
    index: usize,
) -> usize {
    if package.is_null() {
        return 0;
    }
    let assets = unsafe { &(*package).inner.assets };
    match assets.get_index(index) {
        Some((path, _)) => path.len(),
        None => 0,
    }
}

/// Asset bytes at `index` (not owned; valid until package is freed). Length via
/// `svg3dx_package_asset_len`.
#[no_mangle]
pub extern "C" fn svg3dx_package_asset_data(
    package: *const Svg3dxPackageHandle,
    index: usize,
) -> *const u8 {
    if package.is_null() {
        return ptr::null();
    }
    let assets = unsafe { &(*package).inner.assets };
    match assets.get_index(index) {
        Some((_, data)) => data.as_ptr(),
        None => ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn svg3dx_package_asset_len(
    package: *const Svg3dxPackageHandle,
    index: usize,
) -> usize {
    if package.is_null() {
        return 0;
    }
    let assets = unsafe { &(*package).inner.assets };
    match assets.get_index(index) {
        Some((_, data)) => data.len(),
        None => 0,
    }
}

/// Write package to `.svg3dxz` bytes. Caller frees with `svg3dx_buffer_free`.
#[no_mangle]
pub extern "C" fn svg3dx_package_write(
    package: *const Svg3dxPackageHandle,
    out_error: *mut *mut c_char,
) -> *mut Svg3dxBuffer {
    set_out_error(out_error, None);
    if package.is_null() {
        set_out_error(out_error, Some("null package".into()));
        return ptr::null_mut();
    }
    match write_svg3dxz(unsafe { &(*package).inner }) {
        Ok(data) => Box::into_raw(Box::new(Svg3dxBuffer { data })),
        Err(e) => {
            set_out_error(out_error, Some(e.to_string()));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn svg3dx_buffer_data(buf: *const Svg3dxBuffer) -> *const u8 {
    if buf.is_null() {
        return ptr::null();
    }
    unsafe { (&(*buf).data).as_ptr() }
}

#[no_mangle]
pub extern "C" fn svg3dx_buffer_len(buf: *const Svg3dxBuffer) -> usize {
    if buf.is_null() {
        return 0;
    }
    unsafe { (&(*buf).data).len() }
}

#[no_mangle]
pub extern "C" fn svg3dx_buffer_free(buf: *mut Svg3dxBuffer) {
    if buf.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(buf));
    }
}

#[no_mangle]
pub extern "C" fn svg3dx_package_free(package: *mut Svg3dxPackageHandle) {
    if package.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(package));
    }
}

/// Free a C string allocated by package APIs (`out_error`).
#[no_mangle]
pub extern "C" fn svg3dx_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    unsafe {
        drop(CString::from_raw(s));
    }
}

fn raw_bytes<'a>(data: *const u8, data_len: usize) -> std::result::Result<&'a [u8], String> {
    if data.is_null() && data_len != 0 {
        return Err("null data with non-zero length".into());
    }
    if data_len == 0 {
        return Ok(&[]);
    }
    Ok(unsafe { slice::from_raw_parts(data, data_len) })
}

fn set_out_error(out_error: *mut *mut c_char, msg: Option<String>) {
    if out_error.is_null() {
        return;
    }
    unsafe {
        *out_error = match msg {
            Some(m) => CString::new(m)
                .unwrap_or_else(|_| CString::new("error").unwrap())
                .into_raw(),
            None => ptr::null_mut(),
        };
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
