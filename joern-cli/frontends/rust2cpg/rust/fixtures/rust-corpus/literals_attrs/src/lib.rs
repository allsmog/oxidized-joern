#[inline]
pub fn fast(x: i32) -> i32 {
    x + 1
}

#[must_use]
pub fn pure(x: i32) -> i32 {
    x * 2
}

#[allow(dead_code)]
fn unused() {}

pub fn escapes() -> String {
    let s = "tab\there\nnewline\\backslash\u{1F600}";
    let c = '\n';
    let raw = r"no\escape";
    let multi = "line one \
                 continued";
    format!("{}{}{}{}", s, c, raw, multi)
}

pub fn quote_count(text: &str) -> usize {
    text.chars().filter(|&c| c == '"').count()
}
