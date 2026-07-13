use std::cell::{Cell, RefCell};
use std::fmt;

pub fn split_counts(s: &str) -> usize {
    let terms: Vec<&str> = s.split_terminator(';').collect();
    let indices: Vec<(usize, char)> = s.char_indices().collect();
    let rev_lines: Vec<&str> = s.lines().rev().collect();
    terms.len() + indices.len() + rev_lines.len()
}

pub struct State {
    counter: Cell<i32>,
    items: RefCell<Vec<i32>>,
}

pub fn snapshot(s: &State) -> i32 {
    let old = s.counter.take();
    if let Ok(v) = s.items.try_borrow() {
        old + v.len() as i32
    } else {
        old
    }
}

pub struct Wrapper(i32);

impl fmt::Display for &Wrapper {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "W({})", self.0)
    }
}

pub trait Named {
    fn name(&self) -> String;
}

pub trait Describe {
    fn describe(&self) -> String;
}

impl<T: Named> Describe for T {
    fn describe(&self) -> String {
        format!("named: {}", self.name())
    }
}
