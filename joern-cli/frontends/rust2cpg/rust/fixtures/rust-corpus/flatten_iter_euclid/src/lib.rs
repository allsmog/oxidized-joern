use std::fmt::Display;

pub fn flatten_opt(x: Option<Option<i32>>) -> Option<i32> {
    x.flatten()
}

pub fn transpose_res(x: Result<Option<i32>, String>) -> Option<Result<i32, String>> {
    x.transpose()
}

pub fn join<T, U>(a: T, b: U) -> String
where
    T: Display,
    U: Display,
{
    format!("{}-{}", a, b)
}

pub fn squares() -> Vec<u32> {
    let mut state = 0u32;
    std::iter::from_fn(move || {
        state += 1;
        if state <= 5 {
            Some(state * state)
        } else {
            None
        }
    })
    .collect()
}

pub fn euclidean(a: i32, b: i32) -> (i32, i32) {
    (a.rem_euclid(b), a.div_euclid(b))
}

pub fn float_ops(x: f64) -> f64 {
    x.signum() + x.copysign(-1.0) + x.clamp(0.0, 10.0)
}

pub fn padded(mut v: Vec<i32>) -> Vec<i32> {
    v.resize(5, 0);
    v.extend_from_slice(&[7, 8]);
    v
}

pub fn conditional(cond: bool, x: i32) -> Option<i32> {
    cond.then(|| x * 2)
}
