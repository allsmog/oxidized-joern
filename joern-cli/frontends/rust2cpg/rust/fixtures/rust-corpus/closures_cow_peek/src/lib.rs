use std::borrow::Cow;

pub fn adder(x: i32) -> impl Fn(i32) -> Box<dyn Fn(i32) -> i32> {
    move |y| {
        let sum = x + y;
        Box::new(move |z| sum + z)
    }
}

pub fn ensure_owned(input: Cow<str>) -> String {
    input.into_owned()
}

pub fn adjacent_products(values: &[i32]) -> i32 {
    let mut it = values.iter().peekable();
    let mut total = 0;
    while let Some(&x) = it.next() {
        if let Some(&&next) = it.peek() {
            total += x * next;
        }
    }
    total
}

pub fn compose_all() -> i32 {
    let make = adder(1);
    let inner = make(2);
    inner(3)
}
