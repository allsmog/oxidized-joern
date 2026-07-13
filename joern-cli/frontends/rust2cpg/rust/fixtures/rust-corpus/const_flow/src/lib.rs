pub const MAX: usize = 100;
pub static NAME: &str = "joern";

pub const fn square(x: i32) -> i32 {
    x * x
}

pub const COMPUTED: i32 = square(4);

pub fn never_returns() -> ! {
    panic!("unreachable")
}

pub fn unwrap_or_die(x: Option<i32>) -> i32 {
    match x {
        Some(v) => v,
        None => never_returns(),
    }
}

pub fn chained(opt: Option<i32>) -> i32 {
    let Some(x) = opt else {
        return -1;
    };
    if let Some(y) = Some(x + 1) {
        y
    } else {
        0
    }
}

pub fn ref_destructure(pair: &(i32, String)) -> i32 {
    let (ref a, ref _b) = *pair;
    *a
}

pub fn formatted(name: &str, n: i32) -> String {
    let mut s = String::new();
    s.push_str(&format!("{name}={n}"));
    assert!(n >= 0, "must be non-negative");
    s
}
