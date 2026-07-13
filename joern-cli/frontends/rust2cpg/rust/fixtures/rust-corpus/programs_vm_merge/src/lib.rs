use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub enum Op {
    Push(i64),
    Add,
    Mul,
    Dup,
}

pub fn run(prog: &[Op]) -> Option<i64> {
    let mut stack: Vec<i64> = Vec::new();
    for op in prog {
        match op {
            Op::Push(n) => stack.push(*n),
            Op::Add => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(a + b);
            }
            Op::Mul => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(a * b);
            }
            Op::Dup => {
                let x = *stack.last()?;
                stack.push(x);
            }
        }
    }
    stack.pop()
}

pub fn merge_k(lists: Vec<Vec<i32>>) -> Vec<i32> {
    let mut heap: BinaryHeap<Reverse<(i32, usize, usize)>> = BinaryHeap::new();
    for (i, list) in lists.iter().enumerate() {
        if !list.is_empty() {
            heap.push(Reverse((list[0], i, 0)));
        }
    }
    let mut out = Vec::new();
    while let Some(Reverse((val, li, ei))) = heap.pop() {
        out.push(val);
        if ei + 1 < lists[li].len() {
            heap.push(Reverse((lists[li][ei + 1], li, ei + 1)));
        }
    }
    out
}

pub fn next_permutation(v: &mut Vec<i32>) -> bool {
    if v.len() < 2 {
        return false;
    }
    let mut i = v.len() - 1;
    while i > 0 && v[i - 1] >= v[i] {
        i -= 1;
    }
    if i == 0 {
        v.reverse();
        return false;
    }
    let mut j = v.len() - 1;
    while v[j] <= v[i - 1] {
        j -= 1;
    }
    v.swap(i - 1, j);
    v[i..].reverse();
    true
}

pub fn parse_ini(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if let Some((k, v)) = line.split_once('=') {
            map.insert(format!("{}.{}", section, k.trim()), v.trim().to_string());
        }
    }
    map
}
