//! `soma.engine/1`: engine configuration.
//!
//! Architecture v2 §9 defines a deliberately thin configuration layer: a
//! preset id, a search window, the atom window, fallback switches, and the
//! vertical cost model. It is **not** a second template DSL and it cannot
//! express presentation. The document shape is fixed:
//!
//! ```yaml
//! schema: soma.engine/1
//! preset: default-v1
//! window: paragraph
//! max_kp_atoms: null
//! fallback:
//!   allow_emergency: true
//!   allow_forbidden: true
//!   allow_overfull: true
//!   emergency_stretch: 3em
//! vertical:
//!   page_weight: 0
//!   badness_scale: 100
//!   overflow_penalty: 1000000
//!   club_penalty: 10000
//!   widow_penalty: 10000
//! ```
//!
//! Because the shape is fixed, this crate carries a small, strict parser rather
//! than the template YAML subset: two levels, no sequences, no flow
//! collections. Unknown keys are errors. All values are integers or booleans;
//! lengths are integer `em`.
//!
//! `preset: default-v1` is the only preset. Changing a default requires a new
//! preset id (architecture §9), so this crate exposes only the overrides the
//! architecture allows.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::{self, Write};

use soma_core::{BreakConfig, ParamSet, VerticalParams, Width, Window};

/// Canonical schema id.
pub const SCHEMA: &str = "soma.engine/1";

/// The preset the engine ships. `default-v1` stays selectable for frozen
/// comparisons.
pub const DEFAULT_PRESET: &str = "publish-v1";

/// A validated engine configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    params: ParamSet,
    vertical: VerticalParams,
    hash: u64,
}

