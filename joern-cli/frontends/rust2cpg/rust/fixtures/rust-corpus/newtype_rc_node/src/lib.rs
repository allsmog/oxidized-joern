use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

pub struct Stack(Vec<i32>);

impl Deref for Stack {
    type Target = Vec<i32>;
    fn deref(&self) -> &Vec<i32> {
        &self.0
    }
}

impl Stack {
    pub fn new() -> Self {
        Stack(Vec::new())
    }
    pub fn top(&self) -> Option<&i32> {
        self.last()
    }
}

pub struct Node {
    pub value: i32,
    pub next: Option<Rc<RefCell<Node>>>,
}

pub fn linked() -> i32 {
    let a = Rc::new(RefCell::new(Node {
        value: 1,
        next: None,
    }));
    let b = Rc::new(RefCell::new(Node {
        value: 2,
        next: Some(Rc::clone(&a)),
    }));
    a.borrow_mut().value += 10;
    let head = b.borrow().value;
    let tail = b.borrow().next.as_ref().map(|n| n.borrow().value).unwrap_or(0);
    head + tail
}
