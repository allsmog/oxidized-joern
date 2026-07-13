use std::collections::{BTreeMap, BTreeSet};

pub fn swap_reverse(v: &mut [i32]) {
    if v.len() >= 2 {
        let (left, right) = v.split_at_mut(2);
        left.swap(0, 1);
        right.reverse();
    }
}

pub fn rchunk_count(v: &[i32]) -> usize {
    v.rchunks(3).count()
}

pub fn set_relation(a: &BTreeSet<i32>, b: &BTreeSet<i32>) -> (bool, usize) {
    let sub = a.is_subset(b);
    let sym: BTreeSet<_> = a.symmetric_difference(b).collect();
    (sub, sym.len())
}

pub fn repeat_clone<T: Clone>(item: T, n: usize) -> Vec<T> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(item.clone());
    }
    out
}

pub fn three_strings() -> Vec<String> {
    repeat_clone("x".to_string(), 3)
}

pub fn split_map(mut m: BTreeMap<i32, i32>) -> (BTreeMap<i32, i32>, BTreeMap<i32, i32>) {
    let high = m.split_off(&5);
    (m, high)
}
