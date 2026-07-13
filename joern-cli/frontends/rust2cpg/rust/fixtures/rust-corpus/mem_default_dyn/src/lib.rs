#[derive(Default, Debug)]
pub struct Config {
    pub name: String,
    pub retries: u32,
    pub verbose: bool,
}

pub fn with_retries() -> Config {
    Config {
        retries: 3,
        ..Default::default()
    }
}

pub fn swap_ends(a: &mut i32, b: &mut i32) {
    std::mem::swap(a, b);
}

pub fn drain(values: &mut Vec<i32>) -> Vec<i32> {
    std::mem::take(values)
}

pub fn reset(slot: &mut String) -> String {
    std::mem::replace(slot, String::new())
}

pub struct Handler {
    callback: Box<dyn Fn(i32) -> i32>,
}

impl Handler {
    pub fn new(f: Box<dyn Fn(i32) -> i32>) -> Self {
        Handler { callback: f }
    }
    pub fn call(&self, x: i32) -> i32 {
        (self.callback)(x)
    }
}

pub struct Parser<'a, T> {
    input: &'a str,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T> Parser<'a, T> {
    pub fn new(input: &'a str) -> Self {
        Parser {
            input,
            _marker: std::marker::PhantomData,
        }
    }
    pub fn raw(&self) -> &'a str {
        self.input
    }
}
