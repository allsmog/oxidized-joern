use std::rc::{Rc, Weak};

pub fn boxed_slice(v: Vec<i32>) -> Box<[i32]> {
    v.into_boxed_slice()
}

pub fn boxed_str(s: String) -> Box<str> {
    s.into_boxed_str()
}

pub fn weak_value() -> i32 {
    let strong = Rc::new(42);
    let weak: Weak<i32> = Rc::downgrade(&strong);
    weak.upgrade().map(|r| *r).unwrap_or(0)
}

pub fn sizes() -> usize {
    std::mem::size_of::<i64>()
        + std::mem::align_of::<u32>()
        + std::mem::size_of::<[u8; 16]>()
}

pub fn option_moves() -> (Option<i32>, Option<i32>) {
    let mut a = Some(5);
    let taken = a.take();
    let mut b: Option<i32> = None;
    let old = b.replace(9);
    (taken, old)
}

pub fn convert(x: i32) -> i32 {
    std::convert::identity(x)
}
