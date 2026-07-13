use std::hash::{Hash, Hasher};

pub static GREETING: &str = "hello";
pub static TABLE: [i32; 3] = [1, 2, 3];
static mut COUNTER: u32 = 0;

pub fn bump() -> u32 {
    unsafe {
        COUNTER += 1;
        COUNTER
    }
}

pub struct Id {
    major: u32,
    minor: u32,
}

impl Hash for Id {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.major.hash(state);
        self.minor.hash(state);
    }
}

pub fn clear(v: &mut Vec<i32>) {
    v.as_mut_slice().fill(0);
}

pub fn own(s: &str) -> String {
    s.to_owned()
}

pub fn convert<T: Into<String>>(t: T) -> String {
    t.into()
}

pub struct Tree {
    value: i32,
    left: Option<Box<Tree>>,
    right: Option<Box<Tree>>,
}

impl Tree {
    pub fn leaf(value: i32) -> Self {
        Tree {
            value,
            left: None,
            right: None,
        }
    }
    pub fn insert(&mut self, v: i32) {
        if v < self.value {
            match &mut self.left {
                Some(n) => n.insert(v),
                None => self.left = Some(Box::new(Tree::leaf(v))),
            }
        } else {
            match &mut self.right {
                Some(n) => n.insert(v),
                None => self.right = Some(Box::new(Tree::leaf(v))),
            }
        }
    }
}
