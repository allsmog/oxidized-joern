pub fn leading_alpha(words: &[&str]) -> Vec<char> {
    words
        .iter()
        .flat_map(|s| s.chars())
        .take_while(|c| c.is_alphabetic())
        .collect()
}

pub fn running_sums() -> Vec<i32> {
    (0..20)
        .step_by(3)
        .scan(0, |state, x| {
            *state += x;
            Some(*state)
        })
        .collect()
}

pub fn nested_result(x: Option<Result<i32, String>>) -> i32 {
    match x {
        Some(Ok(v)) => v,
        Some(Err(_)) => -1,
        None => 0,
    }
}

pub fn chained(a: &[i32], b: &[i32]) -> Vec<i32> {
    a.iter()
        .chain(b.iter())
        .copied()
        .filter(|x| *x != 0)
        .collect()
}

pub fn joined(parts: &[&str]) -> String {
    parts.iter().copied().collect::<Vec<_>>().join("-")
}
