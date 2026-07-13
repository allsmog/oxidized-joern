use std::collections::VecDeque;
use std::fmt::Write;

pub fn queue_roundtrip() -> i32 {
    let mut q: VecDeque<i32> = VecDeque::new();
    q.push_back(1);
    q.push_front(0);
    q.push_back(2);
    let front = q.pop_front().unwrap_or(0);
    front + q.len() as i32
}

pub fn pairwise_sums(values: &[i32]) -> Vec<i32> {
    values.windows(2).map(|w| w[0] + w[1]).collect()
}

pub fn chunk_count(values: &[i32]) -> usize {
    values.chunks(3).count()
}

pub fn is_positive(x: Option<i32>) -> bool {
    matches!(x, Some(n) if n > 0)
}

pub fn render(items: &[i32]) -> String {
    let mut out = String::new();
    for (i, x) in items.iter().enumerate() {
        writeln!(out, "{i}: {x}").unwrap();
    }
    out
}
