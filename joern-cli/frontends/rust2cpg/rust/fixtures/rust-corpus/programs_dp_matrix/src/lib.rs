use std::collections::HashSet;

pub fn rotate(m: &mut Vec<Vec<i32>>) {
    let n = m.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let tmp = m[i][j];
            m[i][j] = m[j][i];
            m[j][i] = tmp;
        }
    }
    for row in m.iter_mut() {
        row.reverse();
    }
}

pub fn word_break(s: &str, dict: &HashSet<String>) -> bool {
    let n = s.len();
    let mut dp = vec![false; n + 1];
    dp[0] = true;
    for i in 1..=n {
        for j in 0..i {
            if dp[j] && dict.contains(&s[j..i]) {
                dp[i] = true;
                break;
            }
        }
    }
    dp[n]
}

pub fn longest_palindrome(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let (mut start, mut max_len) = (0, 0);
    let expand = |l: i32, r: i32| -> (usize, usize) {
        let (mut l, mut r) = (l, r);
        while l >= 0 && (r as usize) < chars.len() && chars[l as usize] == chars[r as usize] {
            l -= 1;
            r += 1;
        }
        ((l + 1) as usize, (r - l - 1) as usize)
    };
    for i in 0..chars.len() {
        for (s2, len) in [expand(i as i32, i as i32), expand(i as i32, i as i32 + 1)] {
            if len > max_len {
                start = s2;
                max_len = len;
            }
        }
    }
    chars[start..start + max_len].iter().collect()
}

pub fn count_primes(n: usize) -> usize {
    if n < 3 {
        return 0;
    }
    let mut sieve = vec![true; n];
    let mut count = 0;
    for i in 2..n {
        if sieve[i] {
            count += 1;
            let mut j = i * i;
            while j < n {
                sieve[j] = false;
                j += i;
            }
        }
    }
    count
}

pub fn is_happy(mut n: u32) -> bool {
    let mut seen = HashSet::new();
    while n != 1 && seen.insert(n) {
        n = n
            .to_string()
            .chars()
            .map(|c| {
                let d = c.to_digit(10).unwrap();
                d * d
            })
            .sum();
    }
    n == 1
}
