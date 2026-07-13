use std::fmt;

pub struct Fib {
    a: u64,
    b: u64,
}

impl Iterator for Fib {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        let r = self.a;
        self.a = self.b;
        self.b = r + self.b;
        Some(r)
    }
}

pub fn fib_iter() -> Fib {
    Fib { a: 0, b: 1 }
}

pub struct Point {
    x: i32,
    y: i32,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

pub struct Celsius(f64);
pub struct Fahrenheit(f64);

impl From<Celsius> for Fahrenheit {
    fn from(c: Celsius) -> Self {
        Fahrenheit(c.0 * 9.0 / 5.0 + 32.0)
    }
}

#[derive(Debug)]
pub enum ParseError {
    Empty,
    Invalid(char),
    TooLong(usize),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "empty"),
            ParseError::Invalid(c) => write!(f, "invalid char {}", c),
            ParseError::TooLong(n) => write!(f, "too long: {}", n),
        }
    }
}

impl std::error::Error for ParseError {}

pub enum State {
    Idle,
    Running,
    Paused,
    Stopped,
}

pub fn transition(s: State, event: &str) -> State {
    match (s, event) {
        (State::Idle, "start") => State::Running,
        (State::Running, "pause") => State::Paused,
        (State::Paused, "resume") => State::Running,
        (State::Running, "stop") | (State::Paused, "stop") => State::Stopped,
        (other, _) => other,
    }
}
