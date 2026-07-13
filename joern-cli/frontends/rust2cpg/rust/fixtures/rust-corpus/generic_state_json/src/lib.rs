use std::collections::HashMap;
use std::fmt;
use std::ops::Add;

pub fn sum_default<T: Add<Output = T> + Copy + Default>(items: &[T]) -> T {
    let mut acc = T::default();
    for &x in items {
        acc = acc + x;
    }
    acc
}

pub fn scale(m: &mut HashMap<String, i32>) {
    for (_, v) in m.iter_mut() {
        *v *= 10;
    }
}

pub fn count_ab(input: &str) -> u32 {
    let mut state = 0u32;
    let mut count = 0u32;
    for c in input.chars() {
        state = match (state, c) {
            (0, 'a') => 1,
            (1, 'b') => {
                count += 1;
                0
            }
            _ => 0,
        };
    }
    count
}

pub fn sum_lines(text: &str) -> i32 {
    text.lines()
        .filter_map(|l| l.trim().parse::<i32>().ok())
        .sum()
}

pub fn concat_rows(rows: &[Vec<i32>]) -> Vec<i32> {
    rows.concat()
}

pub enum Json {
    Null,
    Num(f64),
    Arr(Vec<Json>),
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Json::Null => write!(f, "null"),
            Json::Num(n) => write!(f, "{}", n),
            Json::Arr(items) => {
                write!(f, "[")?;
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "{}", it)?;
                }
                write!(f, "]")
            }
        }
    }
}
