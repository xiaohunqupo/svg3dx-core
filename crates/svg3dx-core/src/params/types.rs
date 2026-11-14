use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Runtime value in the svg3dx parameter / expression system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Number(f64),
    Vector(Vec<f64>),
    Bool(bool),
    String(String),
    /// Nested object (e.g. `<s3dx:profile>` attrs after evaluation).
    Map(IndexMap<String, Value>),
    /// Nested list (e.g. multiple loft profiles).
    List(Vec<Value>),
}

impl Value {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Bool(true) => Some(1.0),
            Value::Bool(false) => Some(0.0),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Number(n) => Some(*n != 0.0),
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<&[f64]> {
        match self {
            Value::Vector(v) => Some(v),
            _ => None,
        }
    }

    pub fn component(&self, axis: &str) -> Option<f64> {
        let v = self.as_vector()?;
        let idx = match axis {
            "x" | "X" => 0,
            "y" | "Y" => 1,
            "z" | "Z" => 2,
            _ => return None,
        };
        v.get(idx).copied()
    }

    /// Expand for attribute embedding: vectors become space-separated numbers.
    pub fn expand_for_attr(&self) -> String {
        match self {
            Value::Number(n) => format_number(*n),
            Value::Vector(v) => v
                .iter()
                .map(|n| format_number(*n))
                .collect::<Vec<_>>()
                .join(" "),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => s.clone(),
            Value::Map(_) | Value::List(_) => String::new(),
        }
    }

    /// Parse a CSS custom-property / attribute literal per spec §2.4.
    pub fn parse_literal(raw: &str) -> Self {
        let s = raw.trim();
        if s.is_empty() {
            return Value::String(String::new());
        }
        if s.eq_ignore_ascii_case("true") {
            return Value::Bool(true);
        }
        if s.eq_ignore_ascii_case("false") {
            return Value::Bool(false);
        }
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() == 1 {
            if let Ok(n) = parts[0].parse::<f64>() {
                return Value::Number(n);
            }
            return Value::String(s.to_string());
        }
        let mut nums = Vec::with_capacity(parts.len());
        for p in &parts {
            match p.parse::<f64>() {
                Ok(n) => nums.push(n),
                Err(_) => return Value::String(s.to_string()),
            }
        }
        Value::Vector(nums)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.expand_for_attr())
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_literals() {
        assert_eq!(Value::parse_literal("42"), Value::Number(42.0));
        assert_eq!(
            Value::parse_literal("1 0 0"),
            Value::Vector(vec![1.0, 0.0, 0.0])
        );
        assert_eq!(Value::parse_literal("true"), Value::Bool(true));
        assert_eq!(
            Value::parse_literal("solid"),
            Value::String("solid".into())
        );
    }
}
