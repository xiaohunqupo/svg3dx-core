use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::Warning;
use crate::params::types::Value;

/// How `s3dx:transform` was written (self-describing from syntax).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformSyntax {
    /// `matrix(...)` or bare 12-number list.
    Matrix,
    /// `translate(...) rotate(...) scale(...)` (any subset).
    Trs,
}

/// 12-number 3D transform: 3×3 linear + translation.
/// Layout: [a b c  d e f  g h i  tx ty tz]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Mat4x3 {
    pub m: [f64; 12],
}

impl Default for Mat4x3 {
    fn default() -> Self {
        Self::identity()
    }
}

impl Mat4x3 {
    pub fn identity() -> Self {
        Self {
            m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn from_slice(v: &[f64]) -> Result<Self, Warning> {
        if v.len() != 12 {
            return Err(Warning::new(format!(
                "3D matrix requires 12 numbers, got {}",
                v.len()
            )));
        }
        let mut m = [0.0; 12];
        m.copy_from_slice(v);
        Ok(Self { m })
    }

    /// Compose TRS: `M = T · R · S` (scale, then rotate, then translate).
    /// `R = Rz · Ry · Rx`: a column vector rotates about the fixed axes X, then Y, then Z.
    pub fn from_trs(
        tx: f64,
        ty: f64,
        tz: f64,
        rx_deg: f64,
        ry_deg: f64,
        rz_deg: f64,
        sx: f64,
        sy: f64,
        sz: f64,
    ) -> Self {
        let s = Self {
            m: [sx, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, sz, 0.0, 0.0, 0.0],
        };
        let r = rotation_xyz_deg(rx_deg, ry_deg, rz_deg);
        let t = Self {
            m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, tx, ty, tz],
        };
        t.mul(r.mul(s))
    }

    /// Detect matrix vs TRS from attribute text (before or after expression expand).
    pub fn detect_syntax(raw: &str) -> Option<TransformSyntax> {
        let s = raw.trim();
        if s.is_empty() {
            return None;
        }
        let lower = s.to_ascii_lowercase();
        let has_matrix = lower.contains("matrix(");
        let has_trs = looks_like_trs(&lower);
        if has_matrix && has_trs {
            // Mixed: prefer first recognizable form.
            if let Some(first) = first_transform_keyword(&lower) {
                return Some(match first {
                    "matrix" => TransformSyntax::Matrix,
                    _ => TransformSyntax::Trs,
                });
            }
        }
        if has_matrix {
            return Some(TransformSyntax::Matrix);
        }
        if has_trs {
            return Some(TransformSyntax::Trs);
        }
        // Bare number list → matrix.
        if s
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .all(|t| t.parse::<f64>().is_ok())
        {
            return Some(TransformSyntax::Matrix);
        }
        None
    }

    /// Parse `matrix(...)`, bare 12-number list, or TRS function list.
    /// Embedded expressions should already be expanded into numbers.
    pub fn parse(raw: &str) -> Result<Self, Warning> {
        let s = raw.trim();
        if s.is_empty() {
            return Ok(Self::identity());
        }
        match Self::detect_syntax(s) {
            Some(TransformSyntax::Trs) => Self::parse_trs(s),
            Some(TransformSyntax::Matrix) | None => Self::parse_matrix(s),
        }
    }

    fn parse_matrix(s: &str) -> Result<Self, Warning> {
        let inner = if let Some(rest) = strip_prefix_ci(s, "matrix(") {
            rest.strip_suffix(')')
                .ok_or_else(|| Warning::new("unclosed matrix("))?
        } else {
            s
        };
        let nums: Result<Vec<f64>, _> = inner
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .map(|t| {
                t.parse::<f64>()
                    .map_err(|_| Warning::new(format!("invalid matrix number: {t}")))
            })
            .collect();
        Self::from_slice(&nums?)
    }

    fn parse_trs(s: &str) -> Result<Self, Warning> {
        let mut tx = 0.0;
        let mut ty = 0.0;
        let mut tz = 0.0;
        let mut rx = 0.0;
        let mut ry = 0.0;
        let mut rz = 0.0;
        let mut sx = 1.0;
        let mut sy = 1.0;
        let mut sz = 1.0;
        let mut saw_any = false;

        for (name, args) in iter_transform_funcs(s)? {
            let nums = parse_func_nums(&args, &name)?;
            match name.as_str() {
                "translate" => {
                    saw_any = true;
                    tx = *nums.first().unwrap_or(&0.0);
                    ty = *nums.get(1).unwrap_or(&0.0);
                    tz = *nums.get(2).unwrap_or(&0.0);
                }
                "rotate" => {
                    saw_any = true;
                    rx = *nums.first().unwrap_or(&0.0);
                    ry = *nums.get(1).unwrap_or(&0.0);
                    rz = *nums.get(2).unwrap_or(&0.0);
                }
                "scale" => {
                    saw_any = true;
                    match nums.as_slice() {
                        [] => {}
                        [u] => {
                            sx = *u;
                            sy = *u;
                            sz = *u;
                        }
                        [a, b] => {
                            sx = *a;
                            sy = *b;
                            sz = 1.0;
                        }
                        [a, b, c, ..] => {
                            sx = *a;
                            sy = *b;
                            sz = *c;
                        }
                    }
                }
                "matrix" => {
                    return Err(Warning::new(
                        "s3dx:transform: cannot mix matrix(...) with TRS functions",
                    ));
                }
                other => {
                    return Err(Warning::new(format!(
                        "s3dx:transform: unknown function '{other}'"
                    )));
                }
            }
        }

        if !saw_any {
            return Err(Warning::new("s3dx:transform: empty TRS value"));
        }
        Ok(Self::from_trs(tx, ty, tz, rx, ry, rz, sx, sy, sz))
    }

    pub fn from_value(v: &Value) -> Result<Self, Warning> {
        match v {
            Value::Vector(nums) => Self::from_slice(nums),
            Value::String(s) => Self::parse(s),
            _ => Err(Warning::new("transform must be a 12-number matrix or TRS")),
        }
    }

    /// Compose `self * other` (apply `other` first, then `self`).
    /// Column-vector convention: p' = M * p.
    pub fn mul(self, other: Self) -> Self {
        let a = &self.m;
        let b = &other.m;

        let c0x = a[0] * b[0] + a[3] * b[1] + a[6] * b[2];
        let c0y = a[1] * b[0] + a[4] * b[1] + a[7] * b[2];
        let c0z = a[2] * b[0] + a[5] * b[1] + a[8] * b[2];

        let c1x = a[0] * b[3] + a[3] * b[4] + a[6] * b[5];
        let c1y = a[1] * b[3] + a[4] * b[4] + a[7] * b[5];
        let c1z = a[2] * b[3] + a[5] * b[4] + a[8] * b[5];

        let c2x = a[0] * b[6] + a[3] * b[7] + a[6] * b[8];
        let c2y = a[1] * b[6] + a[4] * b[7] + a[7] * b[8];
        let c2z = a[2] * b[6] + a[5] * b[7] + a[8] * b[8];

        let ctx = a[0] * b[9] + a[3] * b[10] + a[6] * b[11] + a[9];
        let cty = a[1] * b[9] + a[4] * b[10] + a[7] * b[11] + a[10];
        let ctz = a[2] * b[9] + a[5] * b[10] + a[8] * b[11] + a[11];

        Self {
            m: [c0x, c0y, c0z, c1x, c1y, c1z, c2x, c2y, c2z, ctx, cty, ctz],
        }
    }

    /// Apply parent transform after local: world = parent * local.
    pub fn then(self, parent: Self) -> Self {
        parent.mul(self)
    }

    /// Transform a point: `p' = M * p` (column vector).
    pub fn transform_point(self, p: [f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [
            m[0] * p[0] + m[3] * p[1] + m[6] * p[2] + m[9],
            m[1] * p[0] + m[4] * p[1] + m[7] * p[2] + m[10],
            m[2] * p[0] + m[5] * p[1] + m[8] * p[2] + m[11],
        ]
    }

    /// Invert an affine transform. Returns `None` if the 3×3 linear part is singular.
    pub fn try_inverse(self) -> Option<Self> {
        let m = &self.m;
        let (a, b, c) = (m[0], m[1], m[2]);
        let (d, e, f) = (m[3], m[4], m[5]);
        let (g, h, i) = (m[6], m[7], m[8]);
        // det = c0 · (c1 × c2)
        let c1xc2 = [e * i - f * h, f * g - d * i, d * h - e * g];
        let det = a * c1xc2[0] + b * c1xc2[1] + c * c1xc2[2];
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let c2xc0 = [h * c - i * b, i * a - g * c, g * b - h * a];
        let c0xc1 = [b * f - c * e, c * d - a * f, a * e - b * d];
        // A^{-1} rows = (c1×c2, c2×c0, c0×c1) / det → store as columns
        let inv_m = [
            c1xc2[0] * inv,
            c2xc0[0] * inv,
            c0xc1[0] * inv,
            c1xc2[1] * inv,
            c2xc0[1] * inv,
            c0xc1[1] * inv,
            c1xc2[2] * inv,
            c2xc0[2] * inv,
            c0xc1[2] * inv,
        ];
        let (tx, ty, tz) = (m[9], m[10], m[11]);
        let itx = -(inv_m[0] * tx + inv_m[3] * ty + inv_m[6] * tz);
        let ity = -(inv_m[1] * tx + inv_m[4] * ty + inv_m[7] * tz);
        let itz = -(inv_m[2] * tx + inv_m[5] * ty + inv_m[8] * tz);
        Some(Self {
            m: [
                inv_m[0], inv_m[1], inv_m[2], inv_m[3], inv_m[4], inv_m[5], inv_m[6], inv_m[7],
                inv_m[8], itx, ity, itz,
            ],
        })
    }
}

fn rotation_xyz_deg(rx_deg: f64, ry_deg: f64, rz_deg: f64) -> Mat4x3 {
    let rx = rx_deg.to_radians();
    let ry = ry_deg.to_radians();
    let rz = rz_deg.to_radians();
    let (cx, sx) = (rx.cos(), rx.sin());
    let (cy, sy) = (ry.cos(), ry.sin());
    let (cz, sz) = (rz.cos(), rz.sin());

    // R = Rz * Ry * Rx (fixed-axis X, then Y, then Z; THREE Euler order 'XYZ').
    // rotate(90 90 0) = matrix(0 0 -1 1 0 0 0 -1 0 0 0 0); +X maps to -Z.
    let mx = Mat4x3 {
        m: [1.0, 0.0, 0.0, 0.0, cx, sx, 0.0, -sx, cx, 0.0, 0.0, 0.0],
    };
    let my = Mat4x3 {
        m: [cy, 0.0, -sy, 0.0, 1.0, 0.0, sy, 0.0, cy, 0.0, 0.0, 0.0],
    };
    let mz = Mat4x3 {
        m: [cz, sz, 0.0, -sz, cz, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
    };
    mz.mul(my.mul(mx))
}

fn looks_like_trs(lower: &str) -> bool {
    lower.contains("translate(") || lower.contains("rotate(") || lower.contains("scale(")
}

fn first_transform_keyword(lower: &str) -> Option<&'static str> {
    let candidates = [
        ("matrix(", "matrix"),
        ("translate(", "translate"),
        ("rotate(", "rotate"),
        ("scale(", "scale"),
    ];
    let mut best: Option<(usize, &'static str)> = None;
    for (pat, name) in candidates {
        if let Some(i) = lower.find(pat) {
            if best.map(|(bi, _)| i < bi).unwrap_or(true) {
                best = Some((i, name));
            }
        }
    }
    best.map(|(_, n)| n)
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Yield `(name_lowercase, args_inside_parens)` for each `name(...)` in `s`.
fn iter_transform_funcs(s: &str) -> Result<Vec<(String, String)>, Warning> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_alphabetic() || chars[i] == '_') {
            i += 1;
        }
        if i == start {
            return Err(Warning::new(format!(
                "s3dx:transform: unexpected character near '{}'",
                chars[start]
            )));
        }
        let name: String = chars[start..i].iter().collect::<String>().to_ascii_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '(' {
            return Err(Warning::new(format!(
                "s3dx:transform: expected '(' after '{name}'"
            )));
        }
        i += 1; // skip '('
        let args_start = i;
        let mut depth = 1i32;
        while i < chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let args: String = chars[args_start..i].iter().collect();
                        out.push((name.clone(), args));
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if depth != 0 {
            return Err(Warning::new(format!(
                "s3dx:transform: unclosed '(' in '{name}'"
            )));
        }
    }
    Ok(out)
}

fn parse_func_nums(args: &str, name: &str) -> Result<Vec<f64>, Warning> {
    let nums: Result<Vec<f64>, _> = args
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| {
            t.parse::<f64>()
                .map_err(|_| Warning::new(format!("invalid {name} number: {t}")))
        })
        .collect();
    let nums = nums?;
    if nums.len() > 3 {
        return Err(Warning::new(format!(
            "{name}(...) expects at most 3 numbers, got {}",
            nums.len()
        )));
    }
    Ok(nums)
}

