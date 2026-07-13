pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in 0..=a.len() {
        dp[i][0] = i;
    }
    for j in 0..=b.len() {
        dp[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[a.len()][b.len()]
}

pub fn coin_change(coins: &[u32], amount: u32) -> Option<u32> {
    let mut dp = vec![u32::MAX; (amount + 1) as usize];
    dp[0] = 0;
    for a in 1..=amount {
        for &c in coins {
            if c <= a {
                if let Some(v) = dp[(a - c) as usize].checked_add(1) {
                    dp[a as usize] = dp[a as usize].min(v);
                }
            }
        }
    }
    if dp[amount as usize] == u32::MAX {
        None
    } else {
        Some(dp[amount as usize])
    }
}

pub fn knapsack(weights: &[u32], values: &[u32], cap: u32) -> u32 {
    let mut dp = vec![0u32; (cap + 1) as usize];
    for i in 0..weights.len() {
        for w in (weights[i]..=cap).rev() {
            dp[w as usize] = dp[w as usize].max(dp[(w - weights[i]) as usize] + values[i]);
        }
    }
    dp[cap as usize]
}

pub fn lis(nums: &[i32]) -> usize {
    let mut tails: Vec<i32> = Vec::new();
    for &n in nums {
        match tails.binary_search(&n) {
            Ok(_) => {}
            Err(pos) => {
                if pos == tails.len() {
                    tails.push(n);
                } else {
                    tails[pos] = n;
                }
            }
        }
    }
    tails.len()
}
