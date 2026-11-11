//! A small, safe expression language for the numbers in a spec that move: "a*x^2 + b",
//! "sin(k*x)", "exp(-(x-m)^2/2)". A model's expression is parsed into a tree and evaluated; it is
//! never run as code. There are no loops, no names but the ones a spec declares, and no way to
//! reach anything but numbers, so the worst an expression can do is give a number that is not one,
//! which the layout handles and the checks report.
//!
//! ```text
//!   numbers 2, 1.5, .5, 3e-2    names: those the caller allows (x, a parameter), pi, e
//!   + - * / ^ (power, right to left), unary -, brackets, < <= > >= (1 when true, 0 when false)
//!   a number or a closing bracket followed by a name or a bracket multiplies: 2x, 3(x + 1), (a)(b)
//!   functions: sin cos tan asin acos atan atan2 sinh cosh tanh sqrt cbrt abs exp ln log log2
//!              floor ceil round sign min max mod if(condition, then, else)
//! ```
//!
//! Text is limited in length and nesting, so a parse cannot run away.

/// The longest expression accepted, in characters.
const LONGEST: usize = 240;
/// The deepest nesting of brackets and operators accepted.
const DEEPEST: usize = 48;

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    node: Node,
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Num(f64),
    Var(usize),
    Neg(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Func {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sinh,
    Cosh,
    Tanh,
    Sqrt,
    Cbrt,
    Abs,
    Exp,
    Ln,
    Log,
    Log2,
    Floor,
    Ceil,
    Round,
    Sign,
    Min,
    Max,
    Mod,
    If,
}

const FUNCTIONS: [(&str, Func, usize); 25] = [
    ("sin", Func::Sin, 1),
    ("cos", Func::Cos, 1),
    ("tan", Func::Tan, 1),
    ("asin", Func::Asin, 1),
    ("acos", Func::Acos, 1),
    ("atan", Func::Atan, 1),
    ("atan2", Func::Atan2, 2),
    ("sinh", Func::Sinh, 1),
    ("cosh", Func::Cosh, 1),
    ("tanh", Func::Tanh, 1),
    ("sqrt", Func::Sqrt, 1),
    ("cbrt", Func::Cbrt, 1),
    ("abs", Func::Abs, 1),
    ("exp", Func::Exp, 1),
    ("ln", Func::Ln, 1),
    ("log", Func::Log, 1),
    ("log2", Func::Log2, 1),
    ("floor", Func::Floor, 1),
    ("ceil", Func::Ceil, 1),
    ("round", Func::Round, 1),
    ("sign", Func::Sign, 1),
    ("min", Func::Min, 2),
    ("max", Func::Max, 2),
    ("mod", Func::Mod, 2),
    ("if", Func::If, 3),
];

/// Names an expression may not take for its own: the constants and the functions.
pub fn reserved(name: &str) -> bool {
    name == "pi" || name == "e" || FUNCTIONS.iter().any(|(n, _, _)| *n == name)
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f64),
    Name(String),
    Op(char),
    Le,
    Ge,
    Open,
    Close,
    Comma,
}

