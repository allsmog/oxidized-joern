pub fn head_and_rest(values: &[i32]) -> i32 {
    match values {
        [first, rest @ ..] => first + rest.iter().sum::<i32>(),
        [] => 0,
    }
}

pub fn classify(c: char) -> &'static str {
    match c {
        'a'..='z' => "lower",
        'A'..='Z' => "upper",
        '0'..='9' => "digit",
        _ => "other",
    }
}

pub struct Point {
    x: i32,
    y: i32,
    label: String,
}

pub fn make(x: i32, y: i32) -> Point {
    let label = format!("{x},{y}");
    Point { x, y, label }
}

#[repr(i32)]
pub enum Code {
    A = 10,
    B,
    C = 20,
    D,
}

pub fn code_value(c: Code) -> i32 {
    c as i32
}

pub fn nested_match(pair: (Option<i32>, Result<i32, ()>)) -> i32 {
    match pair {
        (Some(a), Ok(b)) => a + b,
        (Some(a), Err(_)) => a,
        (None, Ok(b)) => b,
        (None, Err(_)) => -1,
    }
}
