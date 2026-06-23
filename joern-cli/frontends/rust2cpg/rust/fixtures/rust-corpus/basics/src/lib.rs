//! Small fixture corpus consumed by the differential JSON harness.
//!
//! It mirrors the breadth of the inline coverage fixture so a reference
//! `rust_ast_gen` and this crate's CLI can be compared node-for-node.

pub struct Counter {
    value: i32,
}

impl Counter {
    pub fn new() -> Self {
        Counter { value: 0 }
    }

    pub fn bump(&mut self) -> i32 {
        self.value += 1;
        self.value
    }
}

impl Default for Counter {
    fn default() -> Self {
        Counter::new()
    }
}

pub enum Op {
    Add(i32),
    Sub(i32),
    Noop,
}

pub fn apply(op: &Op, acc: i32) -> i32 {
    match op {
        Op::Add(n) => acc + n,
        Op::Sub(n) => acc - n,
        Op::Noop => acc,
    }
}

pub fn greet(name: &str) -> String {
    let mut out = String::new();
    out.push_str("hi, ");
    out.push_str(name);
    out.trim().to_string()
}

pub fn sum(values: &[i32]) -> i32 {
    let mut total = 0;
    for value in values {
        total += value;
    }
    total
}
