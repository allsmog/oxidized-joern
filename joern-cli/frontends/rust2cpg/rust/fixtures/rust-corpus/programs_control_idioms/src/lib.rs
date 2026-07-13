pub fn classify(v: &[i32]) -> &str {
    match v {
        [] => "empty",
        [_] => "single",
        [first, .., last] if first == last => "palindrome-ends",
        [a, b] => {
            if a < b {
                "ascending-pair"
            } else {
                "other-pair"
            }
        }
        [head, tail @ ..] => {
            if tail.contains(head) {
                "head-repeats"
            } else {
                "general"
            }
        }
    }
}

pub fn drain_positive(stack: &mut Vec<i32>) -> i32 {
    let mut sum = 0;
    while let Some(&top) = stack.last() {
        if top <= 0 {
            break;
        }
        sum += top;
        stack.pop();
    }
    sum
}

pub fn parse_or_default(s: &str) -> i32 {
    if let Ok(n) = s.parse::<i32>() {
        n
    } else if let Some(stripped) = s.strip_prefix("0x") {
        i32::from_str_radix(stripped, 16).unwrap_or(0)
    } else {
        -1
    }
}

pub fn compose<A, B, C>(f: impl Fn(A) -> B, g: impl Fn(B) -> C) -> impl Fn(A) -> C {
    move |x| g(f(x))
}

pub fn adder(n: i32) -> impl Fn(i32) -> i32 {
    move |x| x + n
}

pub enum Op {
    Add(i64),
    Sub(i64),
    Mul(i64),
    Neg,
}

impl Op {
    pub fn apply(&self, acc: i64) -> i64 {
        match self {
            Op::Add(n) => acc + n,
            Op::Sub(n) => acc - n,
            Op::Mul(n) => acc * n,
            Op::Neg => -acc,
        }
    }
}

pub fn run(ops: &[Op], start: i64) -> i64 {
    ops.iter().fold(start, |acc, op| op.apply(acc))
}
