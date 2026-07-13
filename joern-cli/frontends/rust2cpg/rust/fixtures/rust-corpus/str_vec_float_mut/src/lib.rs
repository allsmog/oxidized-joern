pub fn word_analysis(s: &str) -> usize {
    let words = s.split_ascii_whitespace().count();
    let trimmed = s.trim_end_matches('.');
    words + trimmed.len()
}

pub fn shuffle(v: &mut Vec<i32>) {
    for chunk in v.chunks_mut(2) {
        chunk.reverse();
    }
    if !v.is_empty() {
        v.rotate_left(1);
    }
}

pub fn swap_option(mut opt: Option<Vec<i32>>) -> usize {
    if let Some(v) = opt.as_mut() {
        v.push(1);
    }
    let old = opt.replace(vec![9]);
    old.map(|v| v.len()).unwrap_or(0)
}

pub fn fused(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c) + a.recip() + a.to_degrees()
}

pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else if n < 10 {
        "small"
    } else if n < 100 {
        "medium"
    } else {
        "large"
    }
}
