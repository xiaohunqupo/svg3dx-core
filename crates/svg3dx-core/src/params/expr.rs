use super::scope::ParamScope;
use super::types::Value;
use crate::error::Warning;

/// Evaluate an expression string in the given scope.
/// Supports: + - * /, comparisons, and/or/not, parentheses, `var(--name)`, `calc(...)`.
pub fn evaluate(expr: &str, scope: &ParamScope) -> Result<Value, Warning> {
    let mut parser = Parser::new(expr.trim(), scope);
    let value = parser.parse_or()?;
    parser.skip_ws();
    if !parser.eof() {
        return Err(Warning::new(format!(
            "trailing input in expression: {}",
            expr
        )));
    }
    Ok(value)
}

/// Resolve an attribute value: literal, whole `var()`/`calc()`, or embedded refs in text.
pub fn resolve_attr(raw: &str, scope: &ParamScope) -> Result<Value, Warning> {
    let s = raw.trim();
    if s.contains('[') {
        return Err(Warning::new(
            "bracket parameter refs are not supported; use var(--name)",
        ));
    }
    if !s.contains("var(") && !s.contains("calc(") {
        return Ok(Value::parse_literal(s));
    }
    // Whole-attribute expression.
    if s.starts_with("var(") || s.starts_with("calc(") {
        return evaluate(s, scope);
    }
    // Embedded refs (e.g. matrix(... var(--loop-value) ...) / calc inside lists).
    let expanded = expand_embedded(s, scope)?;
    Ok(Value::parse_literal(&expanded))
}

