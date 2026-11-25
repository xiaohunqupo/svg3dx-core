//! SMIL-shaped animation subset (draft 0.2 §10).
//!
//! Collects `<s3dx:animate>` / `<s3dx:animateTransform>` descriptors from the DOM.
//! Geometry evaluation skips these elements; the host clock samples and applies them.

use serde::{Deserialize, Serialize};

use crate::geometry::Mat4x3;
use crate::parser::{Document, Element, Namespace};

/// Timing / endpoint fields shared by both animation kinds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnimationTiming {
    pub from: String,
    pub to: String,
    pub dur: String,
    /// Play count as authored (`0` = indefinite). Omitted ⇒ once (`1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub begin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calc_mode: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnimateTransformType {
    Translate,
    Rotate,
    Scale,
}

impl AnimateTransformType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "translate" => Some(Self::Translate),
            "rotate" => Some(Self::Rotate),
            "scale" => Some(Self::Scale),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnimationDesc {
    /// Parameter timeline targeting a CSS custom property (`--name`).
    Param {
        attribute_name: String,
        /// Parent element `id` when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<String>,
        /// `svg` | `g` | …
        parent_tag: String,
        /// True when parent is the document root `<svg>` (document-root / `:root` scope).
        root_scope: bool,
        timing: AnimationTiming,
    },
    /// Transform overlay on a node that carries `s3dx:transform`.
    Transform {
        /// `attributeName`; semantic target is always `s3dx:transform`.
        attribute_name: String,
        transform_type: AnimateTransformType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_id: Option<String>,
        parent_tag: String,
        timing: AnimationTiming,
    },
}

/// Walk the document and collect animation elements (with validation warnings).
pub fn collect_animations(doc: &Document, warnings: &mut Vec<String>) -> Vec<AnimationDesc> {
    let mut out = Vec::new();
    collect_in_element(&doc.root, true, &mut out, warnings);
    filter_param_animations(doc, out, warnings)
}

fn collect_in_element(
    el: &Element,
    is_doc_root: bool,
    out: &mut Vec<AnimationDesc>,
    warnings: &mut Vec<String>,
) {
    for child in el.child_elements() {
        if child.ns == Namespace::S3dx && child.name == "animate" {
            if let Some(desc) = parse_animate(child, el, is_doc_root, warnings) {
                out.push(desc);
            }
            continue;
        }
        if child.ns == Namespace::S3dx && child.name == "animateTransform" {
            if let Some(desc) = parse_animate_transform(child, el, warnings) {
                out.push(desc);
            }
            continue;
        }
        collect_in_element(child, false, out, warnings);
    }
}

fn parse_timing(el: &Element, warnings: &mut Vec<String>) -> Option<AnimationTiming> {
    let from = el.attr("from")?.to_string();
    let to = el.attr("to")?.to_string();
    let dur = el.attr("dur")?.to_string();
    if parse_duration_secs(&dur).is_none() {
        warnings.push(format!("s3dx:{}: invalid dur '{dur}' — ignored", el.name));
        return None;
    }
    let begin = el.attr("begin").map(|s| s.to_string());
    if let Some(ref b) = begin {
        if parse_duration_secs(b).is_none() {
            warnings.push(format!("s3dx:{}: invalid begin '{b}' — ignored", el.name));
            return None;
        }
    }
    let calc_mode = el.attr("calcMode").map(|s| s.to_string());
    if let Some(ref cm) = calc_mode {
        if !is_calc_mode(cm) {
            warnings.push(format!(
                "s3dx:{}: calcMode '{cm}' unsupported — using linear",
                el.name
            ));
        }
    }
    let fill = el.attr("fill").map(|s| s.to_string());
    if let Some(ref f) = fill {
        if f != "freeze" && f != "remove" {
            warnings.push(format!(
                "s3dx:{}: fill '{f}' ignored (use freeze|remove)",
                el.name
            ));
        }
    }
    let repeat = el.attr("repeat").map(|s| s.to_string());
    if let Some(ref r) = repeat {
        if parse_repeat(r).is_none() {
            warnings.push(format!(
                "s3dx:{}: invalid repeat '{r}' — using 1",
                el.name
            ));
        }
    }
    Some(AnimationTiming {
        from,
        to,
        dur,
        repeat,
        fill,
        begin,
        calc_mode,
    })
}

/// `0` = indefinite; positive integer = play count. Other forms → `None`.
fn parse_repeat(s: &str) -> Option<(bool, f64)> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    // Reject signs / decimals / scientific — plain non-negative integer only.
    if !t.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u64 = t.parse().ok()?;
    if n == 0 {
        Some((true, f64::INFINITY))
    } else {
        Some((false, n as f64))
    }
}

