use std::collections::HashMap;

pub fn move_to_front(data: &[u8]) -> Vec<u8> {
    let mut table: Vec<u8> = (0..=255).collect();
    let mut out = Vec::with_capacity(data.len());
    for &b in data {
        let idx = table.iter().position(|&x| x == b).unwrap();
        out.push(idx as u8);
        let val = table.remove(idx);
        table.insert(0, val);
    }
    out
}

pub fn delta_encode(data: &[i32]) -> Vec<i32> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = 0;
    for &x in data {
        out.push(x - prev);
        prev = x;
    }
    out
}

pub fn delta_decode(deltas: &[i32]) -> Vec<i32> {
    let mut out = Vec::with_capacity(deltas.len());
    let mut acc = 0;
    for &d in deltas {
        acc += d;
        out.push(acc);
    }
    out
}

pub fn rle_decode(encoded: &[(u32, char)]) -> String {
    let mut out = String::new();
    for &(count, c) in encoded {
        for _ in 0..count {
            out.push(c);
        }
    }
    out
}

pub fn bwt(s: &str) -> String {
    let s = format!("{}\u{0}", s);
    let n = s.len();
    let bytes = s.as_bytes();
    let mut rotations: Vec<usize> = (0..n).collect();
    rotations.sort_by(|&a, &b| {
        for i in 0..n {
            let ca = bytes[(a + i) % n];
            let cb = bytes[(b + i) % n];
            if ca != cb {
                return ca.cmp(&cb);
            }
        }
        std::cmp::Ordering::Equal
    });
    rotations
        .iter()
        .map(|&r| bytes[(r + n - 1) % n] as char)
        .collect()
}

pub fn lzw_encode(input: &str) -> Vec<u32> {
    let mut dict: HashMap<String, u32> = (0u32..256)
        .map(|i| ((i as u8 as char).to_string(), i))
        .collect();
    let mut next_code = 256;
    let mut current = String::new();
    let mut out = Vec::new();
    for c in input.chars() {
        let combined = format!("{}{}", current, c);
        if dict.contains_key(&combined) {
            current = combined;
        } else {
            out.push(dict[&current]);
            dict.insert(combined, next_code);
            next_code += 1;
            current = c.to_string();
        }
    }
    if !current.is_empty() {
        out.push(dict[&current]);
    }
    out
}