impl EngineConfig {
    /// Parse and validate a `soma.engine/1` document.
    pub fn parse(source: &str) -> Result<Self, EngineError> {
        let mut schema_seen = false;
        let mut preset: Option<String> = None;
        let mut window: Option<String> = None;
        let mut max_atoms: Option<u32> = None;
        let mut fallback = FallbackInput::default();
        let mut vertical = VerticalInput::default();
        let mut section = Section::Top;
        let mut seen: Vec<&'static str> = Vec::new();

        for (index, raw) in source.lines().enumerate() {
            let line = index as u32 + 1;
            let text = strip_comment(raw);
            if text.trim().is_empty() {
                continue;
            }
            let indent = indentation(&text, line)?;
            let (key, value) = split_pair(&text, line)?;

            if indent == 0 {
                section = Section::Top;
                let Some(value) = value else {
                    section = match key.as_str() {
                        "fallback" => {
                            mark(&mut seen, "fallback", line)?;
                            Section::Fallback
                        }
                        "vertical" => {
                            mark(&mut seen, "vertical", line)?;
                            Section::Vertical
                        }
                        _ => return Err(EngineError::new(line, 1, "", "unknown section")),
                    };
                    continue;
                };

                match key.as_str() {
                    "schema" => {
                        mark(&mut seen, "schema", line)?;
                        schema_seen = true;
                        let text = unquote(&value, line, "schema")?;
                        if text != SCHEMA {
                            return Err(EngineError::new(
                                line,
                                1,
                                "schema",
                                "expected soma.engine/1",
                            ));
                        }
                    }
                    "preset" => {
                        mark(&mut seen, "preset", line)?;
                        preset = Some(unquote(&value, line, "preset")?);
                    }
                    "window" => {
                        mark(&mut seen, "window", line)?;
                        window = Some(unquote(&value, line, "window")?);
                    }
                    "max_kp_atoms" => {
                        mark(&mut seen, "max_kp_atoms", line)?;
                        max_atoms = parse_optional_atoms(&value, line)?;
                    }
                    _ => return Err(EngineError::new(line, 1, "", "unknown key")),
                }
                continue;
            }

            let value = value.ok_or_else(|| EngineError::new(line, 1, "", "expected a value"))?;

            match section {
                Section::Top => {
                    return Err(EngineError::new(line, 1, "", "unexpected indentation"));
                }
                Section::Fallback => match key.as_str() {
                    "allow_emergency" => {
                        mark(&mut seen, "fallback.allow_emergency", line)?;
                        fallback.allow_emergency =
                            Some(parse_bool(&value, line, "allow_emergency")?);
                    }
                    "allow_forbidden" => {
                        mark(&mut seen, "fallback.allow_forbidden", line)?;
                        fallback.allow_forbidden =
                            Some(parse_bool(&value, line, "allow_forbidden")?);
                    }
                    "allow_overfull" => {
                        mark(&mut seen, "fallback.allow_overfull", line)?;
                        fallback.allow_overfull = Some(parse_bool(&value, line, "allow_overfull")?);
                    }
                    "emergency_stretch" => {
                        mark(&mut seen, "fallback.emergency_stretch", line)?;
                        fallback.emergency_stretch =
                            Some(parse_em(&value, line, "emergency_stretch")?);
                    }
                    _ => return Err(EngineError::new(line, 1, "fallback", "unknown key")),
                },
                Section::Vertical => match key.as_str() {
                    "page_weight" => {
                        mark(&mut seen, "vertical.page_weight", line)?;
                        vertical.page_weight = Some(parse_penalty(&value, line, "page_weight")?);
                    }
                    "badness_scale" => {
                        mark(&mut seen, "vertical.badness_scale", line)?;
                        vertical.badness_scale =
                            Some(parse_penalty(&value, line, "badness_scale")?);
                    }
                    "overflow_penalty" => {
                        mark(&mut seen, "vertical.overflow_penalty", line)?;
                        vertical.overflow_penalty =
                            Some(parse_penalty(&value, line, "overflow_penalty")?);
                    }
                    "club_penalty" => {
                        mark(&mut seen, "vertical.club_penalty", line)?;
                        vertical.club_penalty = Some(parse_penalty(&value, line, "club_penalty")?);
                    }
                    "widow_penalty" => {
                        mark(&mut seen, "vertical.widow_penalty", line)?;
                        vertical.widow_penalty =
                            Some(parse_penalty(&value, line, "widow_penalty")?);
                    }
                    _ => return Err(EngineError::new(line, 1, "vertical", "unknown key")),
                },
            }
        }

        if !schema_seen {
            return Err(EngineError::new(1, 1, "schema", "missing schema"));
        }

        let preset = preset.unwrap_or_else(|| DEFAULT_PRESET.to_string());
        let mut params = match preset.as_str() {
            "default-v1" => ParamSet::default_v1(),
            "publish-v1" => ParamSet::publish_v1(),
            "rhythm-v1" => ParamSet::rhythm_v1(),
            _ => return Err(EngineError::new(1, 1, "preset", "unknown preset")),
        };
        match window.as_deref() {
            None | Some("paragraph") => {}
            Some(_) => {
                return Err(EngineError::new(
                    1,
                    1,
                    "window",
                    "only the paragraph window is implemented",
                ));
            }
        }

        params.break_params.max_kp_atoms = max_atoms;
        let fallback_params = &mut params.break_params.fallback;
        if let Some(value) = fallback.allow_emergency {
            fallback_params.allow_emergency = value;
        }
        if let Some(value) = fallback.allow_forbidden {
            fallback_params.allow_forbidden = value;
        }
        if let Some(value) = fallback.allow_overfull {
            fallback_params.allow_overfull = value;
        }
        if let Some(value) = fallback.emergency_stretch {
            fallback_params.emergency_stretch = value;
        }
        params
            .validate()
            .map_err(|error| EngineError::new(1, 1, error.path, error.reason))?;

        let base = VerticalParams::default_v1();
        let vertical = VerticalParams {
            page_weight: vertical.page_weight.unwrap_or(base.page_weight as u64) as i64,
            badness_scale: vertical.badness_scale.unwrap_or(base.badness_scale as u64) as i64,
            overflow_penalty: vertical
                .overflow_penalty
                .unwrap_or(base.overflow_penalty as u64) as i64,
            club_penalty: vertical.club_penalty.unwrap_or(base.club_penalty as u64) as i64,
            widow_penalty: vertical.widow_penalty.unwrap_or(base.widow_penalty as u64) as i64,
        };
        if vertical.badness_scale > 1_000_000 {
            return Err(EngineError::new(
                1,
                1,
                "vertical.badness_scale",
                "must be <= 1000000",
            ));
        }
        if vertical.overflow_penalty > 1_000_000_000_000 {
            return Err(EngineError::new(
                1,
                1,
                "vertical.overflow_penalty",
                "must be <= 1000000000000",
            ));
        }

        let mut config = Self {
            params,
            vertical,
            hash: 0,
        };
        config.hash = fnv(config.canonical_json().as_bytes());
        Ok(config)
    }

