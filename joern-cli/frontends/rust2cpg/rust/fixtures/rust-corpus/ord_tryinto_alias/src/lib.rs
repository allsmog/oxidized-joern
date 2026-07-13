use std::cmp::Ordering;
use std::convert::TryInto;

#[derive(PartialEq, Eq)]
pub struct Person {
    last: String,
    first: String,
    age: u32,
}

impl Ord for Person {
    fn cmp(&self, other: &Person) -> Ordering {
        self.last
            .cmp(&other.last)
            .then_with(|| self.first.cmp(&other.first))
            .then_with(|| self.age.cmp(&other.age))
    }
}

impl PartialOrd for Person {
    fn partial_cmp(&self, other: &Person) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn to_byte(x: i64) -> Result<u8, std::num::TryFromIntError> {
    x.try_into()
}

pub fn to_triple(v: Vec<i32>) -> Result<[i32; 3], Vec<i32>> {
    v.try_into()
}

pub type MyResult<T> = Result<T, String>;

pub fn parse(s: &str) -> MyResult<i32> {
    s.parse().map_err(|_| "bad".to_string())
}

pub fn parse_inc() -> MyResult<i32> {
    let x = parse("5")?;
    Ok(x + 1)
}

pub struct Generator<F: FnMut() -> u32> {
    gen: F,
}

impl<F: FnMut() -> u32> Generator<F> {
    pub fn new(gen: F) -> Self {
        Generator { gen }
    }
    pub fn next(&mut self) -> u32 {
        (self.gen)()
    }
}
