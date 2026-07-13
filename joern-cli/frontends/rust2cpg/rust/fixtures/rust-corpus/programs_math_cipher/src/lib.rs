use std::collections::HashMap;

pub struct Fraction {
    num: i64,
    den: i64,
}

impl Fraction {
    fn gcd(a: i64, b: i64) -> i64 {
        if b == 0 {
            a.abs()
        } else {
            Self::gcd(b, a % b)
        }
    }
    pub fn reduce(self) -> Fraction {
        let g = Self::gcd(self.num, self.den);
        Fraction {
            num: self.num / g,
            den: self.den / g,
        }
    }
    pub fn add(self, o: Fraction) -> Fraction {
        Fraction {
            num: self.num * o.den + o.num * self.den,
            den: self.den * o.den,
        }
        .reduce()
    }
}

pub struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    pub fn mul(&self, o: &Complex) -> Complex {
        Complex {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }
    pub fn abs(&self) -> f64 {
        (self.re * self.re + self.im * self.im).sqrt()
    }
}

pub fn vigenere(text: &str, key: &str) -> String {
    let key: Vec<u8> = key
        .bytes()
        .filter(|b| b.is_ascii_alphabetic())
        .map(|b| b.to_ascii_lowercase() - b'a')
        .collect();
    if key.is_empty() {
        return text.to_string();
    }
    let mut ki = 0;
    text.chars()
        .map(|c| {
            if c.is_ascii_lowercase() {
                let shifted = (((c as u8 - b'a') + key[ki % key.len()]) % 26) + b'a';
                ki += 1;
                shifted as char
            } else {
                c
            }
        })
        .collect()
}

pub fn to_morse(s: &str) -> String {
    let map: HashMap<char, &str> = [('a', ".-"), ('b', "-..."), ('e', "."), ('t', "-")]
        .into_iter()
        .collect();
    s.to_lowercase()
        .chars()
        .filter_map(|c| map.get(&c).copied())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn digit_sum(mut n: u64) -> u64 {
    let mut s = 0;
    while n > 0 {
        s += n % 10;
        n /= 10;
    }
    s
}

pub fn is_armstrong(n: u32) -> bool {
    let digits: Vec<u32> = n
        .to_string()
        .chars()
        .map(|c| c.to_digit(10).unwrap())
        .collect();
    let len = digits.len() as u32;
    digits.iter().map(|&d| d.pow(len)).sum::<u32>() == n
}