    /// Break configuration with the preset values and the document's overrides.
    pub fn break_config(&self) -> BreakConfig {
        BreakConfig::new(self.params, Window::Paragraph)
    }

    pub fn vertical(&self) -> VerticalParams {
        self.vertical
    }

    pub fn preset(&self) -> &'static str {
        self.params.preset.0
    }

    pub fn window(&self) -> Window {
        Window::Paragraph
    }

    /// Content hash of the canonical configuration.
    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub fn canonical_json(&self) -> String {
        let params = &self.params.break_params;
        let fallback = &params.fallback;
        let vertical = self.vertical;
        let mut out = String::new();
        write!(
            out,
            "{{\"schema\":\"{}\",\"preset\":\"{}\",\"window\":\"paragraph\",\"maxKpAtoms\":",
            SCHEMA, self.params.preset.0
        )
        .expect("writing into a String cannot fail");
        match params.max_kp_atoms {
            Some(value) => write!(out, "{value}").expect("write"),
            None => out.push_str("null"),
        }
        write!(
            out,
            ",\"fallback\":{{\"allowEmergency\":{},\"allowForbidden\":{},\"allowOverfull\":{},\"emergencyStretch\":{}}}",
            fallback.allow_emergency,
            fallback.allow_forbidden,
            fallback.allow_overfull,
            fallback.emergency_stretch.0
        )
        .expect("write");
        write!(
            out,
            ",\"vertical\":{{\"pageWeight\":{},\"badnessScale\":{},\"overflowPenalty\":{},\"clubPenalty\":{},\"widowPenalty\":{}}}}}",
            vertical.page_weight,
            vertical.badness_scale,
            vertical.overflow_penalty,
            vertical.club_penalty,
            vertical.widow_penalty
        )
        .expect("write");
        out
    }
}

/// One parse or validation failure with a source position when available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineError {
    pub line: u32,
    pub column: u32,
    pub path: &'static str,
    pub reason: &'static str,
}

impl EngineError {
    const fn new(line: u32, column: u32, path: &'static str, reason: &'static str) -> Self {
        Self {
            line,
            column,
            path,
            reason,
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}:{}: {}", self.line, self.column, self.reason)
        } else {
            write!(
                f,
                "{}:{}: {}: {}",
                self.line, self.column, self.path, self.reason
            )
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Top,
    Fallback,
    Vertical,
}

#[derive(Clone, Copy, Default)]
struct FallbackInput {
    allow_emergency: Option<bool>,
    allow_forbidden: Option<bool>,
    allow_overfull: Option<bool>,
    emergency_stretch: Option<Width>,
}

#[derive(Clone, Copy, Default)]
struct VerticalInput {
    page_weight: Option<u64>,
    badness_scale: Option<u64>,
    overflow_penalty: Option<u64>,
    club_penalty: Option<u64>,
    widow_penalty: Option<u64>,
}

fn mark(seen: &mut Vec<&'static str>, path: &'static str, line: u32) -> Result<(), EngineError> {
    if seen.contains(&path) {
        return Err(EngineError::new(line, 1, path, "duplicate key"));
    }
    seen.push(path);
    Ok(())
}

fn indentation(line: &str, number: u32) -> Result<usize, EngineError> {
    let mut count = 0;
    for character in line.chars() {
        match character {
            ' ' => count += 1,
            '\t' => {
                return Err(EngineError::new(
                    number,
                    count as u32 + 1,
                    "",
                    "tabs are not allowed for indentation",
                ));
            }
            _ => break,
        }
    }
    Ok(count)
}

fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quote: Option<char> = None;
    for character in line.chars() {
        match quote {
            Some(open) => {
                out.push(character);
                if character == open {
                    quote = None;
                }
            }
            None => {
                if character == '#' {
                    break;
                }
                if character == '\'' || character == '"' {
                    quote = Some(character);
                }
                out.push(character);
            }
        }
    }
    out
}

