use std::collections::BTreeSet;

pub fn is_even(n: u64) -> bool {
    if n == 0 {
        true
    } else {
        is_odd(n - 1)
    }
}

pub fn is_odd(n: u64) -> bool {
    if n == 0 {
        false
    } else {
        is_even(n - 1)
    }
}

pub fn in_range(values: &[i32]) -> Vec<i32> {
    let set: BTreeSet<i32> = values.iter().copied().collect();
    set.range(2..8).copied().collect()
}

pub fn ackermann(m: u64, n: u64) -> u64 {
    if m == 0 {
        n + 1
    } else if n == 0 {
        ackermann(m - 1, 1)
    } else {
        ackermann(m - 1, ackermann(m, n - 1))
    }
}
