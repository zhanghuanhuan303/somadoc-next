//! `soma-demo` CLI: render a paragraph to a self-contained HTML page.
//!
//! ```text
//! soma-demo [input.txt] [--width-em 20]
//! ```

use std::process::ExitCode;

use soma_core::Width;
use soma_demo::{blind_sheet, render_html, SAMPLE_TEXT};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input_path: Option<&str> = None;
    let mut width_em = 20i32;
    let mut blind_samples: Option<&str> = None;
    let mut out_dir: Option<&str> = None;
    let mut seed: u64 = 20_261_120;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--width-em" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --width-em");
                    return ExitCode::FAILURE;
                };
                match value.parse::<i32>() {
                    Ok(value) if value > 0 => width_em = value,
                    _ => {
                        eprintln!("invalid --width-em value");
                        return ExitCode::FAILURE;
                    }
                }
                index += 2;
            }
            "--blind-kit" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --blind-kit");
                    return ExitCode::FAILURE;
                };
                blind_samples = Some(value);
                index += 2;
            }
            "--out" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --out");
                    return ExitCode::FAILURE;
                };
                out_dir = Some(value);
                index += 2;
            }
            "--seed" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --seed");
                    return ExitCode::FAILURE;
                };
                match value.parse::<u64>() {
                    Ok(value) => seed = value,
                    Err(_) => {
                        eprintln!("invalid --seed value");
                        return ExitCode::FAILURE;
                    }
                }
                index += 2;
            }
            "--help" | "-h" => {
                println!(
                    "usage: soma-demo [input.txt] [--width-em 20]\n       soma-demo --blind-kit samples.txt [--out DIR] [--seed N] [--width-em 40]"
                );
                return ExitCode::SUCCESS;
            }
            other if !other.starts_with('-') => {
                input_path = Some(other);
                index += 1;
            }
            other => {
                eprintln!("unknown option: {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let text = match input_path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("cannot read {path}: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => SAMPLE_TEXT.to_string(),
    };

    if let Some(samples_path) = blind_samples {
        return match write_blind_kit(samples_path, out_dir.unwrap_or("blind-kit"), width_em, seed) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("blind kit failed: {error}");
                ExitCode::FAILURE
            }
        };
    }

    match render_html(&text, Width::from_em(width_em)) {
        Ok(html) => {
            print!("{html}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("render failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn write_blind_kit(path: &str, out: &str, width_em: i32, seed: u64) -> Result<(), String> {
    let input = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let samples: Vec<&str> = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if samples.is_empty() {
        return Err(String::from("no samples"));
    }

    std::fs::create_dir_all(out).map_err(|error| format!("{out}: {error}"))?;
    let (html, key) = blind_sheet(&samples, Width::from_em(width_em), seed)?;
    std::fs::write(format!("{out}/sheet.html"), html).map_err(|error| error.to_string())?;

    let mut key_csv = String::from("paragraph,kp_side\n");
    for (index, side) in &key {
        key_csv.push_str(&format!("{index},{side}\n"));
    }
    std::fs::write(format!("{out}/key.csv"), key_csv).map_err(|error| error.to_string())?;

    let mut ratings = String::from("paragraph,evaluator,choice\n");
    for index in 0..samples.len() {
        for evaluator in 0..5 {
            ratings.push_str(&format!("{index},{evaluator},\n"));
        }
    }
    std::fs::write(format!("{out}/ratings.template.csv"), ratings)
        .map_err(|error| error.to_string())?;

    println!(
        "wrote {out}/sheet.html ({} samples), key.csv, ratings.template.csv",
        samples.len()
    );
    Ok(())
}
