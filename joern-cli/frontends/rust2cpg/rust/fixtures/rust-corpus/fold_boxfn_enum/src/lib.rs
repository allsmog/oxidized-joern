pub fn parse_number(s: &str) -> u32 {
    s.chars()
        .filter_map(|c| c.to_digit(10))
        .fold(0, |acc, d| acc * 10 + d)
}

pub fn make_op(kind: u8) -> Box<dyn Fn(i32) -> i32> {
    match kind {
        0 => Box::new(|x| x + 1),
        1 => Box::new(|x| x * 2),
        _ => Box::new(|x| x),
    }
}

pub fn apply_op() -> i32 {
    make_op(1)(10)
}

pub fn unwrap_brackets(s: &str) -> &str {
    s.strip_prefix("<<")
        .unwrap_or(s)
        .strip_suffix(">>")
        .unwrap_or(s)
        .trim()
}

pub fn big_positive(x: Option<i32>) -> bool {
    x.is_some_and(|n| n > 5)
}

pub fn or_zero(r: Result<i32, String>) -> i32 {
    r.unwrap_or_default()
}

pub enum Token {
    Eof,
    Ident(String),
    Num { value: i64, radix: u32 },
}

pub fn render(t: &Token) -> String {
    match t {
        Token::Eof => "eof".to_string(),
        Token::Ident(s) => s.clone(),
        Token::Num { value, radix } => format!("{}@{}", value, radix),
    }
}
