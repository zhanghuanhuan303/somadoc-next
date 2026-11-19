//! `soma-demo` CLI: render a paragraph to a self-contained HTML page.
//!
//! ```text
//! soma-demo [input.txt] [--width-em 20]
//! ```

use std::process::ExitCode;

use soma_core::Width;
use soma_demo::{render_html, SAMPLE_TEXT};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input_path: Option<&str> = None;
    let mut width_em = 20i32;

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
            "--help" | "-h" => {
                println!("usage: soma-demo [input.txt] [--width-em 20]");
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
