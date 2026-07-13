use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

#[derive(Default)]
pub struct Trie {
    children: HashMap<char, Trie>,
    count: u32,
}

impl Trie {
    pub fn insert(&mut self, word: &str) {
        let mut node = self;
        for c in word.chars() {
            node = node.children.entry(c).or_default();
            node.count += 1;
        }
    }
    pub fn prefix_count(&self, prefix: &str) -> u32 {
        let mut node = self;
        for c in prefix.chars() {
            match node.children.get(&c) {
                Some(n) => node = n,
                None => return 0,
            }
        }
        node.count
    }
}

pub struct SegTree {
    n: usize,
    tree: Vec<i64>,
}

impl SegTree {
    pub fn new(data: &[i64]) -> Self {
        let n = data.len();
        let mut tree = vec![0; 2 * n];
        tree[n..(2 * n)].copy_from_slice(data);
        for i in (1..n).rev() {
            tree[i] = tree[2 * i] + tree[2 * i + 1];
        }
        SegTree { n, tree }
    }
    pub fn query(&self, mut l: usize, mut r: usize) -> i64 {
        let mut sum = 0;
        l += self.n;
        r += self.n;
        while l < r {
            if l & 1 == 1 {
                sum += self.tree[l];
                l += 1;
            }
            if r & 1 == 1 {
                r -= 1;
                sum += self.tree[r];
            }
            l >>= 1;
            r >>= 1;
        }
        sum
    }
}

pub struct Store {
    data: HashMap<String, String>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            data: HashMap::new(),
        }
    }
    pub fn set(&mut self, k: &str, v: &str) {
        self.data.insert(k.to_string(), v.to_string());
    }
    pub fn get(&self, k: &str) -> Option<&String> {
        self.data.get(k)
    }
    pub fn delete(&mut self, k: &str) -> bool {
        self.data.remove(k).is_some()
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

pub struct MovingAverage {
    size: usize,
    window: VecDeque<i32>,
    sum: i64,
}

impl MovingAverage {
    pub fn new(size: usize) -> Self {
        MovingAverage {
            size,
            window: VecDeque::new(),
            sum: 0,
        }
    }
    pub fn next(&mut self, val: i32) -> f64 {
        self.window.push_back(val);
        self.sum += val as i64;
        if self.window.len() > self.size {
            if let Some(old) = self.window.pop_front() {
                self.sum -= old as i64;
            }
        }
        self.sum as f64 / self.window.len() as f64
    }
}

pub struct MedianFinder {
    lo: BinaryHeap<i32>,
    hi: BinaryHeap<Reverse<i32>>,
}

impl MedianFinder {
    pub fn new() -> Self {
        MedianFinder {
            lo: BinaryHeap::new(),
            hi: BinaryHeap::new(),
        }
    }
    pub fn add(&mut self, num: i32) {
        self.lo.push(num);
        let top = self.lo.pop().unwrap();
        self.hi.push(Reverse(top));
        if self.hi.len() > self.lo.len() {
            let Reverse(x) = self.hi.pop().unwrap();
            self.lo.push(x);
        }
    }
    pub fn median(&self) -> f64 {
        if self.lo.len() > self.hi.len() {
            *self.lo.peek().unwrap() as f64
        } else {
            let a = *self.lo.peek().unwrap();
            let Reverse(b) = self.hi.peek().unwrap();
            (a + b) as f64 / 2.0
        }
    }
}

impl Default for MedianFinder {
    fn default() -> Self {
        Self::new()
    }
}
