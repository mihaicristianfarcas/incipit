//! The figure engine from a shell, for trying a model against it:
//!   figure schema        the JSON Schema a model fills in
//!   figure guide         the words that go with it
//!   figure check FILE    what the checks say of a spec (exit 1 when it is refused)

use incipit_scene::{check, schema, Rough, Sequence};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["schema"] => println!(
            "{}",
            serde_json::to_string_pretty(&schema::sequence_schema()).unwrap()
        ),
        ["guide"] => println!("{}", schema::GUIDE),
        ["check", file] => {
            let json = std::fs::read_to_string(file).expect("the file is read");
            let spec: Sequence = match serde_json::from_str(&json) {
                Ok(spec) => spec,
                Err(e) => {
                    println!("not a sequence: {e}");
                    std::process::exit(1);
                }
            };
            let report = check(&spec, &Rough);
            for e in &report.errors {
                println!("error: {e}");
            }
            for w in &report.warnings {
                println!("warning: {w}");
            }
            println!(
                "{} actors, {} events, {} errors, {} warnings",
                spec.actors.len(),
                spec.events.len(),
                report.errors.len(),
                report.warnings.len()
            );
            if !report.ok() {
                std::process::exit(1);
            }
        }
        _ => eprintln!("usage: figure schema | guide | check FILE"),
    }
}
