//! `soma-eval` CLI.
//!
//! ```text
//! soma-eval bench [--repeats 5] [--sizes 100,500,1000,2000,4000] [--check]
//! soma-eval sweep-tolerance [--atoms 200] [--tolerances 1000,2000,3000] [--scales 50,100,200]
//! soma-eval sweep-weight --param boundary_deviation [--values 0,25,50,100] [--atoms 200]
//! soma-eval blind preferences.csv
//! ```

use std::process::ExitCode;

use soma_core::{
    break_paragraph_traced, BreakConfig, ParamSet, Permille, SyntheticMetrics, Weight, Width,
};
use soma_eval::{
    bench, blind_report, build_paragraph, parse_observations, peak_rss_kb, sweep_tolerance,
    sweep_weight, AggregateMetrics, BlindDecision, CorpusKind, WeightParam,
};

const DEFAULT_SIZES: &[usize] = &[100, 500, 1_000, 2_000, 4_000];
const DEFAULT_TOLERANCES: &[u16] = &[1_000, 2_000, 3_000, 4_000, 6_000];
const DEFAULT_SCALES: &[u32] = &[50, 100, 200, 400];
const DEFAULT_SWEEP_ATOMS: usize = 200;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bench") => bench_command(&args[1..]),
        Some("sweep-tolerance") => sweep_tolerance_command(&args[1..]),
        Some("sweep-weight") => sweep_weight_command(&args[1..]),
        Some("blind") => blind_command(&args[1..]),
        Some("inspect") => inspect_command(&args[1..]),
        Some("score") => score_command(&args[1..]),
        Some("corpus") => corpus_command(&args[1..]),
        Some("calibrate") => calibrate_command(&args[1..]),
        Some("help") | Some("--help") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!("soma-eval commands:");
    println!("  bench [--repeats N] [--sizes a,b,c] [--max-kp-atoms N] [--check]");
    println!("  sweep-tolerance [--atoms N] [--tolerances a,b,c] [--scales a,b,c]");
    println!("  sweep-weight --param NAME [--values a,b,c] [--atoms N]");
    println!("  blind <preferences.csv>");
    println!("  inspect [--kind pure-cjk|mixed|punct-dense] [--atoms N]");
    println!("  score [--paragraphs N]   (Q' effect scorecard, CSV)");
    println!("  corpus [--dir PATH] [--tolerance N] [--badness-scale N]");
    println!("  calibrate [--dir PATH] [--tolerances a,b,c] [--scales a,b,c]");
    println!();
    println!("weight names: line, break_penalty, compress_amount, compress_count,");
    println!("              boundary_deviation, short_last_scale");
}

fn bench_command(args: &[String]) -> ExitCode {
    let repeats = match option(args, "--repeats") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) => value,
            Err(_) => return fail("invalid --repeats value"),
        },
        None => 5,
    };
    let max_kp_atoms = match option(args, "--max-kp-atoms") {
        Some(value) => match value.parse::<u32>() {
            Ok(value) if (1..=10_000).contains(&value) => Some(value),
            _ => return fail("invalid --max-kp-atoms value"),
        },
        None => None,
    };
    let sizes = option(args, "--sizes")
        .map(|value| parse_list::<usize>(&value))
        .unwrap_or_else(|| DEFAULT_SIZES.to_vec());
    let check = flag(args, "--check");
    let config = if let Some(limit) = max_kp_atoms {
        let mut params = ParamSet::default_v1();
        params.break_params.max_kp_atoms = Some(limit);
        BreakConfig::from_params(params)
    } else {
        BreakConfig::default()
    };

    println!(
        "kind,atoms,lines,p50_ms,p95_ms,evaluated,cost,breaker,stage,length_limited,emergency,overfull,violations,trim_total,boundary_deviation"
    );
    let mut budget_ok = true;
    for kind in CorpusKind::ALL {
        for row in bench(kind, &sizes, repeats, &config) {
            println!(
                "{},{},{},{:.3},{:.3},{},{},{},{},{},{},{},{},{},{}",
                row.kind.as_str(),
                row.atoms,
                row.metrics.lines,
                row.p50_ms,
                row.p95_ms,
                row.metrics.evaluated,
                row.metrics.cost,
                row.metrics.breaker.as_str(),
                row.metrics.stage.as_str(),
                row.metrics.length_limited,
                row.metrics.emergency_lines,
                row.metrics.overfull_lines,
                row.metrics.forbidden_violations,
                row.metrics.trim_total,
                row.metrics.boundary_deviation_total,
            );
            if check {
                let budget = match row.target_atoms {
                    0..=2_000 => Some(20.0),
                    2_001..=4_000 => Some(80.0),
                    _ => None,
                };
                if let Some(budget) = budget {
                    if row.p95_ms > budget {
                        eprintln!(
                            "budget exceeded: {} atoms={} p95={:.3}ms > {:.1}ms",
                            row.kind.as_str(),
                            row.atoms,
                            row.p95_ms,
                            budget
                        );
                        budget_ok = false;
                    }
                }
            }
        }
    }

    if let Some(peak) = peak_rss_kb() {
        println!("# peak_rss_kb={peak}");
    }
    if check {
        if budget_ok {
            println!("# budget: PASS");
            ExitCode::SUCCESS
        } else {
            println!("# budget: FAIL");
            ExitCode::FAILURE
        }
    } else {
        ExitCode::SUCCESS
    }
}

