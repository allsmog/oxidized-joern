use std::collections::HashMap;

pub fn generate(n: u32) -> Vec<String> {
    let mut out = Vec::new();
    fn bt(open: u32, close: u32, n: u32, cur: &mut String, out: &mut Vec<String>) {
        if cur.len() as u32 == 2 * n {
            out.push(cur.clone());
            return;
        }
        if open < n {
            cur.push('(');
            bt(open + 1, close, n, cur, out);
            cur.pop();
        }
        if close < open {
            cur.push(')');
            bt(open, close + 1, n, cur, out);
            cur.pop();
        }
    }
    bt(0, 0, n, &mut String::new(), &mut out);
    out
}

pub fn combination_sum(candidates: &[i32], target: i32) -> Vec<Vec<i32>> {
    let mut out = Vec::new();
    fn bt(cands: &[i32], start: usize, remain: i32, cur: &mut Vec<i32>, out: &mut Vec<Vec<i32>>) {
        if remain == 0 {
            out.push(cur.clone());
            return;
        }
        for i in start..cands.len() {
            if cands[i] <= remain {
                cur.push(cands[i]);
                bt(cands, i, remain - cands[i], cur, out);
                cur.pop();
            }
        }
    }
    bt(candidates, 0, target, &mut Vec::new(), &mut out);
    out
}

pub fn subsets_with_dup(mut nums: Vec<i32>) -> Vec<Vec<i32>> {
    nums.sort();
    let mut out = Vec::new();
    fn bt(nums: &[i32], start: usize, cur: &mut Vec<i32>, out: &mut Vec<Vec<i32>>) {
        out.push(cur.clone());
        for i in start..nums.len() {
            if i > start && nums[i] == nums[i - 1] {
                continue;
            }
            cur.push(nums[i]);
            bt(nums, i + 1, cur, out);
            cur.pop();
        }
    }
    bt(&nums, 0, &mut Vec::new(), &mut out);
    out
}

pub fn partition(s: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    fn is_palin(c: &[char]) -> bool {
        let (mut i, mut j) = (0, c.len().saturating_sub(1));
        while i < j {
            if c[i] != c[j] {
                return false;
            }
            i += 1;
            j -= 1;
        }
        true
    }
    fn bt(chars: &[char], start: usize, cur: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        if start == chars.len() {
            out.push(cur.clone());
            return;
        }
        for end in start + 1..=chars.len() {
            if is_palin(&chars[start..end]) {
                cur.push(chars[start..end].iter().collect());
                bt(chars, end, cur, out);
                cur.pop();
            }
        }
    }
    bt(&chars, 0, &mut Vec::new(), &mut out);
    out
}

pub fn letter_combinations(digits: &str) -> Vec<String> {
    if digits.is_empty() {
        return Vec::new();
    }
    let map: HashMap<char, &str> = [('2', "abc"), ('3', "def"), ('4', "ghi")]
        .into_iter()
        .collect();
    let chars: Vec<char> = digits.chars().collect();
    let mut out = Vec::new();
    fn bt(
        chars: &[char],
        map: &HashMap<char, &str>,
        idx: usize,
        cur: &mut String,
        out: &mut Vec<String>,
    ) {
        if idx == chars.len() {
            out.push(cur.clone());
            return;
        }
        if let Some(letters) = map.get(&chars[idx]) {
            for c in letters.chars() {
                cur.push(c);
                bt(chars, map, idx + 1, cur, out);
                cur.pop();
            }
        }
    }
    bt(&chars, &map, 0, &mut String::new(), &mut out);
    out
}
