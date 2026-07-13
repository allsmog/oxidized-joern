#![allow(dead_code)]

pub fn graded(x: i32) -> i32 {
    match x {
        n @ 0..=9 if n % 2 == 0 => n * 100,
        n @ 10..=99 => n,
        _ => -1,
    }
}

pub fn byte_sum() -> u32 {
    let bytes = b"hello";
    bytes.iter().map(|&b| b as u32).sum()
}

pub fn glyph_count() -> usize {
    let s = "café ✓ 😀";
    s.chars().count()
}

pub fn parse_or_zero(s: &str) -> i32 {
    s.parse::<i32>().unwrap_or(0)
}

pub fn reserve() -> Vec<u8> {
    Vec::<u8>::with_capacity(10)
}

fn unused_helper() -> i32 {
    7
}