fn expand_embedded(input: &str, scope: &ParamScope) -> Result<String, Warning> {
    let mut out = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let rest = &input[byte_index(&chars, i)..];
        if rest.starts_with("calc(") {
            let (expr, next) = take_paren_expr(&chars, i, "calc(")?;
            let val = evaluate(&expr, scope)?;
            out.push_str(&val.expand_for_attr());
            i = next;
        } else if rest.starts_with("var(") {
            let (expr, next) = take_paren_expr(&chars, i, "var(")?;
            let val = evaluate(&expr, scope)?;
            out.push_str(&val.expand_for_attr());
            i = next;
        } else if chars[i] == '[' {
            return Err(Warning::new(
                "bracket parameter refs are not supported; use var(--name)",
            ));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Ok(out)
}

/// From `start` at the beginning of `prefix` (e.g. `calc(`), return the full
/// expression string including prefix/closing `)` and the next char index.
fn take_paren_expr(
    chars: &[char],
    start: usize,
    prefix: &str,
) -> Result<(String, usize), Warning> {
    let prefix_len = prefix.chars().count();
    let mut depth = 0i32;
    let mut i = start;
    while i < chars.len() {
        if i == start {
            i += prefix_len;
            depth = 1;
            continue;
        }
        match chars[i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let expr: String = chars[start..=i].iter().collect();
                    return Ok((expr, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(Warning::new(format!("unclosed {prefix}")))
}

fn byte_index(chars: &[char], char_idx: usize) -> usize {
    chars[..char_idx].iter().map(|c| c.len_utf8()).sum()
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    scope: &'a ParamScope,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, scope: &'a ParamScope) -> Self {
        Self {
            input,
            pos: 0,
            scope,
        }
    }

    fn eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.bump();
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    fn consume(&mut self, s: &str) -> bool {
        if self.starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn parse_or(&mut self) -> Result<Value, Warning> {
        let mut left = self.parse_and()?;
        loop {
            self.skip_ws();
            if self.consume("or") {
                // ensure word boundary
                if matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
                    self.pos -= 2;
                    break;
                }
                let right = self.parse_and()?;
                let lb = left.as_bool().ok_or_else(|| {
                    Warning::new("type mismatch: expected bool in 'or'")
                })?;
                let rb = right.as_bool().ok_or_else(|| {
                    Warning::new("type mismatch: expected bool in 'or'")
                })?;
                left = Value::Bool(lb || rb);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Value, Warning> {
        let mut left = self.parse_not()?;
        loop {
            self.skip_ws();
            if self.consume("and") {
                if matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
                    self.pos -= 3;
                    break;
                }
                let right = self.parse_not()?;
                let lb = left.as_bool().ok_or_else(|| {
                    Warning::new("type mismatch: expected bool in 'and'")
                })?;
                let rb = right.as_bool().ok_or_else(|| {
                    Warning::new("type mismatch: expected bool in 'and'")
                })?;
                left = Value::Bool(lb && rb);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Value, Warning> {
        self.skip_ws();
        if self.consume("not") {
            if matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
                self.pos -= 3;
            } else {
                let v = self.parse_not()?;
                let b = v
                    .as_bool()
                    .ok_or_else(|| Warning::new("type mismatch: expected bool in 'not'"))?;
                return Ok(Value::Bool(!b));
            }
        }
        self.parse_cmp()
    }

    fn parse_cmp(&mut self) -> Result<Value, Warning> {
        let left = self.parse_add()?;
        self.skip_ws();
        let op = if self.consume("<=") {
            Some("<=")
        } else if self.consume(">=") {
            Some(">=")
        } else if self.consume("<>") {
            Some("<>")
        } else if self.consume("=") {
            Some("=")
        } else if self.consume("<") {
            Some("<")
        } else if self.consume(">") {
            Some(">")
        } else {
            None
        };
        if let Some(op) = op {
            let right = self.parse_add()?;
            let ln = left
                .as_number()
                .ok_or_else(|| Warning::new("type mismatch in comparison"))?;
            let rn = right
                .as_number()
                .ok_or_else(|| Warning::new("type mismatch in comparison"))?;
            let result = match op {
                "=" => (ln - rn).abs() < 1e-9,
                "<>" => (ln - rn).abs() >= 1e-9,
                "<" => ln < rn,
                ">" => ln > rn,
                "<=" => ln <= rn,
                ">=" => ln >= rn,
                _ => unreachable!(),
            };
            Ok(Value::Bool(result))
        } else {
            Ok(left)
        }
    }

    fn parse_add(&mut self) -> Result<Value, Warning> {
        let mut left = self.parse_mul()?;
        loop {
            self.skip_ws();
            if self.consume("+") {
                let right = self.parse_mul()?;
                left = add_values(&left, &right)?;
            } else if self.consume("-") {
                let right = self.parse_mul()?;
                left = sub_values(&left, &right)?;
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Value, Warning> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_ws();
            if self.consume("*") {
                let right = self.parse_unary()?;
                left = mul_values(&left, &right)?;
            } else if self.consume("/") {
                let right = self.parse_unary()?;
                left = div_values(&left, &right)?;
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Value, Warning> {
        self.skip_ws();
        if self.consume("-") {
            let v = self.parse_unary()?;
            return neg_value(&v);
        }
        if self.consume("+") {
            return self.parse_unary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Value, Warning> {
        self.skip_ws();
        if self.consume("calc(") {
            // CSS-aligned: arithmetic only inside calc().
            let v = self.parse_add()?;
            self.skip_ws();
            if !self.consume(")") {
                return Err(Warning::new("unclosed calc("));
            }
            return Ok(v);
        }
        if self.consume("(") {
            let v = self.parse_or()?;
            self.skip_ws();
            if !self.consume(")") {
                return Err(Warning::new("expected ')'"));
            }
            return Ok(v);
        }
        if self.consume("var(") {
            return self.parse_var_ref();
        }
        if self.consume("true") {
            return Ok(Value::Bool(true));
        }
        if self.consume("false") {
            return Ok(Value::Bool(false));
        }
        // number
        let start = self.pos;
        if matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.') {
            while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-')
            {
                // careful with trailing +/-
                let c = self.peek().unwrap();
                if (c == '+' || c == '-') && self.pos > start {
                    let prev = self.input.as_bytes()[self.pos - 1];
                    if prev != b'e' && prev != b'E' {
                        break;
                    }
                }
                self.bump();
            }
            let num_str = &self.input[start..self.pos];
            let n: f64 = num_str
                .parse()
                .map_err(|_| Warning::new(format!("invalid number: {num_str}")))?;
            return Ok(Value::Number(n));
        }
        Err(Warning::new(format!(
            "unexpected token near: {}",
            &self.input[self.pos..]
        )))
    }

    fn parse_var_ref(&mut self) -> Result<Value, Warning> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == ')' {
                break;
            }
            self.bump();
        }
        let inner = self.input[start..self.pos].trim();
        if !self.consume(")") {
            return Err(Warning::new("unclosed var("));
        }
        if !inner.starts_with("--") {
            return Err(Warning::new(
                "var() requires a dashed-ident like --name",
            ));
        }
        let name = &inner[2..];
        if name.is_empty() {
            return Err(Warning::new("empty var(--*) name"));
        }
        self.lookup_name(name)
    }

    fn lookup_name(&self, name: &str) -> Result<Value, Warning> {
        self.scope
            .lookup(name)
            .ok_or_else(|| Warning::new(format!("undefined parameter: {name}")))
    }
}

fn add_values(a: &Value, b: &Value) -> Result<Value, Warning> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x + y)),
        (Value::Vector(x), Value::Vector(y)) if x.len() == y.len() => {
            Ok(Value::Vector(x.iter().zip(y).map(|(a, b)| a + b).collect()))
        }
        (Value::Vector(x), Value::Number(y)) => {
            Ok(Value::Vector(x.iter().map(|a| a + y).collect()))
        }
        (Value::Number(x), Value::Vector(y)) => {
            Ok(Value::Vector(y.iter().map(|a| x + a).collect()))
        }
        _ => Err(Warning::new("type mismatch in addition")),
    }
}

fn sub_values(a: &Value, b: &Value) -> Result<Value, Warning> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x - y)),
        (Value::Vector(x), Value::Vector(y)) if x.len() == y.len() => {
            Ok(Value::Vector(x.iter().zip(y).map(|(a, b)| a - b).collect()))
        }
        (Value::Vector(x), Value::Number(y)) => {
            Ok(Value::Vector(x.iter().map(|a| a - y).collect()))
        }
        _ => Err(Warning::new("type mismatch in subtraction")),
    }
}

fn mul_values(a: &Value, b: &Value) -> Result<Value, Warning> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x * y)),
        (Value::Vector(x), Value::Number(y)) | (Value::Number(y), Value::Vector(x)) => {
            Ok(Value::Vector(x.iter().map(|a| a * y).collect()))
        }
        _ => Err(Warning::new("type mismatch in multiplication")),
    }
}

