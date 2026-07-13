pub fn lcs(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    dp[a.len()][b.len()]
}

pub fn max_profit(prices: &[i32]) -> i32 {
    let mut min_price = i32::MAX;
    let mut best = 0;
    for &p in prices {
        min_price = min_price.min(p);
        best = best.max(p - min_price);
    }
    best
}

pub fn min_total(triangle: &[Vec<i32>]) -> i32 {
    let mut dp = triangle[triangle.len() - 1].clone();
    for row in (0..triangle.len() - 1).rev() {
        for col in 0..=row {
            dp[col] = triangle[row][col] + dp[col].min(dp[col + 1]);
        }
    }
    dp[0]
}

pub fn count_and_say(n: u32) -> String {
    let mut result = "1".to_string();
    for _ in 1..n {
        let mut next = String::new();
        let chars: Vec<char> = result.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let mut count = 1;
            while i + count < chars.len() && chars[i + count] == chars[i] {
                count += 1;
            }
            next.push_str(&count.to_string());
            next.push(chars[i]);
            i += count;
        }
        result = next;
    }
    result
}

pub fn num_trees(n: usize) -> u64 {
    let mut dp = vec![0u64; n + 1];
    dp[0] = 1;
    for nodes in 1..=n {
        for root in 1..=nodes {
            dp[nodes] += dp[root - 1] * dp[nodes - root];
        }
    }
    dp[n]
}
