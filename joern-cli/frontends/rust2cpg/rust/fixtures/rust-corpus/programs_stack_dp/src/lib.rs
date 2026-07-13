use std::collections::HashMap;

pub fn daily_temperatures(temps: &[i32]) -> Vec<i32> {
    let mut out = vec![0; temps.len()];
    let mut stack: Vec<usize> = Vec::new();
    for i in 0..temps.len() {
        while let Some(&top) = stack.last() {
            if temps[i] > temps[top] {
                out[top] = (i - top) as i32;
                stack.pop();
            } else {
                break;
            }
        }
        stack.push(i);
    }
    out
}

pub fn next_greater(nums: &[i32]) -> HashMap<i32, i32> {
    let mut map = HashMap::new();
    let mut stack: Vec<i32> = Vec::new();
    for &n in nums {
        while let Some(&top) = stack.last() {
            if n > top {
                map.insert(top, n);
                stack.pop();
            } else {
                break;
            }
        }
        stack.push(n);
    }
    for n in stack {
        map.insert(n, -1);
    }
    map
}

pub fn knapsack(w: &[u32], v: &[u32], cap: usize) -> u32 {
    let n = w.len();
    let mut dp = vec![vec![0u32; cap + 1]; n + 1];
    for i in 1..=n {
        for c in 0..=cap {
            dp[i][c] = dp[i - 1][c];
            if w[i - 1] as usize <= c {
                dp[i][c] = dp[i][c].max(dp[i - 1][c - w[i - 1] as usize] + v[i - 1]);
            }
        }
    }
    dp[n][cap]
}

pub fn min_path(grid: &[Vec<i32>]) -> i32 {
    let (rows, cols) = (grid.len(), grid[0].len());
    let mut dp = vec![vec![0; cols]; rows];
    dp[0][0] = grid[0][0];
    for j in 1..cols {
        dp[0][j] = dp[0][j - 1] + grid[0][j];
    }
    for i in 1..rows {
        dp[i][0] = dp[i - 1][0] + grid[i][0];
    }
    for i in 1..rows {
        for j in 1..cols {
            dp[i][j] = grid[i][j] + dp[i - 1][j].min(dp[i][j - 1]);
        }
    }
    dp[rows - 1][cols - 1]
}

pub fn num_islands(grid: &mut Vec<Vec<char>>) -> u32 {
    fn sink(grid: &mut Vec<Vec<char>>, r: usize, c: usize) {
        if r >= grid.len() || c >= grid[0].len() || grid[r][c] != '1' {
            return;
        }
        grid[r][c] = '0';
        sink(grid, r + 1, c);
        sink(grid, r, c + 1);
        if r > 0 {
            sink(grid, r - 1, c);
        }
        if c > 0 {
            sink(grid, r, c - 1);
        }
    }
    let mut count = 0;
    for r in 0..grid.len() {
        for c in 0..grid[0].len() {
            if grid[r][c] == '1' {
                count += 1;
                sink(grid, r, c);
            }
        }
    }
    count
}
