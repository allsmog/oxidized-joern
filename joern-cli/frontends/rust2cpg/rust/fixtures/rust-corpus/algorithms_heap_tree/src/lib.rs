use std::collections::{BinaryHeap, VecDeque};

pub fn heapsort(v: Vec<i32>) -> Vec<i32> {
    let mut heap: BinaryHeap<i32> = v.into_iter().collect();
    let mut out = Vec::with_capacity(heap.len());
    while let Some(x) = heap.pop() {
        out.push(x);
    }
    out.reverse();
    out
}

pub struct Node {
    val: i32,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

pub fn level_order(root: &Node) -> Vec<i32> {
    let mut out = Vec::new();
    let mut q: VecDeque<&Node> = VecDeque::new();
    q.push_back(root);
    while let Some(n) = q.pop_front() {
        out.push(n.val);
        if let Some(l) = &n.left {
            q.push_back(l);
        }
        if let Some(r) = &n.right {
            q.push_back(r);
        }
    }
    out
}

pub fn merge_intervals(mut intervals: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    intervals.sort();
    let mut out: Vec<(i32, i32)> = Vec::new();
    for (start, end) in intervals {
        if let Some(last) = out.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        out.push((start, end));
    }
    out
}

pub fn multiply(a: &[Vec<i32>], b: &[Vec<i32>]) -> Vec<Vec<i32>> {
    let n = a.len();
    let m = b[0].len();
    let k = b.len();
    let mut out = vec![vec![0; m]; n];
    for i in 0..n {
        for j in 0..m {
            for x in 0..k {
                out[i][j] += a[i][x] * b[x][j];
            }
        }
    }
    out
}
