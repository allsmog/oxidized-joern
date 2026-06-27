use std::collections::HashMap;

pub fn caesar(text: &str, shift: u8) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                (((c as u8 - b'A' + shift) % 26) + b'A') as char
            } else if c.is_ascii_lowercase() {
                (((c as u8 - b'a' + shift) % 26) + b'a') as char
            } else {
                c
            }
        })
        .collect()
}

pub fn vigenere(text: &str, key: &str) -> String {
    let key: Vec<u8> = key
        .bytes()
        .filter(|b| b.is_ascii_alphabetic())
        .map(|b| b.to_ascii_lowercase() - b'a')
        .collect();
    if key.is_empty() {
        return text.to_string();
    }
    let mut ki = 0;
    text.chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                let base = if c.is_ascii_uppercase() { b'A' } else { b'a' };
                let shifted = (((c as u8 - base) + key[ki % key.len()]) % 26) + base;
                ki += 1;
                shifted as char
            } else {
                c
            }
        })
        .collect()
}

pub fn rle_encode(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut count = 1;
        while i + count < chars.len() && chars[i + count] == chars[i] {
            count += 1;
        }
        out.push_str(&format!("{}{}", count, chars[i]));
        i += count;
    }
    out
}

pub fn is_anagram(a: &str, b: &str) -> bool {
    let mut counts: HashMap<char, i32> = HashMap::new();
    for c in a.chars() {
        *counts.entry(c).or_insert(0) += 1;
    }
    for c in b.chars() {
        *counts.entry(c).or_insert(0) -= 1;
    }
    counts.values().all(|&v| v == 0)
}

pub fn is_isomorphic(s: &str, t: &str) -> bool {
    if s.len() != t.len() {
        return false;
    }
    let mut map_st: HashMap<char, char> = HashMap::new();
    let mut map_ts: HashMap<char, char> = HashMap::new();
    for (a, b) in s.chars().zip(t.chars()) {
        if *map_st.entry(a).or_insert(b) != b {
            return false;
        }
        if *map_ts.entry(b).or_insert(a) != a {
            return false;
        }
    }
    true
}