fn sweep_tolerance_command(args: &[String]) -> ExitCode {
    let atoms = match option(args, "--atoms") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) => value,
            Err(_) => return fail("invalid --atoms value"),
        },
        None => DEFAULT_SWEEP_ATOMS,
    };
    let tolerances = option(args, "--tolerances")
        .map(|value| parse_list::<u16>(&value))
        .unwrap_or_else(|| DEFAULT_TOLERANCES.to_vec());
    if tolerances.iter().any(|tolerance| *tolerance > 10_000) {
        return fail("tolerances must be <= 10000 permille");
    }
    let scales = option(args, "--scales")
        .map(|value| parse_list::<u32>(&value))
        .unwrap_or_else(|| DEFAULT_SCALES.to_vec());
    if scales.iter().any(|scale| *scale > 1_000_000) {
        return fail("badness scales must be <= 1000000");
    }
    let base = ParamSet::default_v1();

    println!(
        "tolerance,badness_scale,paragraphs,lines,total_cost,emergency_lines,overfull_lines,forbidden_violations,trim_total,boundary_deviation_total,normal,emergency,forbidden_rescue,length_limited"
    );
    for row in sweep_tolerance(&CorpusKind::ALL, atoms, &tolerances, &scales, &base) {
        print_aggregate(row.tolerance as u32, row.badness_scale, &row.aggregate);
    }
    ExitCode::SUCCESS
}

fn sweep_weight_command(args: &[String]) -> ExitCode {
    let Some(name) = option(args, "--param") else {
        return fail("missing --param");
    };
    let Some(param) = WeightParam::ALL
        .iter()
        .copied()
        .find(|param| param.as_str() == name)
    else {
        return fail("unknown --param");
    };
    let atoms = match option(args, "--atoms") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) => value,
            Err(_) => return fail("invalid --atoms value"),
        },
        None => DEFAULT_SWEEP_ATOMS,
    };
    let values = option(args, "--values")
        .map(|value| parse_list::<u32>(&value))
        .unwrap_or_else(|| default_weight_values(param));
    if values.iter().any(|value| *value > 1_000_000_000) {
        return fail("weights must be <= 1000000000");
    }
    let base = ParamSet::default_v1();

    println!(
        "param,value,paragraphs,lines,total_cost,emergency_lines,overfull_lines,forbidden_violations,trim_total,boundary_deviation_total,normal,emergency,forbidden_rescue,length_limited"
    );
    for row in sweep_weight(&CorpusKind::ALL, atoms, param, &values, &base) {
        println!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            row.param.as_str(),
            row.value,
            row.aggregate.paragraphs,
            row.aggregate.lines,
            row.aggregate.cost,
            row.aggregate.emergency_lines,
            row.aggregate.overfull_lines,
            row.aggregate.forbidden_violations,
            row.aggregate.trim_total,
            row.aggregate.boundary_deviation_total,
            row.aggregate.normal,
            row.aggregate.emergency,
            row.aggregate.forbidden_rescue,
            row.aggregate.length_limited,
        );
    }
    ExitCode::SUCCESS
}

fn default_weight_values(param: WeightParam) -> Vec<u32> {
    match param {
        WeightParam::Line | WeightParam::BreakPenalty => vec![10, 25, 50, 100, 200],
        WeightParam::CompressAmount => vec![5, 10, 20, 40],
        WeightParam::CompressCount => vec![0, 5, 10, 20],
        WeightParam::BoundaryDeviation => vec![0, 25, 50, 100],
        WeightParam::ShortLastScale => vec![100_000, 300_000, 1_000_000],
    }
}

fn print_aggregate(a: u32, b: u32, aggregate: &AggregateMetrics) {
    println!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        a,
        b,
        aggregate.paragraphs,
        aggregate.lines,
        aggregate.cost,
        aggregate.emergency_lines,
        aggregate.overfull_lines,
        aggregate.forbidden_violations,
        aggregate.trim_total,
        aggregate.boundary_deviation_total,
        aggregate.normal,
        aggregate.emergency,
        aggregate.forbidden_rescue,
        aggregate.length_limited,
    );
}

