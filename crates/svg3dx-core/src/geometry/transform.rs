use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::Warning;
use crate::params::types::Value;

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

    /// Parse `matrix(a,b,c, d,e,f, g,h,i, tx,ty,tz)` or a bare 12-number list.
    /// Embedded expressions should already be expanded into numbers.
    pub fn parse(raw: &str) -> Result<Self, Warning> {
        let s = raw.trim();
        let inner = if let Some(rest) = s.strip_prefix("matrix(") {
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

    pub fn from_value(v: &Value) -> Result<Self, Warning> {
        match v {
            Value::Vector(nums) => Self::from_slice(nums),
            Value::String(s) => Self::parse(s),
            _ => Err(Warning::new("transform must be a 12-number matrix")),
        }
    }

    /// Compose `self * other` (apply `other` first, then `self`).
    /// Column-vector convention: p' = M * p.
    pub fn mul(self, other: Self) -> Self {
        let a = &self.m;
        let b = &other.m;
        // 3x3 blocks
        let r00 = a[0] * b[0] + a[3] * b[1] + a[6] * b[2];
        let r01 = a[1] * b[0] + a[4] * b[1] + a[7] * b[2];
        let r02 = a[2] * b[0] + a[5] * b[1] + a[8] * b[2];

        let r10 = a[0] * b[3] + a[3] * b[4] + a[6] * b[5];
        let r11 = a[1] * b[3] + a[4] * b[4] + a[7] * b[5];
        let r12 = a[2] * b[3] + a[5] * b[4] + a[8] * b[5];

        let r20 = a[0] * b[6] + a[3] * b[7] + a[6] * b[8];
        let r21 = a[1] * b[6] + a[4] * b[7] + a[7] * b[8];
        let r22 = a[2] * b[6] + a[5] * b[7] + a[8] * b[8];

        let tx = a[0] * b[9] + a[3] * b[10] + a[6] * b[11] + a[9];
        let ty = a[1] * b[9] + a[4] * b[10] + a[7] * b[11] + a[10];
        let tz = a[2] * b[9] + a[5] * b[10] + a[8] * b[11] + a[11];

        // Store as row-major of columns? Spec uses matrix(a,b,c, d,e,f, g,h,i, tx,ty,tz)
        // where (a,b,c) is first column of 3x3, (d,e,f) second, (g,h,i) third.
        // Our indexing: m[0]=a, m[1]=b, m[2]=c, m[3]=d, ...
        // Wait - I used a[0],a[3],a[6] as a row which assumes row-major storage of
        // [[a,d,g],[b,e,h],[c,f,i]]. Let's verify:
        // Column 0 = (a,b,c) = (m[0],m[1],m[2])
        // Column 1 = (d,e,f) = (m[3],m[4],m[5])
        // Column 2 = (g,h,i) = (m[6],m[7],m[8])
        // So matrix times vector: x' = a*x + d*y + g*z + tx
        // Which is m[0]*x + m[3]*y + m[6]*z + m[9]
        //
        // For mul M*N: result column j = M * (N column j)
        // Actually my formula mixed things. Let me recompute properly.

        // Recompute with correct column-vector convention:
        let _ = (r00, r01, r02, r10, r11, r12, r20, r21, r22, tx, ty, tz);

        // N columns:
        // n0=(b0,b1,b2), n1=(b3,b4,b5), n2=(b6,b7,b8), nt=(b9,b10,b11)
        // M * n0:
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

    #[test]
    fn parse_identity() {
        let m = Mat4x3::parse("matrix(1,0,0, 0,1,0, 0,0,1, 0,0,0)").unwrap();
        assert_eq!(m, Mat4x3::identity());
    }

    #[test]
    fn compose_translation() {
        let a = Mat4x3::parse("matrix(1,0,0, 0,1,0, 0,0,1, 10,0,0)").unwrap();
        let b = Mat4x3::parse("matrix(1,0,0, 0,1,0, 0,0,1, 0,20,0)").unwrap();
        // apply b first then a: (0,20,0) then + (10,0,0) = (10,20,0)
        let c = a.mul(b);
        assert_eq!(c.m[9], 10.0);
        assert_eq!(c.m[10], 20.0);
        assert_eq!(c.m[11], 0.0);
    }
}
