use std::ops::{MulAssign, SubAssign};

#[derive(Clone, Copy)]
pub struct Value(i32);

impl SubAssign for Value {
    fn sub_assign(&mut self, other: Value) {
        self.0 -= other.0;
    }
}

impl MulAssign<i32> for Value {
    fn mul_assign(&mut self, factor: i32) {
        self.0 *= factor;
    }
}

pub fn extremes(a: i32, b: i32, c: i32) -> (i32, i32) {
    let lo = std::cmp::min(a, std::cmp::min(b, c));
    let hi = a.max(b).max(c);
    (lo, hi)
}

pub fn rotate_bits(x: u32, n: u32) -> u32 {
    ((x << n) | (x >> (32 - n))) & 0xFFFF ^ 0xAAAA
}

pub fn narrowing(x: i64) -> u8 {
    let a = x as i8;
    let b = a as i16 as u64;
    (b as usize % 256) as u8
}