fn inspect_command(args: &[String]) -> ExitCode {
    let kind = match option(args, "--kind") {
        Some(name) => match CorpusKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == name)
        {
            Some(kind) => kind,
            None => return fail("unknown --kind"),
        },
        None => CorpusKind::PureCjk,
    };
    let atoms = match option(args, "--atoms") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) => value,
            Err(_) => return fail("invalid --atoms value"),
        },
        None => 500,
    };

    let paragraph = build_paragraph(kind, atoms, Width::from_em(20));
    let (result, trace) = break_paragraph_traced(
        &paragraph,
        &SyntheticMetrics::default(),
        &BreakConfig::default(),
    )
    .expect("inspect corpus");

    println!(
        "kind={} atoms={} lines={} breaker={} stage={} length_limited={} emergency_lines={} violations={}",
        kind.as_str(),
        paragraph.atoms.len(),
        result.lines.len(),
        result.breaker_used.as_str(),
        result.fallback_stage.as_str(),
        result.length_limited,
        result.emergency_lines,
        result.forbidden_violations,
    );
    for line in &trace.lines {
        println!(
            "{}..{} target={} natural={} effective={} ratio={} badness={} short={} bdev={} bdev_exact={} emergency={} overfull={} cost={}",
            line.start,
            line.end,
            line.target,
            line.natural,
            line.effective,
            line.ratio_milli,
            line.badness,
            line.short_score,
            line.boundary_deviation,
            line.boundary_deviation_exact,
            line.emergency,
            line.overfull,
            line.cost,
        );
    }
    ExitCode::SUCCESS
}

fn blind_command(args: &[String]) -> ExitCode {
    let Some(path) = args.first() else {
        return fail("missing preferences.csv");
    };
    let input = match std::fs::read_to_string(path) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return ExitCode::FAILURE;
        }
    };
    let observations = match parse_observations(&input) {
        Ok(observations) => observations,
        Err(error) => return fail(&error),
    };
    let report = match blind_report(&observations) {
        Ok(report) => report,
        Err(error) => return fail(&error),
    };

    println!("paragraphs={}", report.paragraphs);
    println!("evaluators={}", report.evaluators);
    println!("a_wins={}", report.a_wins);
    println!("b_wins={}", report.b_wins);
    println!("ties={}", report.ties);
    println!("unanimous_b={}", report.unanimous_b);
    println!("point={:.4}", report.point);
    println!(
        "wilson95=[{:.4},{:.4}]",
        report.wilson_low, report.wilson_high
    );
    println!(
        "fleiss_kappa={}",
        report
            .kappa
            .map(|value| format!("{value:.4}"))
            .unwrap_or_else(|| "undefined".into())
    );
    println!("sample_ok={}", report.sample_ok);
    println!("decision={}", report.decision.as_str());

    match report.decision {
        BlindDecision::Pass => ExitCode::SUCCESS,
        BlindDecision::Inconclusive => ExitCode::from(2),
        BlindDecision::Fail => ExitCode::FAILURE,
    }
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn parse_list<T: std::str::FromStr>(value: &str) -> Vec<T> {
    value
        .split(',')
        .filter_map(|part| part.trim().parse::<T>().ok())
        .collect()
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}

fn score_command(args: &[String]) -> ExitCode {
    let paragraphs = match option(args, "--paragraphs") {
        Some(value) => match value.parse::<usize>() {
            Ok(value) => value,
            Err(_) => {
                eprintln!("--paragraphs must be a number");
                return ExitCode::FAILURE;
            }
        },
        None => 40,
    };
    match soma_eval::score::scorecard(paragraphs) {
        Some(card) => {
            print!("{}", card.csv());
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("scorecard failed to lay out the corpus");
            ExitCode::FAILURE
        }
    }
}

fn corpus_command(args: &[String]) -> ExitCode {
    let dir = option(args, "--dir").unwrap_or_else(|| String::from("corpus/zh"));
    let mut params = ParamSet::default_v1();
    if let Some(value) = option(args, "--tolerance") {
        match value.parse::<u16>() {
            Ok(value) => params.break_params.tolerance = Permille(value),
            Err(_) => {
                eprintln!("--tolerance must be a number");
                return ExitCode::FAILURE;
            }
        }
    }
    if let Some(value) = option(args, "--badness-scale") {
        match value.parse::<u32>() {
            Ok(value) => params.break_params.costs.badness_scale = Weight(value),
            Err(_) => {
                eprintln!("--badness-scale must be a number");
                return ExitCode::FAILURE;
            }
        }
    }
    match soma_eval::corpus::evaluate_dir(std::path::Path::new(&dir), &params) {
        Ok(report) => {
            print!("{}", report.csv());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn calibrate_command(args: &[String]) -> ExitCode {
    let dir = option(args, "--dir").unwrap_or_else(|| String::from("corpus/zh"));
    let tolerances: Vec<u16> = match option(args, "--tolerances") {
        Some(value) => match soma_eval::corpus::parse_list(&value) {
            Ok(values) => values,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
        None => vec![1_500, 2_000, 2_500, 3_000],
    };
    let scales: Vec<u32> = match option(args, "--scales") {
        Some(value) => match soma_eval::corpus::parse_list(&value) {
            Ok(values) => values,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
        None => vec![50, 100, 200],
    };
    match soma_eval::corpus::calibrate(std::path::Path::new(&dir), &tolerances, &scales) {
        Ok(rows) => {
            println!("{}", soma_eval::corpus::SweepRow::csv_header());
            for row in &rows {
                println!("{}", row.csv());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