fn split_pair(line: &str, number: u32) -> Result<(String, Option<String>), EngineError> {
    let mut quote: Option<char> = None;
    for (offset, character) in line.char_indices() {
        match quote {
            Some(open) => {
                if character == open {
                    quote = None;
                }
            }
            None => match character {
                '\'' | '"' => quote = Some(character),
                ':' => {
                    let key = line[..offset].trim();
                    if key.is_empty() {
                        return Err(EngineError::new(number, 1, "", "empty key"));
                    }
                    let value = line[offset + 1..].trim();
                    return Ok((
                        key.to_string(),
                        if value.is_empty() {
                            None
                        } else {
                            Some(value.to_string())
                        },
                    ));
                }
                _ => {}
            },
        }
    }
    Err(EngineError::new(number, 1, "", "expected key: value"))
}

fn unquote(text: &str, number: u32, path: &'static str) -> Result<String, EngineError> {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        let inner = &text[1..text.len() - 1];
        return Ok(inner.replace("''", "'"));
    }
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = &text[1..text.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut characters = inner.chars();
        while let Some(character) = characters.next() {
            if character != '\\' {
                out.push(character);
                continue;
            }
            match characters.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                _ => {
                    return Err(EngineError::new(
                        number,
                        1,
                        path,
                        "unsupported escape in a quoted value",
                    ));
                }
            }
        }
        return Ok(out);
    }
    if text.starts_with('\'') || text.starts_with('"') {
        return Err(EngineError::new(
            number,
            1,
            path,
            "unterminated quoted value",
        ));
    }
    Ok(text.to_string())
}

fn parse_bool(text: &str, number: u32, path: &'static str) -> Result<bool, EngineError> {
    match unquote(text, number, path)?.as_str() {
        "true" | "yes" => Ok(true),
        "false" | "no" => Ok(false),
        _ => Err(EngineError::new(number, 1, path, "expected true or false")),
    }
}

fn parse_optional_atoms(text: &str, number: u32) -> Result<Option<u32>, EngineError> {
    let value = unquote(text, number, "max_kp_atoms")?;
    if value == "null" {
        return Ok(None);
    }
    let atoms: u32 = value
        .parse()
        .map_err(|_| EngineError::new(number, 1, "max_kp_atoms", "expected an integer or null"))?;
    if atoms == 0 || atoms > 10_000 {
        return Err(EngineError::new(
            number,
            1,
            "max_kp_atoms",
            "must be in 1..=10000 or null",
        ));
    }
    Ok(Some(atoms))
}

fn parse_em(text: &str, number: u32, path: &'static str) -> Result<Width, EngineError> {
    let value = unquote(text, number, path)?;
    let digits = value
        .strip_suffix("em")
        .ok_or_else(|| EngineError::new(number, 1, path, "expected an em length"))?;
    let em: i64 = digits
        .trim()
        .parse()
        .map_err(|_| EngineError::new(number, 1, path, "expected an em length"))?;
    if !(0..=10).contains(&em) {
        return Err(EngineError::new(number, 1, path, "must be in 0..=10em"));
    }
    Ok(Width::from_em(em as i32))
}

fn parse_penalty(text: &str, number: u32, path: &'static str) -> Result<u64, EngineError> {
    let value = unquote(text, number, path)?;
    let number_value: i64 = value
        .parse()
        .map_err(|_| EngineError::new(number, 1, path, "expected a non-negative integer"))?;
    if number_value < 0 {
        return Err(EngineError::new(
            number,
            1,
            path,
            "expected a non-negative integer",
        ));
    }
    Ok(number_value as u64)
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut state: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        state ^= *byte as u64;
        state = state.wrapping_mul(0x0000_0100_0000_01b3);
    }
    state
}
