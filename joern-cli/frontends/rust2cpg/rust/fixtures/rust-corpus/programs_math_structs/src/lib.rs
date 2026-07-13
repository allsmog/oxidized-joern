use std::ops::{Add, Mul};

#[derive(Clone, Copy)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Add for Complex {
    type Output = Complex;
    fn add(self, o: Complex) -> Complex {
        Complex {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }
}

impl Mul for Complex {
    type Output = Complex;
    fn mul(self, o: Complex) -> Complex {
        Complex {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
}

impl Complex {
    pub fn norm_sq(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
}

pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn dot(&self, o: &Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(&self, o: &Vec3) -> Vec3 {
        Vec3 {
            x: self.y * o.z - self.z * o.y,
            y: self.z * o.x - self.x * o.z,
            z: self.x * o.y - self.y * o.x,
        }
    }
    pub fn len(&self) -> f64 {
        self.dot(self).sqrt()
    }
}

pub fn matmul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let (n, m, p) = (a.len(), b.len(), b[0].len());
    let mut c = vec![vec![0.0; p]; n];
    for i in 0..n {
        for k in 0..m {
            for j in 0..p {
                c[i][j] += a[i][k] * b[k][j];
            }
        }
    }
    c
}

pub struct Fraction {
    num: i64,
    den: i64,
}

impl Fraction {
    pub fn new(num: i64, den: i64) -> Self {
        let g = Self::gcd(num.abs(), den.abs()).max(1);
        Fraction {
            num: num / g,
            den: den / g,
        }
    }
    fn gcd(a: i64, b: i64) -> i64 {
        if b == 0 {
            a
        } else {
            Self::gcd(b, a % b)
        }
    }
    pub fn add(&self, o: &Fraction) -> Fraction {
        Fraction::new(self.num * o.den + o.num * self.den, self.den * o.den)
    }
}

pub fn poly_add(a: &[i64], b: &[i64]) -> Vec<i64> {
    let n = a.len().max(b.len());
    let mut out = vec![0i64; n];
    for (i, &c) in a.iter().enumerate() {
        out[i] += c;
    }
    for (i, &c) in b.iter().enumerate() {
        out[i] += c;
    }
    out
}
