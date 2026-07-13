use std::collections::BinaryHeap;
use std::marker::PhantomData;

pub fn powers_of_two(n: usize) -> Vec<u64> {
    std::iter::successors(Some(1u64), |&x| x.checked_mul(2))
        .take(n)
        .collect()
}

pub fn heap_sort(v: Vec<i32>) -> Vec<i32> {
    let heap: BinaryHeap<i32> = v.into_iter().collect();
    heap.into_sorted_vec()
}

pub fn ends(v: &[i32]) -> i32 {
    let head = v.split_first().map(|(h, _)| *h).unwrap_or(0);
    let tail = v.split_last().map(|(t, _)| *t).unwrap_or(0);
    let first = v.first().copied().unwrap_or(0);
    head + tail + first
}

pub fn transcendental(x: f32) -> f32 {
    x.sqrt() + x.sin() + x.cos() + x.ln() + x.exp()
}

pub enum AppError {
    Parse,
    Io,
}

impl From<std::num::ParseIntError> for AppError {
    fn from(_: std::num::ParseIntError) -> Self {
        AppError::Parse
    }
}

pub fn parse_int(s: &str) -> Result<i32, AppError> {
    let n: i32 = s.parse()?;
    Ok(n)
}

pub struct Token<'a, T> {
    id: u32,
    _marker: PhantomData<&'a T>,
}

impl<'a, T> Token<'a, T> {
    pub fn new(id: u32) -> Self {
        Token {
            id,
            _marker: PhantomData,
        }
    }
    pub fn id(&self) -> u32 {
        self.id
    }
}
