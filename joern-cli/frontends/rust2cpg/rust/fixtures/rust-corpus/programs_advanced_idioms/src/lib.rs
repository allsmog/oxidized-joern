use std::cell::RefCell;
use std::rc::Rc;

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Circle {
    r: f64,
}

pub struct Square {
    s: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.s * self.s
    }
}

pub fn total_area(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

pub struct Node {
    pub val: i32,
    pub next: Option<Rc<RefCell<Node>>>,
}

pub fn sum_list(head: &Option<Rc<RefCell<Node>>>) -> i32 {
    let mut total = 0;
    let mut cur = head.clone();
    while let Some(node) = cur {
        total += node.borrow().val;
        cur = node.borrow().next.clone();
    }
    total
}

pub fn make_counter() -> impl FnMut() -> u32 {
    let mut count = 0;
    move || {
        count += 1;
        count
    }
}

pub fn apply_n<F: FnMut()>(mut f: F, n: u32) {
    for _ in 0..n {
        f();
    }
}

pub fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}

pub struct Holder<'a> {
    part: &'a str,
}

impl<'a> Holder<'a> {
    pub fn new(s: &'a str) -> Self {
        Holder { part: s }
    }
    pub fn get(&self) -> &str {
        self.part
    }
}

pub fn process(a: &[i32], b: &[i32]) -> Vec<(usize, i32)> {
    a.iter()
        .chain(b.iter())
        .enumerate()
        .filter(|(i, _)| i % 2 == 0)
        .map(|(i, &x)| (i, x * 2))
        .collect()
}

pub fn running_sum(v: &[i32]) -> Vec<i32> {
    v.iter()
        .scan(0, |acc, &x| {
            *acc += x;
            Some(*acc)
        })
        .collect()
}
