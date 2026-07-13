use std::collections::HashMap;

pub fn squares(n: u32) -> Vec<u32> {
    (1..=n).map(|x| x * x).collect()
}

pub fn evens(values: &[i32]) -> Vec<i32> {
    values.iter().filter(|&&x| x % 2 == 0).copied().collect()
}

pub fn total(values: &[i32]) -> i32 {
    values.iter().fold(0, |acc, &x| acc + x)
}

pub fn word_counts(words: &[&str]) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for &w in words {
        *counts.entry(w.to_string()).or_insert(0) += 1;
    }
    counts
}

pub fn apply<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    f(x)
}

pub fn chained(values: Vec<i32>) -> i32 {
    values
        .into_iter()
        .map(|x| x + 1)
        .filter(|x| x > &2)
        .sum()
}
