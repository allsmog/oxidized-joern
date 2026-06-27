use std::collections::HashSet;

pub fn failure(pat: &[u8]) -> Vec<usize> {
    let mut f = vec![0; pat.len()];
    let mut k = 0;
    for i in 1..pat.len() {
        while k > 0 && pat[k] != pat[i] {
            k = f[k - 1];
        }
        if pat[k] == pat[i] {
            k += 1;
        }
        f[i] = k;
    }
    f
}

pub fn rabin_karp(text: &[u8], pat: &[u8]) -> Option<usize> {
    if pat.len() > text.len() {
        return None;
    }
    let base: u64 = 256;
    let modulus: u64 = 1_000_000_007;
    let mut ph = 0u64;
    let mut th = 0u64;
    let mut pow = 1u64;
    for i in 0..pat.len() {
        ph = (ph * base + pat[i] as u64) % modulus;
        th = (th * base + text[i] as u64) % modulus;
        if i > 0 {
            pow = pow * base % modulus;
        }
    }
    for i in 0..=text.len() - pat.len() {
        if ph == th && &text[i..i + pat.len()] == pat {
            return Some(i);
        }
        if i + pat.len() < text.len() {
            th = ((th + modulus - text[i] as u64 * pow % modulus) * base
                + text[i + pat.len()] as u64)
                % modulus;
        }
    }
    None
}

pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                1 + prev[j].min(cur[j - 1]).min(prev[j - 1])
            };
        }
        prev = cur;
    }
    prev[b.len()]
}

pub fn set_zeroes(m: &mut Vec<Vec<i32>>) {
    let mut rows = HashSet::new();
    let mut cols = HashSet::new();
    for i in 0..m.len() {
        for j in 0..m[0].len() {
            if m[i][j] == 0 {
                rows.insert(i);
                cols.insert(j);
            }
        }
    }
    for i in 0..m.len() {
        for j in 0..m[0].len() {
            if rows.contains(&i) || cols.contains(&j) {
                m[i][j] = 0;
            }
        }
    }
}

pub fn diagonal(m: &[Vec<i32>]) -> Vec<i32> {
    let (rows, cols) = (m.len(), m[0].len());
    let mut out = Vec::new();
    for d in 0..rows + cols - 1 {
        if d % 2 == 0 {
            let mut r = d.min(rows - 1);
            while r as i32 >= 0 && d - r < cols {
                out.push(m[r][d - r]);
                if r == 0 {
                    break;
                }
                r -= 1;
            }
        } else {
            let mut c = d.min(cols - 1);
            while c as i32 >= 0 && d - c < rows {
                out.push(m[d - c][c]);
                if c == 0 {
                    break;
                }
                c -= 1;
            }
        }
    }
    out
}