fn parse_animate(
    el: &Element,
    parent: &Element,
    parent_is_doc_root: bool,
    warnings: &mut Vec<String>,
) -> Option<AnimationDesc> {
    let timing = parse_timing(el, warnings)?;
    let attribute_name = el.attr("attributeName")?.to_string();
    if !attribute_name.starts_with("--") {
        warnings.push(format!(
            "s3dx:animate: attributeName '{attribute_name}' must be a dashed-ident (--name) — ignored"
        ));
        return None;
    }

    let root_scope = parent_is_doc_root || (parent.ns == Namespace::Svg && parent.name == "svg");
    let parent_ok = root_scope || (parent.ns == Namespace::Svg && parent.name == "g");
    if !parent_ok {
        warnings.push(
            "s3dx:animate: parent must be a parameter-scope host (<svg> or <g>) — ignored".into(),
        );
        return None;
    }

    if !root_scope && !parent_declares_direct_custom_prop(parent, &attribute_name) {
        // Also accept class-rule declarations on the parent.
        let via_class = parent.classes().iter().any(|c| {
            // class_rules live on Document — checked in filter for root only.
            // For <g>, require inline direct literal per v1.
            let _ = c;
            false
        });
        if !via_class {
            warnings.push(format!(
                "s3dx:animate: '{attribute_name}' has no direct inline literal on parent <g> — ignored"
            ));
            return None;
        }
    }

    Some(AnimationDesc::Param {
        attribute_name,
        parent_id: parent.id().map(|s| s.to_string()),
        parent_tag: parent.name.clone(),
        root_scope,
        timing,
    })
}

fn parent_declares_direct_custom_prop(parent: &Element, dashed: &str) -> bool {
    if let Some(style) = parent.attr("style") {
        for decl in style.split(';') {
            let decl = decl.trim();
            if decl.is_empty() {
                continue;
            }
            if let Some((name, value)) = decl.split_once(':') {
                if name.trim() == dashed {
                    return crate::params::is_direct_param_value(value.trim());
                }
            }
        }
    }
    false
}

fn parse_animate_transform(
    el: &Element,
    parent: &Element,
    warnings: &mut Vec<String>,
) -> Option<AnimationDesc> {
    let timing = parse_timing(el, warnings)?;
    let attribute_name = el
        .attr("attributeName")
        .unwrap_or("transform")
        .to_string();
    if attribute_name != "transform" {
        warnings.push(format!(
            "s3dx:animateTransform: attributeName '{attribute_name}' must be 'transform' — ignored"
        ));
        return None;
    }
    let type_raw = el.attr("type")?;
    let Some(transform_type) = AnimateTransformType::parse(type_raw) else {
        warnings.push(format!(
            "s3dx:animateTransform: unknown type '{type_raw}' — ignored"
        ));
        return None;
    };

    let parent_ok =
        (parent.ns == Namespace::Svg && parent.name == "g") || parent.ns == Namespace::S3dx;
    if !parent_ok {
        warnings.push(
            "s3dx:animateTransform: parent must be a transform host (<g> or s3dx:*) — ignored"
                .into(),
        );
        return None;
    }

    if parse_triple(&timing.from).is_none() || parse_triple(&timing.to).is_none() {
        warnings.push("s3dx:animateTransform: from/to must be three numbers — ignored".into());
        return None;
    }

    Some(AnimationDesc::Transform {
        attribute_name,
        transform_type,
        parent_id: parent.id().map(|s| s.to_string()),
        parent_tag: parent.name.clone(),
        timing,
    })
}

/// Parse `2s` / `500ms` → seconds.
pub fn parse_duration_secs(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(rest) = s.strip_suffix("ms") {
        let n: f64 = rest.trim().parse().ok()?;
        if n >= 0.0 {
            return Some(n / 1000.0);
        }
        return None;
    }
    let rest = s.strip_suffix('s').unwrap_or(s);
    let n: f64 = rest.trim().parse().ok()?;
    if n >= 0.0 {
        Some(n)
    } else {
        None
    }
}

pub fn parse_triple(s: &str) -> Option<[f64; 3]> {
    let nums: Vec<f64> = s
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    if nums.len() == 3 {
        Some([nums[0], nums[1], nums[2]])
    } else {
        None
    }
}

/// Progress in \[0, 1\] within the active cycle, or `None` when not applying.
pub fn animation_progress(timing: &AnimationTiming, time_secs: f64) -> Option<f64> {
    let dur = parse_duration_secs(&timing.dur)?;
    let begin = timing
        .begin
        .as_deref()
        .and_then(parse_duration_secs)
        .unwrap_or(0.0);
    if time_secs < begin {
        return None;
    }
    if dur <= 0.0 {
        return Some(1.0);
    }
    let local = time_secs - begin;
    let (indefinite, count) = timing
        .repeat
        .as_deref()
        .and_then(parse_repeat)
        .unwrap_or((false, 1.0));
    let total = if indefinite {
        f64::INFINITY
    } else {
        dur * count
    };
    let fill_freeze = timing.fill.as_deref() == Some("freeze");

    if !indefinite && local >= total {
        return if fill_freeze { Some(1.0) } else { None };
    }
    let cycle = local % dur;
    Some((cycle / dur).clamp(0.0, 1.0))
}

