pub fn rod_cutting(prices: &[i32], n: usize) -> i32 {
    let mut dp = vec![0; n + 1];
    for len in 1..=n {
        let mut best = i32::MIN;
        for cut in 1..=len {
            best = best.max(prices[cut - 1] + dp[len - cut]);
        }
        dp[len] = best;
    }
    dp[n]
}

pub fn matrix_chain(dims: &[usize]) -> usize {
    let n = dims.len() - 1;
    let mut dp = vec![vec![0usize; n]; n];
    for chain in 2..=n {
        for i in 0..=n - chain {
            let j = i + chain - 1;
            dp[i][j] = usize::MAX;
            for k in i..j {
                let cost = dp[i][k] + dp[k + 1][j] + dims[i] * dims[k + 1] * dims[j + 1];
                if cost < dp[i][j] {
                    dp[i][j] = cost;
                }
            }
        }
    }
    dp[0][n - 1]
}

pub fn coin_combinations(coins: &[u32], amount: usize) -> u64 {
    let mut dp = vec![0u64; amount + 1];
    dp[0] = 1;
    for &c in coins {
        for a in c as usize..=amount {
            dp[a] += dp[a - c as usize];
        }
    }
    dp[amount]
}

pub fn subset_sum(nums: &[u32], target: usize) -> bool {
    let mut dp = vec![false; target + 1];
    dp[0] = true;
    for &n in nums {
        for t in (n as usize..=target).rev() {
            if dp[t - n as usize] {
                dp[t] = true;
            }
        }
    }
    dp[target]
}

pub fn num_decodings(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let n = bytes.len();
    if n == 0 || bytes[0] == b'0' {
        return 0;
    }
    let (mut prev, mut cur) = (1u32, 1u32);
    for i in 1..n {
        let mut next = 0;
        if bytes[i] != b'0' {
            next += cur;
        }
        let two = (bytes[i - 1] - b'0') * 10 + (bytes[i] - b'0');
        if (10..=26).contains(&two) {
            next += prev;
        }
        prev = cur;
        cur = next;
    }
    cur
}
