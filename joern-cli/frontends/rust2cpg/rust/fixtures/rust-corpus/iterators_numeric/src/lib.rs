use std::borrow::Cow;
use std::sync::{Arc, Mutex};

pub fn zipped(a: &[i32], b: &[i32]) -> Vec<i32> {
    a.iter()
        .zip(b.iter())
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(_, (x, y))| x + y)
        .collect()
}

pub fn reversed(v: &[i32]) -> Vec<i32> {
    v.iter().rev().take(3).copied().collect()
}

pub fn combinators(x: Option<i32>) -> Result<i32, String> {
    x.map(|v| v * 2)
        .filter(|v| *v > 0)
        .ok_or_else(|| "none".to_string())
}

pub fn numeric(a: u32, b: u32) -> u32 {
    let w = a.wrapping_add(b);
    let c = a.checked_mul(b).unwrap_or(0);
    let s = a.saturating_sub(b);
    (w ^ c) | (s << 2) & 0xFF
}

pub fn shared() -> i32 {
    let data = Arc::new(Mutex::new(0));
    let clone = Arc::clone(&data);
    *clone.lock().unwrap() += 1;
    let guard = data.lock().unwrap();
    *guard
}

pub fn maybe_owned(input: &str) -> Cow<str> {
    if input.contains(' ') {
        Cow::Owned(input.replace(' ', "_"))
    } else {
        Cow::Borrowed(input)
    }
}
