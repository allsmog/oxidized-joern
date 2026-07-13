use std::marker::PhantomData;
use std::ops::Deref;

pub struct Holder<'a> {
    value: &'a i32,
}

impl<'a> Holder<'a> {
    pub fn new(value: &'a i32) -> Self {
        Holder { value }
    }
    pub fn get(&self) -> &'a i32 {
        self.value
    }
}

pub fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}

pub fn call_with<F>(f: F) -> i32
where
    F: for<'a> Fn(&'a i32) -> i32,
{
    let n = 5;
    f(&n)
}

pub struct Typed<T> {
    raw: i32,
    _marker: PhantomData<T>,
}

impl<T> Typed<T> {
    pub fn new(raw: i32) -> Self {
        Typed {
            raw,
            _marker: PhantomData,
        }
    }
    pub fn raw(&self) -> i32 {
        self.raw
    }
}

pub struct MyBox<T>(T);

impl<T> Deref for MyBox<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

pub fn deref_chain() -> i32 {
    let b = MyBox(MyBox(9));
    **b
}

pub fn nested_generics() -> Vec<Vec<Box<i32>>> {
    let inner: Vec<Box<i32>> = vec![Box::new(1), Box::new(2)];
    vec![inner]
}
