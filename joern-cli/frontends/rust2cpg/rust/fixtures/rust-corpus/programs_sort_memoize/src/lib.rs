use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub fn bubble_sort(v: &mut Vec<i32>) {
    let n = v.len();
    for i in 0..n {
        let mut swapped = false;
        for j in 0..n - 1 - i {
            if v[j] > v[j + 1] {
                v.swap(j, j + 1);
                swapped = true;
            }
        }
        if !swapped {
            break;
        }
    }
}

pub struct Memoize<F: Fn(u64) -> u64> {
    cache: HashMap<u64, u64>,
    func: F,
}

impl<F: Fn(u64) -> u64> Memoize<F> {
    pub fn new(func: F) -> Self {
        Memoize {
            cache: HashMap::new(),
            func,
        }
    }
    pub fn call(&mut self, x: u64) -> u64 {
        if let Some(&v) = self.cache.get(&x) {
            return v;
        }
        let v = (self.func)(x);
        self.cache.insert(x, v);
        v
    }
}

pub fn count_inversions(v: &[i32]) -> u64 {
    let mut count = 0;
    for i in 0..v.len() {
        for j in (i + 1)..v.len() {
            if v[i] > v[j] {
                count += 1;
            }
        }
    }
    count
}

pub fn kth_largest(nums: &[i32], k: usize) -> Option<i32> {
    let mut heap = BinaryHeap::new();
    for &n in nums {
        heap.push(Reverse(n));
        if heap.len() > k {
            heap.pop();
        }
    }
    heap.pop().map(|Reverse(x)| x)
}

pub fn ackermann(m: u64, n: u64) -> u64 {
    if m == 0 {
        n + 1
    } else if n == 0 {
        ackermann(m - 1, 1)
    } else {
        ackermann(m - 1, ackermann(m, n - 1))
    }
}
