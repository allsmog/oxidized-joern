use std::collections::{HashMap, VecDeque};

pub fn is_palindrome(s: &str) -> bool {
    let chars: Vec<char> = s
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    chars.iter().eq(chars.iter().rev())
}

pub fn top_word(text: &str) -> Option<String> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for w in text.split_whitespace() {
        *counts.entry(w).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(w, _)| w.to_string())
}

pub fn insert_sorted(v: &mut Vec<i32>, x: i32) {
    let pos = v.binary_search(&x).unwrap_or_else(|e| e);
    v.insert(pos, x);
}

pub fn sum_all<I: Iterator<Item = i32>>(iter: I) -> i32 {
    iter.fold(0, |a, b| a + b)
}

pub fn sum_vec() -> i32 {
    sum_all(vec![1, 2, 3].into_iter())
}

pub struct Lru {
    cap: usize,
    items: VecDeque<i32>,
}

impl Lru {
    pub fn new(cap: usize) -> Self {
        Lru {
            cap,
            items: VecDeque::new(),
        }
    }
    pub fn access(&mut self, x: i32) {
        if let Some(pos) = self.items.iter().position(|&v| v == x) {
            self.items.remove(pos);
        } else if self.items.len() == self.cap {
            self.items.pop_back();
        }
        self.items.push_front(x);
    }
}
