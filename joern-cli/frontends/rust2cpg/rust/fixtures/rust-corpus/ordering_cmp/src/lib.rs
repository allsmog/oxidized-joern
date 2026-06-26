use std::cmp::Ordering;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

pub fn compare(a: &Version, b: &Version) -> Ordering {
    a.cmp(b)
}

pub fn descending(mut values: Vec<i32>) -> Vec<i32> {
    values.sort_by(|a, b| b.cmp(a));
    values
}

pub fn by_second(mut pairs: Vec<(String, u32)>) -> Vec<(String, u32)> {
    pairs.sort_by_key(|p| p.1);
    pairs
}

pub fn find(sorted: &[i32], target: i32) -> Option<usize> {
    sorted.binary_search(&target).ok()
}

pub fn alphanumeric_count(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphanumeric()).count()
}

pub fn hex_digit(c: char) -> Option<u32> {
    c.to_digit(16)
}
