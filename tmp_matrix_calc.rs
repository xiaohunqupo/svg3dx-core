use svg3dx_core::geometry::Mat4x3;
use svg3dx_core::params::{resolve_attr, ParamScope, Value};
fn main() {
  let scope = ParamScope::new();
  let raw = ""matrix(1 0 0 0 0 1 0 calc(-1) 0 100 0 0)"";
  match resolve_attr(raw, &scope) {
    Ok(v) => {
      println!("resolved={}", v.expand_for_attr());
      match Mat4x3::from_value(&v).or_else(|_| Mat4x3::parse(&v.expand_for_attr())) {
        Ok(m) => println!("m={:?}", m.m),
        Err(e) => println!("parse_err={}", e.message),
      }
    }
    Err(e) => println!("resolve_err={}", e.message),
  }
  let raw2 = ""matrix(1 0 0 0 0 1 0 -1 0 100 0 0)"";
  match Mat4x3::parse(raw2) {
    Ok(m) => println!("bare_ok m8={} tx={}", m.m[7], m.m[9]),
    Err(e) => println!("bare_err={}", e.message),
  }
}
