//! Print whether a table covers some codepoints: `coverage <table> <hex>...`.
fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: coverage <table.smt> <hex>...")?;
    let bytes = std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    let table = soma_metrics::MetricsTable::parse(&bytes).map_err(|error| format!("{error:?}"))?;
    for spec in args {
        let codepoint =
            u32::from_str_radix(spec.trim_start_matches("0x"), 16).map_err(|_| spec.clone())?;
        let mut row = Vec::new();
        for face in table.faces() {
            row.push(if face.has(codepoint) {
                format!("f{}:{}", face.family, face.advance_at(codepoint))
            } else {
                format!("f{}:-", face.family)
            });
        }
        println!("U+{codepoint:04X} {}", row.join(" "));
    }
    Ok(())
}
