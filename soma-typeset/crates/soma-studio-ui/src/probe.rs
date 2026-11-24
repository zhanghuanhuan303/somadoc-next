//! Browser-side font probe (D101).
//!
//! The engine lays out from the `.smt` table, but the browser draws with a
//! font file. If the installed family is missing or substituted, the ink can
//! differ by a percent or two without moving a single breakpoint. This module
//! measures the same characters the engine sampled and reports the p95
//! deviation in 1/4096 em. It is a diagnostic: nothing here feeds the breaker
//! or the renderer.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::state::Studio;

/// The probe's verdict, shown in the title bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeReport {
    /// p95 absolute deviation in per-mille of the expected advance.
    pub p95: i32,
    pub samples: usize,
    pub missing: bool,
    pub label: &'static str,
    /// `ok`, `warn` or `bad`, used as a CSS class.
    pub class: &'static str,
    pub detail: String,
}

impl ProbeReport {
    fn new(p95: i32, samples: usize, missing: bool) -> Self {
        let (label, class) = if missing || p95 > 20 {
            ("不一致", "bad")
        } else if p95 >= 5 {
            ("接近", "warn")
        } else {
            ("字体一致", "ok")
        };
        let mut detail = format!("p95 {p95}‰ · {samples} 个样本");
        if missing {
            detail.push_str(" · 缺少模板字体");
        }
        detail.push_str("（诊断，不参与断行）");
        Self {
            p95,
            samples,
            missing,
            label,
            class,
            detail,
        }
    }
}

/// Await `document.fonts.ready`, then measure the engine's probe samples with
/// the family the template installed. Returns `None` when there is no build or
/// no probe data.
pub async fn measure(studio: Studio) -> Option<ProbeReport> {
    let (samples, fonts) = studio.build.with(|build| {
        let samples = build
            .as_ref()
            .map(|build| build.build.probe.clone())
            .unwrap_or_default();
        let fonts = studio.project.with(|project| {
            project
                .as_ref()
                .map(|project| project.fonts.clone())
                .unwrap_or_default()
        });
        (samples, fonts)
    });
    if samples.is_empty() {
        return None;
    }

    let document = web_sys::window()?.document()?;
    let font_set = document.fonts();
    if let Ok(ready) = font_set.ready() {
        let _ = JsFuture::from(ready).await;
    }

    let canvas = document
        .create_element("canvas")
        .ok()?
        .dyn_into::<HtmlCanvasElement>()
        .ok()?;
    let context = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()?;

    let mut deviations = Vec::new();
    let mut missing = false;
    for sample in &samples {
        let Some(family) = family_for(&fonts, &sample.role, sample.bold) else {
            missing = true;
            continue;
        };
        let spec = format!("100px {}", css_string(family));
        if !font_set.check(&spec).unwrap_or(false) {
            missing = true;
            continue;
        }
        context.set_font(&spec);
        let measured = context
            .measure_text(&sample.text)
            .map(|text| text.width())
            .unwrap_or(0.0);
        if !(measured.is_finite() && measured > 0.0) || sample.advance <= 0 {
            missing = true;
            continue;
        }
        let per_em = measured / 100.0 * 4096.0;
        let deviation =
            ((per_em - sample.advance as f64).abs() / sample.advance as f64 * 1000.0).round();
        deviations.push(deviation.min(100_000.0) as i32);
    }
    if deviations.is_empty() {
        return Some(ProbeReport::new(0, samples.len(), true));
    }
    deviations.sort_unstable();
    let rank = ((deviations.len() as f64) * 0.95).ceil() as usize;
    let p95 = deviations[rank.saturating_sub(1).min(deviations.len() - 1)];
    Some(ProbeReport::new(p95, deviations.len(), missing))
}

/// The CSS family for a metrics role. A missing `fallback` uses the body
/// family: the browser will substitute a system face, which is exactly the
/// condition the probe is meant to reveal.
fn family_for<'a>(
    fonts: &'a [soma_studio_core::FontInfo],
    role: &str,
    bold: bool,
) -> Option<&'a str> {
    let role_name = if bold {
        format!("{role}-bold")
    } else {
        role.to_string()
    };
    let pick = |name: &str| {
        fonts
            .iter()
            .find(|font| font.role == name)
            .map(|font| font.family.as_str())
    };
    pick(&role_name)
        .or_else(|| pick(role))
        .or_else(|| (role == "fallback").then(|| pick("serif")).flatten())
}

fn css_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
