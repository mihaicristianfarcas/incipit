//! The three notations a formula comes in, each written as Unicode math.

use super::*;

#[test]
fn spoken_math_is_written_as_math() {
    let cases = [
        ("x 1 comma x 2 comma x 3 comma x 4", "x₁, x₂, x₃, x₄"),
        ("upper K 0", "K₀"),
        ("m c squared", "mc²"),
        ("rho 0 upper V", "ρ₀V"),
        ("normal upper Delta t prime", "Δt′"),
        ("x prime minus c t Superscript prime Baseline equals 0", "x′ − ct′ = 0"),
        ("StartFraction upper E 0 Over c squared EndFraction", "E₀/c²"),
        ("StartRoot 1 minus v squared slash c squared EndRoot", "√(1 − v²/c²)"),
        ("StartRoot negative 1 EndRoot dot c t", "√(−1)·ct"),
        ("upper A long right-arrow upper M", "A ⟶ M"),
        ("x 1 ellipsis x 4", "x₁ … x₄"),
        ("times", "×"),
        ("pi", "π"),
        ("g 11", "g₁₁"),
        (
            "x prime equals a left-parenthesis 1 minus StartFraction v squared Over c squared \
             EndFraction right-parenthesis x period",
            "x′ = a(1 − v²/c²)x.",
        ),
        (
            "x prime 1 squared plus x prime 2 squared equals x 1 squared plus x 2 squared",
            "x′₁² + x′₂² = x₁² + x₂²",
        ),
        ("w plus v left-parenthesis 1 minus StartFraction 1 Over n squared EndFraction right-parenthesis", "w + v(1 − 1/n²)"),
        (
            "StartLayout 1st Row 1st Column x Superscript prime Baseline 2nd Column equals \
             3rd Column a x minus b c t 2nd Row 1st Column c t Superscript prime Baseline \
             2nd Column equals 3rd Column a c t minus b x EndLayout right-brace period",
            "x′ = ax − bct; ct′ = act − bx.",
        ),
    ];
    for (said, written) in cases {
        assert_eq!(spoken(said).as_deref(), Some(written), "{said}");
    }
}

#[test]
fn words_that_are_not_spoken_math_are_left_alone() {
    assert_eq!(spoken("A photograph of a train"), None);
    assert_eq!(spoken("Figure 3"), None);
    assert_eq!(spoken("StartFraction a Over b"), None);
    assert_eq!(spoken(""), None);
}

#[test]
fn tex_is_written_as_math() {
    assert_eq!(tex("E = mc^2"), "E = mc²");
    assert_eq!(tex("$x_{1} + x_2'$"), "x₁ + x₂′");
    assert_eq!(tex("\\frac{a+b}{c}"), "(a+b)/c");
    assert_eq!(tex("\\sqrt{1-\\frac{v^2}{c^2}}"), "√(1−v²/c²)");
    assert_eq!(tex("\\alpha \\leq \\Omega"), "α ≤ Ω");
    assert_eq!(tex("\\sin\\theta"), "sin θ");
    assert_eq!(tex("x^{q}"), "x^q");
    assert_eq!(tex("e^{i\\pi}"), "e^(iπ)");
    assert!(looks_like_tex("\\frac{1}{2}") && !looks_like_tex("one half"));
}

#[test]
fn mathml_is_read_by_its_structure() {
    let html = scraper::Html::parse_fragment(
        "<math><mrow><msup><mi>E</mi><mn>2</mn></msup><mo>=</mo><mfrac><mrow><mi>a</mi>\
         <mo>+</mo><mi>b</mi></mrow><mi>c</mi></mfrac><mo>-</mo><msqrt><mi>x</mi></msqrt>\
         <mo>,</mo><msub><mi>v</mi><mn>0</mn></msub><mover><mi>v</mi><mo>→</mo></mover>\
         </mrow><annotation encoding=\"TeX\">ignored</annotation></math>",
    );
    let math = html
        .select(&scraper::Selector::parse("math").unwrap())
        .next()
        .unwrap();
    assert_eq!(mathml(math), "E² = (a + b)/c − √x, v₀v⃗");
}
