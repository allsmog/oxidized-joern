use std::fmt;
use std::ops::{Add, Mul};

pub struct Buffer<'a, T, const N: usize> {
    items: &'a [T; N],
}

impl<'a, T: Copy, const N: usize> Buffer<'a, T, N> {
    pub fn new(items: &'a [T; N]) -> Self {
        Buffer { items }
    }
    pub fn first(&self) -> Option<T> {
        self.items.first().copied()
    }
    pub fn len(&self) -> usize {
        N
    }
}

#[derive(Clone, Copy)]
pub struct Scalar(i32);

impl Add for Scalar {
    type Output = Scalar;
    fn add(self, other: Scalar) -> Scalar {
        Scalar(self.0 + other.0)
    }
}

impl Mul for Scalar {
    type Output = Scalar;
    fn mul(self, other: Scalar) -> Scalar {
        Scalar(self.0 * other.0)
    }
}

pub fn combine(a: Scalar, b: Scalar, c: Scalar) -> Scalar {
    a * b + c
}

pub struct Meters(f64);

impl fmt::Display for Meters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)?;
        write!(f, "m")
    }
}

pub fn locate(sorted: &[i32], x: i32) -> String {
    match sorted.binary_search(&x) {
        Ok(idx) => format!("found at {}", idx),
        Err(idx) => format!("insert at {}", idx),
    }
}

pub fn trig(x: f64, y: f64) -> f64 {
    x.hypot(y) + x.atan2(y) + x.log10() + x.exp2() + x.cbrt()
}
