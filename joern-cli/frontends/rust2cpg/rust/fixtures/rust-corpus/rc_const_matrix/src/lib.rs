use std::collections::BTreeMap;
use std::rc::Rc;

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Circle(f64);

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.0 * self.0
    }
}

pub fn shared_area() -> f64 {
    let s: Rc<dyn Shape> = Rc::new(Circle(2.0));
    s.area()
}

pub fn swap_ends<T>(v: &mut [T]) {
    if v.len() >= 2 {
        let last = v.len() - 1;
        v.swap(0, last);
    }
}

pub fn exchange<T>(a: &mut T, b: &mut T) {
    std::mem::swap(a, b);
}

pub fn matmul<const N: usize>(a: &[[f64; N]; N], b: &[[f64; N]; N]) -> [[f64; N]; N] {
    let mut out = [[0.0; N]; N];
    for i in 0..N {
        for j in 0..N {
            for k in 0..N {
                out[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    out
}

pub struct Celsius(f64);
pub struct Fahrenheit(f64);

impl PartialEq<Fahrenheit> for Celsius {
    fn eq(&self, other: &Fahrenheit) -> bool {
        (self.0 * 9.0 / 5.0 + 32.0 - other.0).abs() < 0.01
    }
}

pub fn pop_highest(mut m: BTreeMap<i32, i32>) -> Option<(i32, i32)> {
    m.pop_last()
}

pub fn uppercase_positions(s: &str) -> Vec<usize> {
    s.char_indices()
        .filter(|(_, c)| c.is_uppercase())
        .map(|(i, _)| i)
        .collect()
}
