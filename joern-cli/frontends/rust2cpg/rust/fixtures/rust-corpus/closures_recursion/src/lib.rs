pub fn make_counter() -> impl FnMut() -> i32 {
    let mut count = 0;
    move || {
        count += 1;
        count
    }
}

pub fn compose() -> impl Fn(i32) -> i32 {
    let inc = |x: i32| x + 1;
    move |x| inc(x) * 2
}

pub fn fib(n: u64) -> u64 {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

pub fn factorial(n: u64) -> u64 {
    (1..=n).product()
}

pub fn shadow(x: i32) -> String {
    let x = x + 1;
    let x = x * 2;
    let x = format!("value={}", x);
    x
}

pub fn parse_all(inputs: &[&str]) -> Result<Vec<i32>, std::num::ParseIntError> {
    inputs.iter().map(|s| s.parse::<i32>()).collect()
}
