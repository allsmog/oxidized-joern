pub enum Expr {
    Num(i32),
    Add(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
}

pub fn eval(e: &Expr) -> i32 {
    match e {
        Expr::Num(n) => *n,
        Expr::Add(a, b) => eval(a) + eval(b),
        Expr::Mul(a, b) => eval(a) * eval(b),
    }
}

pub fn pipeline(x: i32) -> i32 {
    let ops: Vec<Box<dyn Fn(i32) -> i32>> = vec![
        Box::new(|n| n + 1),
        Box::new(|n| n * 2),
        Box::new(|n| n - 3),
    ];
    ops.iter().fold(x, |acc, op| op(acc))
}

pub fn edit(mut v: Vec<i32>) -> Vec<i32> {
    v.insert(0, 99);
    if v.len() > 2 {
        v.remove(2);
    }
    v.retain(|&x| x != 0);
    v
}

pub fn build_string() -> String {
    let mut s = String::from("hello");
    s.push('!');
    s.push_str(" world");
    s.insert(0, '>');
    s.truncate(8);
    s
}

pub fn is_sorted(v: &[i32]) -> bool {
    v.windows(2).all(|w| w[0] <= w[1])
}

pub fn reverse_chars(s: &str) -> String {
    s.chars().rev().collect()
}

pub fn wide(a: i128, b: u128) -> i128 {
    (a * 1_000_000_000_000) + (b as i128)
}
