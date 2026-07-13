pub fn find_substr(text: &str, pat: &str) -> Option<usize> {
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pat.chars().collect();
    if p.is_empty() {
        return Some(0);
    }
    for i in 0..=t.len().saturating_sub(p.len()) {
        if t[i..i + p.len()] == p[..] {
            return Some(i);
        }
    }
    None
}

pub fn to_base(mut n: u32, base: u32) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let digits = b"0123456789abcdef";
    let mut out = Vec::new();
    while n > 0 {
        out.push(digits[(n % base) as usize]);
        n /= base;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

pub fn pascal(rows: usize) -> Vec<Vec<u64>> {
    let mut tri: Vec<Vec<u64>> = Vec::new();
    for i in 0..rows {
        let mut row = vec![1u64; i + 1];
        for j in 1..i {
            row[j] = tri[i - 1][j - 1] + tri[i - 1][j];
        }
        tri.push(row);
    }
    tri
}

pub fn caesar(s: &str, shift: u8) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_lowercase() {
                (((c as u8 - b'a' + shift) % 26) + b'a') as char
            } else if c.is_ascii_uppercase() {
                (((c as u8 - b'A' + shift) % 26) + b'A') as char
            } else {
                c
            }
        })
        .collect()
}

pub fn longest_common_prefix(strs: &[&str]) -> String {
    if strs.is_empty() {
        return String::new();
    }
    let first = strs[0];
    let mut end = first.len();
    for s in &strs[1..] {
        end = end.min(
            first
                .chars()
                .zip(s.chars())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    first[..end].to_string()
}
