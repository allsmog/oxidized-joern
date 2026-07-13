use std::collections::HashMap;

pub fn to_roman(mut n: u32) -> String {
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for &(val, sym) in table.iter() {
        while n >= val {
            out.push_str(sym);
            n -= val;
        }
    }
    out
}

pub fn collatz(mut n: u64) -> Vec<u64> {
    let mut seq = vec![n];
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        seq.push(n);
    }
    seq
}

pub fn valid_parens(s: &str) -> bool {
    let mut stack = Vec::new();
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => stack.push(c),
            ')' => {
                if stack.pop() != Some('(') {
                    return false;
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return false;
                }
            }
            '}' => {
                if stack.pop() != Some('{') {
                    return false;
                }
            }
            _ => {}
        }
    }
    stack.is_empty()
}

pub fn max_subarray(nums: &[i32]) -> i32 {
    let mut best = nums[0];
    let mut cur = nums[0];
    for &n in &nums[1..] {
        cur = n.max(cur + n);
        best = best.max(cur);
    }
    best
}

pub fn group_anagrams(words: &[&str]) -> Vec<Vec<String>> {
    let mut groups: HashMap<Vec<char>, Vec<String>> = HashMap::new();
    for &w in words {
        let mut key: Vec<char> = w.chars().collect();
        key.sort();
        groups.entry(key).or_default().push(w.to_string());
    }
    groups.into_values().collect()
}