fn tokens(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            // An exponent only when digits follow it, so "2e" stays 2 times e.
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            let v: f64 = text
                .parse()
                .map_err(|_| format!("\"{text}\" is not a number"))?;
            out.push(Token::Num(v));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Token::Name(chars[start..i].iter().collect()));
        } else {
            i += 1;
            let next = chars.get(i).copied();
            out.push(match (c, next) {
                ('<', Some('=')) => {
                    i += 1;
                    Token::Le
                }
                ('>', Some('=')) => {
                    i += 1;
                    Token::Ge
                }
                ('*', Some('*')) => {
                    i += 1;
                    Token::Op('^')
                }
                ('+' | '-' | '*' | '/' | '^' | '<' | '>', _) => Token::Op(c),
                ('×' | '·', _) => Token::Op('*'),
                ('−', _) => Token::Op('-'),
                ('(', _) => Token::Open,
                (')', _) => Token::Close,
                (',', _) => Token::Comma,
                _ => return Err(format!("\"{c}\" cannot be used in an expression")),
            });
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    names: &'a [&'a str],
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn deeper(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > DEEPEST {
            Err("it is nested too deeply".into())
        } else {
            Ok(())
        }
    }

    fn compare(&mut self) -> Result<Node, String> {
        self.deeper()?;
        let left = self.sum()?;
        let op = match self.peek() {
            Some(Token::Op('<')) => Op::Lt,
            Some(Token::Op('>')) => Op::Gt,
            Some(Token::Le) => Op::Le,
            Some(Token::Ge) => Op::Ge,
            _ => {
                self.depth -= 1;
                return Ok(left);
            }
        };
        self.at += 1;
        let right = self.sum()?;
        self.depth -= 1;
        Ok(Node::Bin(op, Box::new(left), Box::new(right)))
    }

    fn sum(&mut self) -> Result<Node, String> {
        let mut left = self.product()?;
        while let Some(Token::Op(c @ ('+' | '-'))) = self.peek() {
            let op = if *c == '+' { Op::Add } else { Op::Sub };
            self.at += 1;
            let right = self.product()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn product(&mut self) -> Result<Node, String> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Token::Op('*')) => Op::Mul,
                Some(Token::Op('/')) => Op::Div,
                // Written side by side: 2x, 3(x + 1), x(x - 1).
                Some(Token::Name(_) | Token::Num(_) | Token::Open) => {
                    let right = self.power()?;
                    left = Node::Bin(Op::Mul, Box::new(left), Box::new(right));
                    continue;
                }
                _ => return Ok(left),
            };
            self.at += 1;
            let right = self.unary()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some(Token::Op('-')) => {
                self.at += 1;
                self.deeper()?;
                let inner = self.unary()?;
                self.depth -= 1;
                Ok(Node::Neg(Box::new(inner)))
            }
            Some(Token::Op('+')) => {
                self.at += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Node, String> {
        let base = self.primary()?;
        if let Some(Token::Op('^')) = self.peek() {
            self.at += 1;
            self.deeper()?;
            let exponent = self.unary()?;
            self.depth -= 1;
            return Ok(Node::Bin(Op::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Node, String> {
        match self.next() {
            Some(Token::Num(v)) => Ok(Node::Num(v)),
            Some(Token::Open) => {
                let inner = self.compare()?;
                match self.next() {
                    Some(Token::Close) => Ok(inner),
                    _ => Err("a bracket is not closed".into()),
                }
            }
            Some(Token::Name(name)) => self.name(name),
            Some(Token::Close) => Err("there is a \")\" with nothing before it".into()),
            Some(Token::Comma) => Err("there is a \",\" outside a function's brackets".into()),
            Some(Token::Op(c)) => Err(format!("\"{c}\" has nothing before it")),
            Some(Token::Le | Token::Ge) => Err("a comparison has nothing before it".into()),
            None => Err("it ends too soon".into()),
        }
    }

    fn name(&mut self, name: String) -> Result<Node, String> {
        if let Some(&(_, func, arity)) = FUNCTIONS.iter().find(|(n, _, _)| *n == name) {
            if self.next() != Some(Token::Open) {
                return Err(format!("{name} needs brackets: {name}(…)"));
            }
            self.deeper()?;
            let mut args = vec![self.compare()?];
            while self.peek() == Some(&Token::Comma) {
                self.at += 1;
                args.push(self.compare()?);
            }
            self.depth -= 1;
            if self.next() != Some(Token::Close) {
                return Err(format!("the brackets of {name} are not closed"));
            }
            if args.len() != arity {
                return Err(format!(
                    "{name} takes {arity} number{}, it was given {}",
                    if arity == 1 { "" } else { "s" },
                    args.len()
                ));
            }
            return Ok(Node::Call(func, args));
        }
        if let Some(i) = self.names.iter().position(|n| *n == name) {
            return Ok(Node::Var(i));
        }
        match name.as_str() {
            "pi" => Ok(Node::Num(std::f64::consts::PI)),
            "e" => Ok(Node::Num(std::f64::consts::E)),
            _ => Err(format!(
                "\"{name}\" is not a name it can use{}",
                if self.names.is_empty() {
                    ": only numbers, pi and e here".to_string()
                } else {
                    format!(" (it can use {}, pi and e)", self.names.join(", "))
                }
            )),
        }
    }
}

impl Expr {
    /// Parses `src`, which may use the variables in `names`; their values are given to `eval` in
    /// the same order. The error says what is wrong in words.
    pub fn parse(src: &str, names: &[&str]) -> Result<Expr, String> {
        let fail = |why: String| format!("the expression \"{src}\" cannot be read: {why}");
        if src.trim().is_empty() {
            return Err(fail("it is empty".into()));
        }
        if src.chars().count() > LONGEST {
            return Err(fail(format!("it is longer than {LONGEST} characters")));
        }
        let mut p = Parser {
            tokens: tokens(src).map_err(fail)?,
            at: 0,
            names,
            depth: 0,
        };
        let node = p.compare().map_err(fail)?;
        if let Some(t) = p.peek() {
            let what = match t {
                Token::Close => "a \")\" that was never opened".to_string(),
                Token::Comma => "a \",\" outside a function's brackets".to_string(),
                _ => "more after a complete expression".to_string(),
            };
            return Err(fail(format!("there is {what}")));
        }
        Ok(Expr { node })
    }

    /// The value with `values` for the names given to `parse`. Never panics; a value outside a
    /// function's domain gives NaN, a division by zero an infinity.
    pub fn eval(&self, values: &[f64]) -> f64 {
        eval(&self.node, values)
    }

    /// The name it is, when it is nothing but one of the names given to `parse`.
    pub fn only_name(&self) -> Option<usize> {
        match self.node {
            Node::Var(i) => Some(i),
            _ => None,
        }
    }
}

fn eval(node: &Node, v: &[f64]) -> f64 {
    match node {
        Node::Num(n) => *n,
        Node::Var(i) => v.get(*i).copied().unwrap_or(f64::NAN),
        Node::Neg(a) => -eval(a, v),
        Node::Bin(op, a, b) => {
            let (a, b) = (eval(a, v), eval(b, v));
            let truth = |t: bool| if t { 1.0 } else { 0.0 };
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
                Op::Pow => a.powf(b),
                Op::Lt => truth(a < b),
                Op::Le => truth(a <= b),
                Op::Gt => truth(a > b),
                Op::Ge => truth(a >= b),
            }
        }
        Node::Call(f, args) => {
            let a = eval(&args[0], v);
            let b = || eval(&args[1], v);
            match f {
                Func::Sin => a.sin(),
                Func::Cos => a.cos(),
                Func::Tan => a.tan(),
                Func::Asin => a.asin(),
                Func::Acos => a.acos(),
                Func::Atan => a.atan(),
                Func::Atan2 => a.atan2(b()),
                Func::Sinh => a.sinh(),
                Func::Cosh => a.cosh(),
                Func::Tanh => a.tanh(),
                Func::Sqrt => a.sqrt(),
                Func::Cbrt => a.cbrt(),
                Func::Abs => a.abs(),
                Func::Exp => a.exp(),
                Func::Ln => a.ln(),
                Func::Log => a.log10(),
                Func::Log2 => a.log2(),
                Func::Floor => a.floor(),
                Func::Ceil => a.ceil(),
                Func::Round => a.round(),
                Func::Sign => {
                    if a > 0.0 {
                        1.0
                    } else if a < 0.0 {
                        -1.0
                    } else {
                        0.0
                    }
                }
                Func::Min => a.min(b()),
                Func::Max => a.max(b()),
                Func::Mod => a.rem_euclid(b()),
                Func::If => {
                    if a != 0.0 && !a.is_nan() {
                        b()
                    } else {
                        eval(&args[2], v)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(src: &str, x: f64) -> f64 {
        Expr::parse(src, &["x", "a"]).unwrap().eval(&[x, 2.0])
    }

    #[test]
    fn arithmetic_follows_the_rules_of_a_page() {
        assert_eq!(at("1 + 2 * 3", 0.0), 7.0);
        assert_eq!(at("-x^2", 3.0), -9.0);
        assert_eq!(at("2^3^2", 0.0), 512.0);
        assert_eq!(at("2^-1", 0.0), 0.5);
        assert_eq!(at("(1 + 2) * 3", 0.0), 9.0);
        assert_eq!(at("a*x^2", 3.0), 18.0);
    }

    #[test]
    fn numbers_and_brackets_side_by_side_multiply() {
        assert_eq!(at("2x", 3.0), 6.0);
        assert_eq!(at("3(x + 1)", 1.0), 6.0);
        assert_eq!(at("2 sin(0)", 1.0), 0.0);
        assert_eq!(at("(x)(a)", 3.0), 6.0);
        assert_eq!(at("2e-1", 0.0), 0.2);
        assert!((at("2e", 0.0) - 2.0 * std::f64::consts::E).abs() < 1e-12);
    }

    #[test]
    fn functions_and_comparisons_give_numbers() {
        assert!((at("sin(pi/2)", 0.0) - 1.0).abs() < 1e-12);
        assert_eq!(at("if(x < 0, -x, x)", -4.0), 4.0);
        assert_eq!(at("max(x, a)", 1.0), 2.0);
        assert_eq!(at("mod(x, 3)", -1.0), 2.0);
        assert!(at("sqrt(x)", -1.0).is_nan());
        assert!(at("1/x", 0.0).is_infinite());
    }

    #[test]
    fn an_unknown_name_is_named_with_the_ones_allowed() {
        let e = Expr::parse("a*x + b", &["x", "a"]).unwrap_err();
        assert!(e.contains("\"b\" is not a name"), "{e}");
        assert!(e.contains("x, a"), "{e}");
    }

    #[test]
    fn broken_text_is_refused_with_a_reason() {
        for (src, why) in [
            ("1 +", "ends too soon"),
            ("(1 + 2", "not closed"),
            ("1 + 2)", "never opened"),
            ("sin x", "needs brackets"),
            ("max(1)", "takes 2 numbers"),
            ("x $ 2", "cannot be used"),
            ("", "empty"),
        ] {
            let e = Expr::parse(src, &["x"]).unwrap_err();
            assert!(e.contains(why), "{src}: {e}");
        }
    }

    #[test]
    fn deep_nesting_is_refused_rather_than_followed() {
        let deep = format!("{}x{}", "(".repeat(100), ")".repeat(100));
        assert!(Expr::parse(&deep, &["x"]).is_err());
        let long = "x+".repeat(200) + "x";
        assert!(Expr::parse(&long, &["x"]).is_err());
    }
}
