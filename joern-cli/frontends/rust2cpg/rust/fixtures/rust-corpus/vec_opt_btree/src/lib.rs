use std::collections::{BTreeMap, HashMap};

pub fn replace_middle(mut v: Vec<i32>) -> Vec<i32> {
    if v.len() >= 3 {
        let removed: Vec<i32> = v.splice(1..3, vec![10, 20, 30]).collect();
        v.extend(removed);
    }
    v.dedup();
    v
}

pub fn unzip_pairs(pairs: Vec<Option<(i32, char)>>) -> (Vec<i32>, Vec<char>) {
    pairs.into_iter().flatten().unzip()
}

pub fn split_pair(o: Option<(i32, i32)>) -> (Option<i32>, Option<i32>) {
    o.unzip()
}

pub fn accumulate(items: &[(i32, i32)]) -> BTreeMap<i32, i32> {
    let mut m = BTreeMap::new();
    for (k, v) in items {
        *m.entry(*k).or_insert(0) += v;
    }
    m
}

pub fn nested() -> Vec<HashMap<String, Vec<i32>>> {
    let mut m: HashMap<String, Vec<i32>> = HashMap::new();
    m.entry("a".to_string()).or_default().push(1);
    m.entry("a".to_string()).or_default().push(2);
    vec![m]
}
