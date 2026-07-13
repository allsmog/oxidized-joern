use std::collections::HashMap;

pub fn map_stats(m: &HashMap<String, i32>) -> i32 {
    let sum: i32 = m.values().sum();
    let keys: Vec<&String> = m.keys().collect();
    sum + keys.len() as i32
}

pub fn bump(m: &mut HashMap<String, i32>, key: &str) {
    if let Some(v) = m.get_mut(key) {
        *v += 1;
    }
}

pub fn merge_split(mut a: Vec<i32>, b: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
    a.extend(b.iter());
    let tail = a.split_off(2);
    a.retain(|&x| x > 0);
    (a, tail)
}

pub fn split_parity(values: &[i32]) -> (Vec<i32>, Vec<i32>) {
    values.iter().partition(|&&x| x % 2 == 0)
}

pub fn unzip_pairs(pairs: Vec<(i32, char)>) -> (Vec<i32>, Vec<char>) {
    pairs.into_iter().unzip()
}

pub fn first_number(words: &[&str]) -> Option<i32> {
    words.iter().find_map(|s| s.parse::<i32>().ok())
}

pub fn index_of_zero(values: &[i32]) -> Option<usize> {
    values.iter().position(|&x| x == 0)
}

pub fn sum_to(n: u64, acc: u64) -> u64 {
    if n == 0 {
        acc
    } else {
        sum_to(n - 1, acc + n)
    }
}
