//! Typesets each line of a file of LaTeX formulas and prints the ones that fail, with why.
//!   probe FILE

fn main() {
    let file = std::env::args().nth(1).expect("usage: probe FILE");
    let text = std::fs::read_to_string(file).expect("the file is read");
    let (mut ok, mut bad) = (0, 0);
    for latex in text.lines().filter(|l| !l.trim().is_empty()) {
        match incipit_math::render(latex, true, 16.0, (0, 0, 0), 1.0) {
            Ok(_) => ok += 1,
            Err(e) => {
                bad += 1;
                println!("{latex}\t{e}");
            }
        }
    }
    eprintln!("{ok} typeset, {bad} failed");
}
