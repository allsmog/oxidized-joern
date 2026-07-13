use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub fn traced(x: i32) -> i32 {
    let y = dbg!(x * 2);
    eprintln!("computed {}", y);
    print!("{}", y);
    y
}

pub struct Counter {
    count: Cell<u32>,
}

impl Counter {
    pub fn new() -> Self {
        Counter { count: Cell::new(0) }
    }
    pub fn bump(&self) {
        self.count.set(self.count.get() + 1);
    }
    pub fn value(&self) -> u32 {
        self.count.get()
    }
}

pub struct TreeNode {
    parent: RefCell<Weak<TreeNode>>,
    value: i32,
}

pub fn make_node(value: i32) -> Rc<TreeNode> {
    Rc::new(TreeNode {
        parent: RefCell::new(Weak::new()),
        value,
    })
}

pub fn node_value(node: &Rc<TreeNode>) -> i32 {
    node.value
}
