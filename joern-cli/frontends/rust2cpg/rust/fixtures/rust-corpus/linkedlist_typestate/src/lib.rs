use std::marker::PhantomData;

pub struct Node {
    value: i32,
    next: Option<Box<Node>>,
}

pub struct List {
    head: Option<Box<Node>>,
}

impl List {
    pub fn new() -> Self {
        List { head: None }
    }
    pub fn push(&mut self, value: i32) {
        let node = Box::new(Node {
            value,
            next: self.head.take(),
        });
        self.head = Some(node);
    }
    pub fn sum(&self) -> i32 {
        let mut total = 0;
        let mut cur = &self.head;
        while let Some(n) = cur {
            total += n.value;
            cur = &n.next;
        }
        total
    }
}

pub struct Locked;
pub struct Unlocked;

pub struct Door<S> {
    _state: PhantomData<S>,
}

impl Door<Locked> {
    pub fn new() -> Self {
        Door {
            _state: PhantomData,
        }
    }
    pub fn unlock(self) -> Door<Unlocked> {
        Door {
            _state: PhantomData,
        }
    }
}

impl Door<Unlocked> {
    pub fn open(&self) -> bool {
        true
    }
}

pub struct A(i32);
pub struct B(i32);
pub struct C(i32);

impl From<A> for B {
    fn from(a: A) -> B {
        B(a.0 + 1)
    }
}

impl From<B> for C {
    fn from(b: B) -> C {
        C(b.0 * 2)
    }
}

pub fn convert(a: A) -> C {
    let b: B = a.into();
    b.into()
}
