//! The figure engine from a shell, for trying a model against it:
//!   figure kinds              the kinds, one per line, with what each is for
//!   figure schema KIND        the JSON Schema a model fills in for that kind
//!   figure guide KIND         the words that go with it
//!   figure example KIND       the built-in example, as JSON
//!   figure check FILE         what the checks say of a spec (exit 1 when it is refused)
//! A file may hold the figure itself or a model's reply with it under `structured_output`.

use incipit_scene::{check, schema, Figure, Kind, Rough};

fn kind(name: &str) -> Kind {
    Kind::parse(name).unwrap_or_else(|| {
        eprintln!("no kind \"{name}\"; the kinds are: {}", names());
        std::process::exit(2);
    })
}

fn names() -> String {
    Kind::ALL.map(Kind::name).join(", ")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["kinds"] => {
            for k in Kind::ALL {
                println!("{}\t{}", k.name(), k.purpose());
            }
        }
        ["schema", name] => println!(
            "{}",
            serde_json::to_string_pretty(&schema::schema(kind(name))).expect("a schema is JSON")
        ),
        ["guide", name] => println!("{}", schema::guide(kind(name))),
        ["example", name] => println!(
            "{}",
            serde_json::to_string_pretty(&schema::example(kind(name))).expect("an example is JSON")
        ),
        ["check", file] => {
            let json = std::fs::read_to_string(file).unwrap_or_else(|e| {
                eprintln!("{file}: {e}");
                std::process::exit(2);
            });
            let mut value: serde_json::Value = serde_json::from_str(&json).unwrap_or_else(|e| {
                println!("not JSON: {e}");
                std::process::exit(1);
            });
            if let Some(inner) = value.get("structured_output") {
                value = inner.clone();
            }
            let figure: Figure = match serde_json::from_value(value) {
                Ok(f) => f,
                Err(e) => {
                    println!("not a figure: {e}");
                    std::process::exit(1);
                }
            };
            let report = check(&figure, &Rough);
            for e in &report.errors {
                println!("error: {e}");
            }
            for w in &report.warnings {
                println!("warning: {w}");
            }
            println!(
                "{}: {} states looked at, {} errors, {} warnings",
                figure.kind().name(),
                report.states,
                report.errors.len(),
                report.warnings.len()
            );
            if !report.ok() {
                std::process::exit(1);
            }
        }
        _ => eprintln!(
            "usage: figure kinds | schema KIND | guide KIND | example KIND | check FILE\nkinds: {}",
            names()
        ),
    }
}
