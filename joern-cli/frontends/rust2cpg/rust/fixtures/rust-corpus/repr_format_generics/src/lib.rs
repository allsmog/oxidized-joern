#[repr(C)]
pub struct Header {
    a: u8,
    b: u32,
    c: u16,
}

#[non_exhaustive]
pub enum Event {
    Start,
    Stop,
    Tick(u64),
}

pub fn pair<T, U>(t: T, u: U) -> (T, U) {
    (t, u)
}

pub fn typed_pair() -> (i32, String) {
    pair(1, "x".to_string())
}

pub fn render(name: &str, n: i32) -> String {
    format!("{0} {name} {n:>5} {1:.2}", "pre", 3.14159)
}

pub fn swap_pair<A, B>(input: (A, B)) -> (B, A) {
    let (a, b) = input;
    (b, a)
}
