use std::collections::{BTreeMap, HashMap, HashSet};

pub fn word_index(words: &[&str]) -> HashMap<String, usize> {
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, &w) in words.iter().enumerate() {
        index.entry(w.to_string()).or_insert(i);
    }
    index
}

pub fn sorted_counts(values: &[i32]) -> BTreeMap<i32, u32> {
    let mut counts: BTreeMap<i32, u32> = BTreeMap::new();
    for &v in values {
        *counts.entry(v).or_insert(0) += 1;
    }
    counts
}

pub fn unique(values: &[i32]) -> usize {
    let set: HashSet<i32> = values.iter().copied().collect();
    set.len()
}

pub fn vec_ops(mut values: Vec<i32>) -> Vec<i32> {
    values.retain(|&x| x > 0);
    values.dedup();
    values.sort();
    values
}
