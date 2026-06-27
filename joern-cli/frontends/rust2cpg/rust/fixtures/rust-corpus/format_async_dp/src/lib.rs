pub fn table(items: &[(&str, f64)]) -> String {
    items
        .iter()
        .map(|(name, val)| format!("{:>10}: {:8.3}", name, val))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn deferred(x: i32) -> impl std::future::Future<Output = i32> {
    async move {
        let y = x + 1;
        y * 2
    }
}

pub fn rob(nums: &[i32]) -> i32 {
    let (mut prev, mut cur) = (0, 0);
    for &n in nums {
        let next = cur.max(prev + n);
        prev = cur;
        cur = next;
    }
    cur
}

pub fn climb_stairs(n: u32) -> u64 {
    if n <= 2 {
        return n as u64;
    }
    let (mut a, mut b) = (1u64, 2u64);
    for _ in 3..=n {
        let c = a + b;
        a = b;
        b = c;
    }
    b
}

pub fn unique_paths(rows: usize, cols: usize) -> u64 {
    let mut dp = vec![vec![1u64; cols]; rows];
    for i in 1..rows {
        for j in 1..cols {
            dp[i][j] = dp[i - 1][j] + dp[i][j - 1];
        }
    }
    dp[rows - 1][cols - 1]
}
