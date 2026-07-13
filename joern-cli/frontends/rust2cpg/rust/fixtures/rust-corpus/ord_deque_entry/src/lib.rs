use std::collections::{HashMap, VecDeque};

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version {
    major: u32,
    minor: u32,
}

pub fn sort_versions(mut vs: Vec<Version>) -> Vec<Version> {
    vs.sort();
    vs
}

pub fn rotate(mut d: VecDeque<i32>) -> Vec<i32> {
    if !d.is_empty() {
        d.rotate_left(1);
    }
    d.make_contiguous().to_vec()
}

pub fn accumulate(items: &[(&str, i32)]) -> HashMap<String, i32> {
    let mut m: HashMap<String, i32> = HashMap::new();
    for (k, v) in items {
        m.entry(k.to_string()).and_modify(|e| *e += v).or_insert(*v);
    }
    for v in m.values_mut() {
        *v *= 2;
    }
    m
}

pub fn group(pairs: &[(&str, i32)]) -> HashMap<String, Vec<i32>> {
    let mut m: HashMap<String, Vec<i32>> = HashMap::new();
    for (k, v) in pairs {
        m.entry(k.to_string()).or_default().push(*v);
    }
    m
}
