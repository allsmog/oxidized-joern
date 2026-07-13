use std::collections::HashMap;

pub fn dedup_runs(v: &[i32]) -> Vec<i32> {
    let mut it = v.iter().peekable();
    let mut out = Vec::new();
    while let Some(&x) = it.next() {
        while it.next_if(|&&y| y == x).is_some() {}
        out.push(x);
    }
    out
}

pub fn maximum(v: &[i32]) -> Option<i32> {
    v.iter().copied().reduce(|a, b| a.max(b))
}

pub fn char_freq(words: &[&str]) -> HashMap<char, u32> {
    words
        .iter()
        .flat_map(|s| s.chars())
        .fold(HashMap::new(), |mut m, c| {
            *m.entry(c).or_insert(0) += 1;
            m
        })
}

pub fn sum_values(m: HashMap<String, i32>) -> i32 {
    m.into_values().sum()
}

pub fn lookup<'a>(m: &'a HashMap<String, i32>, key: &str) -> Option<(&'a String, &'a i32)> {
    m.get_key_value(key)
}

pub fn alnum_only(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}
