use std::collections::BTreeMap;
use std::ops::Add;

pub fn ordered(m: &BTreeMap<i32, String>) -> usize {
    let first = m.first_key_value();
    let last = m.last_key_value();
    let mid: Vec<_> = m.range(2..8).collect();
    first.map(|(k, _)| *k).unwrap_or(0) as usize
        + last.map(|(k, _)| *k).unwrap_or(0) as usize
        + mid.len()
}

#[derive(Clone, Copy)]
pub struct Vector(i32);

impl Add for &Vector {
    type Output = Vector;
    fn add(self, other: &Vector) -> Vector {
        Vector(self.0 + other.0)
    }
}

pub fn add_refs(a: &Vector, b: &Vector) -> Vector {
    a + b
}

pub fn double(s: &str) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let n: i32 = s.parse()?;
    Ok(n * 2)
}

pub fn parse_all(inputs: &[&str]) -> Result<Vec<i32>, std::num::ParseIntError> {
    inputs
        .iter()
        .map(|s| s.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()
}

pub async fn fetch(id: u32) -> Result<u32, String> {
    if id > 0 {
        Ok(id * 10)
    } else {
        Err("bad id".into())
    }
}

pub async fn pipeline() -> Result<u32, String> {
    let a = fetch(1).await?;
    let b = fetch(a).await?;
    Ok(a + b)
}