fn div_values(a: &Value, b: &Value) -> Result<Value, Warning> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            if *y == 0.0 {
                return Err(Warning::new("division by zero"));
            }
            Ok(Value::Number(x / y))
        }
        (Value::Vector(x), Value::Number(y)) => {
            if *y == 0.0 {
                return Err(Warning::new("division by zero"));
            }
            Ok(Value::Vector(x.iter().map(|a| a / y).collect()))
        }
        _ => Err(Warning::new("type mismatch in division")),
    }
}

fn neg_value(v: &Value) -> Result<Value, Warning> {
    match v {
        Value::Number(n) => Ok(Value::Number(-n)),
        Value::Vector(x) => Ok(Value::Vector(x.iter().map(|a| -a).collect())),
        _ => Err(Warning::new("type mismatch in unary minus")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::scope::ParamScope;

    #[test]
    fn arithmetic_and_compare() {
        let mut scope = ParamScope::new();
        scope.set_local("R", Value::Number(50.0));
        scope.set_local("D", Value::Number(200.0));
        assert_eq!(
            evaluate("var(--R) = 50", &scope).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            evaluate("calc(var(--D) - 10)", &scope).unwrap(),
            Value::Number(190.0)
        );
        assert_eq!(
            evaluate("calc(var(--R) / 3)", &scope).unwrap(),
            Value::Number(50.0 / 3.0)
        );
    }

    #[test]
    fn loop_value_and_components() {
        let mut scope = ParamScope::new();
        scope.set_local("loop-value", Value::Vector(vec![1.0, 2.0, 3.0]));
        scope.set_local("loop-value-x", Value::Number(1.0));
        assert_eq!(
            evaluate("var(--loop-value)", &scope).unwrap(),
            Value::Vector(vec![1.0, 2.0, 3.0])
        );
        assert_eq!(
            evaluate("calc(var(--loop-value-x) * 50)", &scope).unwrap(),
            Value::Number(50.0)
        );
    }

    #[test]
    fn reject_legacy_bracket_and_bare_var() {
        let scope = ParamScope::new();
        assert!(resolve_attr("[R]", &scope).is_err());
        assert!(evaluate("var(R)", &scope).is_err());
    }
}
