use crate::error::Warning;
use crate::params::{resolve_attr, ParamScope, Value};
use crate::parser::Element;

pub enum LoopIter {
    Points(Vec<Value>),
    Range { values: Vec<f64> },
}

/// Parse loop iteration sequence. Returns None on skip (unknown / error).
pub fn prepare_loop(
    el: &Element,
    scope: &ParamScope,
    warnings: &mut Vec<String>,
) -> Option<LoopIter> {
    if el.is_s3dx("loop-points") {
        let raw = el.attr("points").unwrap_or("");
        let val = match resolve_attr(raw, scope) {
            Ok(v) => v,
            Err(Warning { message, .. }) => {
                warnings.push(format!("loop-points: {message}"));
                return None;
            }
        };
        let nums = match val {
            Value::Vector(v) => v,
            Value::String(s) => {
                match Value::parse_literal(&s) {
                    Value::Vector(v) => v,
                    _ => {
                        warnings.push("loop-points: points must be numeric triples".into());
                        return None;
                    }
                }
            }
            Value::Number(n) => vec![n],
            _ => {
                warnings.push("loop-points: invalid points".into());
                return None;
            }
        };
        if nums.len() % 3 != 0 {
            warnings.push(format!(
                "loop-points: points length {} is not a multiple of 3",
                nums.len()
            ));
            return None;
        }
        let points = nums
            .chunks(3)
            .map(|c| Value::Vector(c.to_vec()))
            .collect();
        Some(LoopIter::Points(points))
    } else if el.is_s3dx("loop-range") {
        let start = num_attr(el, "start", scope, warnings)?;
        let end = num_attr(el, "end", scope, warnings)?;
        let step = match el.attr("step") {
            Some(s) => num_expr(s, scope, warnings)?,
            None => 1.0,
        };
        if step == 0.0 {
            warnings.push("loop-range: step must not be 0".into());
            return None;
        }
        let mut values = Vec::new();
        let mut cur = start;
        if step > 0.0 {
            while cur < end {
                values.push(cur);
                cur += step;
            }
        } else {
            while cur > end {
                values.push(cur);
                cur += step;
            }
        }
        Some(LoopIter::Range { values })
    } else {
        // unknown loop-* → skip silently per spec
        None
    }
}

fn num_attr(
    el: &Element,
    name: &str,
    scope: &ParamScope,
    warnings: &mut Vec<String>,
) -> Option<f64> {
    let raw = el.attr(name)?;
    num_expr(raw, scope, warnings)
}

fn num_expr(raw: &str, scope: &ParamScope, warnings: &mut Vec<String>) -> Option<f64> {
    match resolve_attr(raw, scope) {
        Ok(v) => v.as_number().or_else(|| {
            warnings.push(format!("expected number, got {v}"));
            None
        }),
        Err(Warning { message, .. }) => {
            warnings.push(message);
            None
        }
    }
}
