use std::collections::BTreeMap;

pub fn split_variants(s: &str) -> (Vec<String>, Option<(&str, &str)>) {
    let inclusive: Vec<String> = s.split_inclusive('.').map(|p| p.to_string()).collect();
    let last_eq = s.rsplit_once('=');
    (inclusive, last_eq)
}

pub fn duplicate_prefix(mut v: Vec<i32>) -> Vec<i32> {
    if v.len() >= 2 {
        v.extend_from_within(0..2);
    }
    v
}

pub fn split_at(mut v: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
    let tail = if v.len() >= 2 { v.split_off(2) } else { Vec::new() };
    (v, tail)
}

pub fn option_checks(x: Option<i32>) -> bool {
    x.inspect(|n| {
        let _ = n;
    })
    .is_none_or(|n| n > 0)
}

pub fn result_checks(r: Result<i32, String>) -> Result<i32, String> {
    r.inspect_err(|e| {
        let _ = e;
    })
}

pub fn first_pair(m: BTreeMap<i32, i32>) -> Option<(i32, i32)> {
    m.into_iter().next()
}

pub fn split_iter(v: &[i32]) -> (Vec<i32>, Vec<i32>) {
    let mut it = v.iter();
    let first: Vec<i32> = it.by_ref().take(2).copied().collect();
    let rest: Vec<i32> = it.copied().collect();
    (first, rest)
}
