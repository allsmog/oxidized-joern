use std::fmt::Debug;

pub trait Animal {
    fn name(&self) -> String;
    fn legs(&self) -> u32 {
        4
    }
}

pub struct Dog {
    pub nick: String,
}

impl Animal for Dog {
    fn name(&self) -> String {
        self.nick.clone()
    }
}

pub trait Container {
    type Item;
    const CAPACITY: usize;
    fn first(&self) -> Option<&Self::Item>;
}

pub struct Stack<T> {
    items: Vec<T>,
}

impl<T: Clone + Debug> Stack<T> {
    pub fn new() -> Self {
        Stack { items: Vec::new() }
    }
    pub fn push(&mut self, value: T) {
        self.items.push(value);
    }
    pub fn peek(&self) -> Option<&T> {
        self.items.last()
    }
}

pub fn largest<T: PartialOrd + Copy>(list: &[T]) -> T {
    let mut max = list[0];
    for &item in list {
        if item > max {
            max = item;
        }
    }
    max
}

pub fn make_adder(x: i32) -> impl Fn(i32) -> i32 {
    move |y| x + y
}

pub fn describe<T: Debug>(value: &T) -> String {
    format!("{:?}", value)
}
