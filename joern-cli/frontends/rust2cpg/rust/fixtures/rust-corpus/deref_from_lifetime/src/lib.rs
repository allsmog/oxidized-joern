use std::fmt;
use std::ops::{Deref, DerefMut};

pub struct Stack {
    inner: Vec<i32>,
}

impl Deref for Stack {
    type Target = Vec<i32>;
    fn deref(&self) -> &Vec<i32> {
        &self.inner
    }
}

impl DerefMut for Stack {
    fn deref_mut(&mut self) -> &mut Vec<i32> {
        &mut self.inner
    }
}

pub struct Name(String);

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Name(s.to_string())
    }
}

pub fn named() -> Name {
    Name::from("alice")
}

pub struct Pair<'a, 'b> {
    first: &'a str,
    second: &'b str,
}

impl<'a, 'b> Pair<'a, 'b> {
    pub fn new(first: &'a str, second: &'b str) -> Self {
        Pair { first, second }
    }
    pub fn first(&self) -> &'a str {
        self.first
    }
    pub fn second(&self) -> &'b str {
        self.second
    }
}

pub const N: usize = 4;

pub fn zeros() -> [i32; N] {
    [0; N]
}

pub fn ones() -> [i32; 2 * 3] {
    [1; 6]
}

pub enum Color {
    Red,
    Green,
    Blue,
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            Color::Red => "red",
            Color::Green => "green",
            Color::Blue => "blue",
        };
        write!(f, "{}", s)
    }
}
