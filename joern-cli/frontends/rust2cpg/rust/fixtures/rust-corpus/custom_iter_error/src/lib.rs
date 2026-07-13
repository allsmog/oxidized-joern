use std::error::Error;

pub struct Fib {
    a: u64,
    b: u64,
}

impl Iterator for Fib {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        let current = self.a;
        self.a = self.b;
        self.b = current + self.b;
        Some(current)
    }
}

pub fn even_fibs() -> Vec<u64> {
    let fib = Fib { a: 0, b: 1 };
    fib.take(10).filter(|x| x % 2 == 0).collect()
}

pub fn parse_double(s: &str) -> Result<i32, Box<dyn Error>> {
    let n: i32 = s.trim().parse()?;
    Ok(n * 2)
}

pub fn sum_lines(text: &str) -> Result<i32, Box<dyn Error>> {
    let mut total = 0;
    for line in text.lines() {
        total += line.trim().parse::<i32>()?;
    }
    Ok(total)
}