impl fmt::Display for Mat4x3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "matrix({},{},{}, {},{},{}, {},{},{}, {},{},{})",
            self.m[0],
            self.m[1],
            self.m[2],
            self.m[3],
            self.m[4],
            self.m[5],
            self.m[6],
            self.m[7],
            self.m[8],
            self.m[9],
            self.m[10],
            self.m[11]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: &Mat4x3, b: &Mat4x3) {
        for i in 0..12 {
            assert!(
                (a.m[i] - b.m[i]).abs() < 1e-9,
                "m[{i}]: {} vs {}",
                a.m[i],
                b.m[i]
            );
        }
    }

    #[test]
    fn parse_identity() {
        let m = Mat4x3::parse("matrix(1,0,0, 0,1,0, 0,0,1, 0,0,0)").unwrap();
        assert_eq!(m, Mat4x3::identity());
    }

    #[test]
    fn compose_translation() {
        let a = Mat4x3::parse("matrix(1,0,0, 0,1,0, 0,0,1, 10,0,0)").unwrap();
        let b = Mat4x3::parse("matrix(1,0,0, 0,1,0, 0,0,1, 0,20,0)").unwrap();
        let c = a.mul(b);
        assert_eq!(c.m[9], 10.0);
        assert_eq!(c.m[10], 20.0);
        assert_eq!(c.m[11], 0.0);
    }

    #[test]
    fn parse_trs_translate_only() {
        let m = Mat4x3::parse("translate(10 20 30)").unwrap();
        let expect = Mat4x3::parse("matrix(1 0 0 0 1 0 0 0 1 10 20 30)").unwrap();
        approx_eq(&m, &expect);
    }

    #[test]
    fn rotate_xy_90_is_rz_ry_rx() {
        let m = Mat4x3::parse("rotate(90 90 0)").unwrap();
        let expect = Mat4x3::parse("matrix(0 0 -1 1 0 0 0 -1 0 0 0 0)").unwrap();
        approx_eq(&m, &expect);
        let p = m.transform_point([1.0, 0.0, 0.0]);
        assert!((p[0]).abs() < 1e-9);
        assert!((p[1]).abs() < 1e-9);
        assert!((p[2] + 1.0).abs() < 1e-9);
    }

    #[test]
    fn parse_trs_rotate_y_45() {
        let m = Mat4x3::parse("rotate(0 45 0)").unwrap();
        let from = Mat4x3::from_trs(0.0, 0.0, 0.0, 0.0, 45.0, 0.0, 1.0, 1.0, 1.0);
        approx_eq(&m, &from);
        // X axis maps toward +Z for +Y rotation (right-handed).
        let c = 45f64.to_radians().cos();
        let s = 45f64.to_radians().sin();
        assert!((m.m[0] - c).abs() < 1e-9);
        assert!((m.m[2] - (-s)).abs() < 1e-9);
        assert!((m.m[6] - s).abs() < 1e-9);
        assert!((m.m[8] - c).abs() < 1e-9);
    }

    #[test]
    fn parse_trs_full_order_independent() {
        let a = Mat4x3::parse("scale(2 3 4) translate(1 2 3) rotate(10 20 30)").unwrap();
        let b = Mat4x3::parse("translate(1 2 3) rotate(10 20 30) scale(2 3 4)").unwrap();
        approx_eq(&a, &b);
        let expect = Mat4x3::from_trs(1.0, 2.0, 3.0, 10.0, 20.0, 30.0, 2.0, 3.0, 4.0);
        approx_eq(&a, &expect);
    }

    #[test]
    fn detect_syntax() {
        assert_eq!(
            Mat4x3::detect_syntax("matrix(1 0 0 0 1 0 0 0 1 0 0 0)"),
            Some(TransformSyntax::Matrix)
        );
        assert_eq!(
            Mat4x3::detect_syntax("translate(1 0 0)"),
            Some(TransformSyntax::Trs)
        );
        assert_eq!(
            Mat4x3::detect_syntax("1 0 0 0 1 0 0 0 1 0 0 0"),
            Some(TransformSyntax::Matrix)
        );
        // Mixed → first keyword wins
        assert_eq!(
            Mat4x3::detect_syntax("translate(1 0 0) matrix(1 0 0 0 1 0 0 0 1 0 0 0)"),
            Some(TransformSyntax::Trs)
        );
    }
}