/// Approach-law weight \(s = f(t)\) — same set as loft `blend` (§3.2 / §10.1).
fn calc_mode_weight(mode: &str, t: f64) -> f64 {
    let x = t.clamp(0.0, 1.0);
    match mode {
        "sine" => (std::f64::consts::PI * x / 2.0).sin(),
        "cosine" => 0.5 - 0.5 * (std::f64::consts::PI * x).cos(),
        "tan" => (std::f64::consts::PI * x / 4.0).tan(),
        "cotan" => 1.0 - (std::f64::consts::PI * (1.0 - x) / 4.0).tan(),
        _ => x, // linear + unknown
    }
}

fn is_calc_mode(mode: &str) -> bool {
    matches!(mode, "linear" | "sine" | "cosine" | "tan" | "cotan")
}

fn eased_progress(timing: &AnimationTiming, time_secs: f64) -> Option<f64> {
    let t = animation_progress(timing, time_secs)?;
    let mode = timing.calc_mode.as_deref().unwrap_or("linear");
    Some(calc_mode_weight(mode, t))
}

/// Sample a scalar `from`→`to` at document time. `None` = remove override.
pub fn sample_scalar(timing: &AnimationTiming, time_secs: f64) -> Option<f64> {
    let s = eased_progress(timing, time_secs)?;
    let a: f64 = timing.from.trim().parse().ok()?;
    let b: f64 = timing.to.trim().parse().ok()?;
    Some(a + (b - a) * s)
}

/// Sample a TRS triple animation → matrix `M_anim(t)`.
pub fn sample_animate_transform(
    transform_type: AnimateTransformType,
    timing: &AnimationTiming,
    time_secs: f64,
) -> Option<Mat4x3> {
    let s = eased_progress(timing, time_secs)?;
    let a = parse_triple(&timing.from)?;
    let b = parse_triple(&timing.to)?;
    let v = [
        a[0] + (b[0] - a[0]) * s,
        a[1] + (b[1] - a[1]) * s,
        a[2] + (b[2] - a[2]) * s,
    ];
    Some(match transform_type {
        AnimateTransformType::Translate => {
            Mat4x3::from_trs(v[0], v[1], v[2], 0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
        }
        AnimateTransformType::Rotate => {
            Mat4x3::from_trs(0.0, 0.0, 0.0, v[0], v[1], v[2], 1.0, 1.0, 1.0)
        }
        AnimateTransformType::Scale => {
            Mat4x3::from_trs(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, v[0], v[1], v[2])
        }
    })
}

fn filter_param_animations(
    doc: &Document,
    anims: Vec<AnimationDesc>,
    warnings: &mut Vec<String>,
) -> Vec<AnimationDesc> {
    let root_names = crate::params::externalizable_root_names(doc);
    anims
        .into_iter()
        .filter(|a| match a {
            AnimationDesc::Param {
                attribute_name,
                root_scope,
                ..
            } => {
                if !*root_scope {
                    true
                } else {
                    let bare = attribute_name.trim_start_matches('-');
                    if root_names.iter().any(|n| n == bare) {
                        true
                    } else {
                        warnings.push(format!(
                            "s3dx:animate: '{attribute_name}' is not a direct :root literal — ignored"
                        ));
                        false
                    }
                }
            }
            AnimationDesc::Transform { .. } => true,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parsing() {
        assert_eq!(parse_duration_secs("2s"), Some(2.0));
        assert_eq!(parse_duration_secs("500ms"), Some(0.5));
        assert_eq!(parse_duration_secs("0"), Some(0.0));
        assert!(parse_duration_secs("-1s").is_none());
    }

    #[test]
    fn progress_indefinite() {
        let timing = AnimationTiming {
            from: "0".into(),
            to: "10".into(),
            dur: "2s".into(),
            repeat: Some("0".into()),
            fill: None,
            begin: None,
            calc_mode: None,
        };
        let p = animation_progress(&timing, 1.0).unwrap();
        assert!((p - 0.5).abs() < 1e-9);
        let s = sample_scalar(&timing, 1.0).unwrap();
        assert!((s - 5.0).abs() < 1e-9);
    }

    #[test]
    fn parse_repeat_values() {
        let zero = parse_repeat("0").unwrap();
        assert!(zero.0 && zero.1.is_infinite());
        assert_eq!(parse_repeat("1"), Some((false, 1.0)));
        assert_eq!(parse_repeat("3"), Some((false, 3.0)));
        assert!(parse_repeat("indefinite").is_none());
        assert!(parse_repeat("-1").is_none());
        assert!(parse_repeat("1.5").is_none());
    }
}
