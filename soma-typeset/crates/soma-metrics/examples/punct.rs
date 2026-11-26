//! Measure the side bearings that the trim constants stand in for.
//!
//! ```text
//! cargo run -p soma-metrics --example punct -- path/to/table.smt
//! ```
//!
//! The rules layer trims an opening or closing mark by a fixed fraction of an
//! em when a line ends next to it. When the metrics table carries outline ink
//! (ADR-0037) the renderer measures the real gap instead; these constants are
//! the fallback for faces that do not. This tool reads the ink the table
//! already stores and prints the median per punctuation class, which is what
//! the fallback should be.
//!
//! Output is one line per class: the count, the median ink gap in 1/4096 em,
//! and the same value as a fraction of a full-width em.

use soma_core::PunctClass;
use soma_metrics::{Face, MetricsTable};
use soma_rules::punct_class;

/// One full-width em in the table's units.
const EM: i32 = 4096;

#[derive(Default)]
struct Buckets {
    open_left: Vec<i32>,
    close_right: Vec<i32>,
    middle_left: Vec<i32>,
    middle_right: Vec<i32>,
    narrow_left: Vec<i32>,
    narrow_right: Vec<i32>,
}

fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| String::from("usage: punct <table.smt>"))?;
    let bytes = std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    let table = MetricsTable::parse(&bytes).map_err(|error| format!("{path}: {error:?}"))?;

    let mut buckets = Buckets::default();
    for face in table.faces() {
        collect(face, &mut buckets);
    }

    println!("table,{path}");
    report("open.left", &mut buckets.open_left);
    report("close.right", &mut buckets.close_right);
    report("middle.left", &mut buckets.middle_left);
    report("middle.right", &mut buckets.middle_right);
    report("halfwidth.left", &mut buckets.narrow_left);
    report("halfwidth.right", &mut buckets.narrow_right);
    Ok(())
}

fn collect(face: &Face, buckets: &mut Buckets) {
    for codepoint in 0x20..=0x10FFFFu32 {
        let Some(character) = char::from_u32(codepoint) else {
            continue;
        };
        let Some((left, right)) = face.ink(codepoint) else {
            continue;
        };
        let full_width = face.advance_at(codepoint) >= EM * 9 / 10;
        match punct_class(character) {
            PunctClass::Open if full_width => buckets.open_left.push(left),
            PunctClass::Close if full_width => buckets.close_right.push(right),
            PunctClass::Middle if full_width => {
                buckets.middle_left.push(left);
                buckets.middle_right.push(right);
            }
            PunctClass::Open | PunctClass::Close | PunctClass::Middle => {
                buckets.narrow_left.push(left);
                buckets.narrow_right.push(right);
            }
            // A wide mark (dash, ellipsis) keeps its side space; measuring it
            // would only invite a trim that closes the gap.
            PunctClass::Dash | PunctClass::None => {}
        }
    }
}

fn report(name: &str, values: &mut [i32]) {
    if values.is_empty() {
        println!("{name},0,0,0.000");
        return;
    }
    values.sort_unstable();
    let median = values[values.len() / 2];
    let ratio = f64::from(median) / f64::from(EM);
    println!("{name},{},{median},{ratio:.3}", values.len());
}
