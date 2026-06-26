use std::collections::BTreeMap;

/// A task status.
pub enum Status {
    /// The active state.
    Active,
    /// The paused state with a reason.
    Paused(String),
}

/// Runtime configuration.
pub struct Config {
    /// Maximum number of retries.
    pub retries: u32,
    /// Whether verbose logging is enabled.
    pub verbose: bool,
}

pub fn arithmetic(a: i32, b: i32) -> i32 {
    let s = a.saturating_add(b);
    let w = a.wrapping_mul(b);
    let c = a.checked_div(b).unwrap_or(0);
    let p = a.pow(2);
    let d = a.abs_diff(b) as i32;
    s.clamp(-100, 100) + w + c + p + d
}

pub fn indexed(pairs: &[(i32, &str)]) -> BTreeMap<i32, String> {
    pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
}

pub fn padded() -> Vec<i32> {
    std::iter::once(1)
        .chain(std::iter::repeat(0).take(3))
        .chain(std::iter::empty())
        .collect()
}
