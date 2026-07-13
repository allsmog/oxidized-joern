use std::collections::HashMap;

pub fn insertion_sort(v: &mut Vec<i32>) {
    for i in 1..v.len() {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
    }
}

pub fn fizzbuzz(n: u32) -> Vec<String> {
    (1..=n)
        .map(|i| match (i % 3, i % 5) {
            (0, 0) => "FizzBuzz".to_string(),
            (0, _) => "Fizz".to_string(),
            (_, 0) => "Buzz".to_string(),
            _ => i.to_string(),
        })
        .collect()
}

pub fn pow_mod(mut base: u64, mut exp: u64, modulus: u64) -> u64 {
    let mut result = 1;
    base %= modulus;
    while exp > 0 {
        if exp & 1 == 1 {
            result = result * base % modulus;
        }
        exp >>= 1;
        base = base * base % modulus;
    }
    result
}

pub fn histogram(text: &str) -> HashMap<char, u32> {
    let mut h = HashMap::new();
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        *h.entry(c.to_ascii_lowercase()).or_insert(0) += 1;
    }
    h
}

pub fn hanoi(n: u32, from: char, to: char, via: char, moves: &mut Vec<(char, char)>) {
    if n == 0 {
        return;
    }
    hanoi(n - 1, from, via, to, moves);
    moves.push((from, to));
    hanoi(n - 1, via, to, from, moves);
}
