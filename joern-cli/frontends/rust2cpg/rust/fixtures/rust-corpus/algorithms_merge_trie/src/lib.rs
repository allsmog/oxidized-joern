use std::collections::{HashMap, VecDeque};

pub fn merge_sort(v: &[i32]) -> Vec<i32> {
    if v.len() <= 1 {
        return v.to_vec();
    }
    let mid = v.len() / 2;
    let left = merge_sort(&v[..mid]);
    let right = merge_sort(&v[mid..]);
    let mut out = Vec::with_capacity(v.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if left[i] <= right[j] {
            out.push(left[i]);
            i += 1;
        } else {
            out.push(right[j]);
            j += 1;
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    out
}

#[derive(Default)]
pub struct Trie {
    children: HashMap<char, Trie>,
    end: bool,
}

impl Trie {
    pub fn insert(&mut self, word: &str) {
        let mut node = self;
        for c in word.chars() {
            node = node.children.entry(c).or_default();
        }
        node.end = true;
    }
    pub fn contains(&self, word: &str) -> bool {
        let mut node = self;
        for c in word.chars() {
            match node.children.get(&c) {
                Some(n) => node = n,
                None => return false,
            }
        }
        node.end
    }
}

pub fn max_window(nums: &[i32], k: usize) -> Vec<i32> {
    let mut dq: VecDeque<usize> = VecDeque::new();
    let mut out = Vec::new();
    for i in 0..nums.len() {
        while let Some(&front) = dq.front() {
            if front + k <= i {
                dq.pop_front();
            } else {
                break;
            }
        }
        while let Some(&back) = dq.back() {
            if nums[back] <= nums[i] {
                dq.pop_back();
            } else {
                break;
            }
        }
        dq.push_back(i);
        if i + 1 >= k {
            out.push(nums[*dq.front().unwrap()]);
        }
    }
    out
}

pub fn rotate90(m: &[[i32; 3]; 3]) -> [[i32; 3]; 3] {
    let mut out = [[0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[j][2 - i] = m[i][j];
        }
    }
    out
}
