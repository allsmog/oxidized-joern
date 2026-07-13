use std::collections::{BinaryHeap, HashMap};

pub fn simplify(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                stack.pop();
            }
            _ => stack.push(part),
        }
    }
    format!("/{}", stack.join("/"))
}

pub fn top_k(nums: &[i32], k: usize) -> Vec<i32> {
    let mut counts: HashMap<i32, u32> = HashMap::new();
    for &n in nums {
        *counts.entry(n).or_insert(0) += 1;
    }
    let mut pairs: Vec<(i32, u32)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));
    pairs.into_iter().take(k).map(|(n, _)| n).collect()
}

pub fn partition_labels(s: &str) -> Vec<usize> {
    let bytes = s.as_bytes();
    let mut last: HashMap<u8, usize> = HashMap::new();
    for (i, &b) in bytes.iter().enumerate() {
        last.insert(b, i);
    }
    let mut out = Vec::new();
    let (mut start, mut end) = (0, 0);
    for (i, &b) in bytes.iter().enumerate() {
        end = end.max(last[&b]);
        if i == end {
            out.push(end - start + 1);
            start = i + 1;
        }
    }
    out
}

pub fn insert(intervals: Vec<(i32, i32)>, new: (i32, i32)) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let (mut s, mut e) = new;
    let mut placed = false;
    for (cs, ce) in intervals {
        if ce < s {
            out.push((cs, ce));
        } else if cs > e {
            if !placed {
                out.push((s, e));
                placed = true;
            }
            out.push((cs, ce));
        } else {
            s = s.min(cs);
            e = e.max(ce);
        }
    }
    if !placed {
        out.push((s, e));
    }
    out
}

pub fn reorganize(s: &str) -> String {
    let mut counts: HashMap<char, i32> = HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0) += 1;
    }
    let mut heap: BinaryHeap<(i32, char)> = counts.into_iter().map(|(c, n)| (n, c)).collect();
    let mut out = String::new();
    let mut prev: Option<(i32, char)> = None;
    while let Some((n, c)) = heap.pop() {
        out.push(c);
        if let Some((pn, pc)) = prev.take() {
            if pn > 0 {
                heap.push((pn, pc));
            }
        }
        prev = Some((n - 1, c));
    }
    out
}
