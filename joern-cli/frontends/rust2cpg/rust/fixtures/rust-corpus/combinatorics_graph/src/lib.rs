use std::collections::VecDeque;

pub fn subsets(nums: &[i32]) -> Vec<Vec<i32>> {
    let mut result = Vec::new();
    for mask in 0..(1u32 << nums.len()) {
        let subset: Vec<i32> = nums
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, &x)| x)
            .collect();
        result.push(subset);
    }
    result
}

pub fn permute(nums: &mut Vec<i32>, start: usize, out: &mut Vec<Vec<i32>>) {
    if start == nums.len() {
        out.push(nums.clone());
        return;
    }
    for i in start..nums.len() {
        nums.swap(start, i);
        permute(nums, start + 1, out);
        nums.swap(start, i);
    }
}

pub fn count_queens(n: usize) -> usize {
    fn solve(row: usize, n: usize, cols: u32, diag1: u32, diag2: u32) -> usize {
        if row == n {
            return 1;
        }
        let mut count = 0;
        let available = !(cols | diag1 | diag2) & ((1 << n) - 1);
        let mut bits = available;
        while bits != 0 {
            let bit = bits & bits.wrapping_neg();
            bits ^= bit;
            count += solve(row + 1, n, cols | bit, (diag1 | bit) << 1, (diag2 | bit) >> 1);
        }
        count
    }
    solve(0, n, 0, 0, 0)
}

pub fn step(grid: &[[bool; 3]; 3]) -> [[bool; 3]; 3] {
    let mut next = [[false; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            let mut n = 0;
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let (nr, nc) = (r as i32 + dr, c as i32 + dc);
                    if nr >= 0 && nr < 3 && nc >= 0 && nc < 3 && grid[nr as usize][nc as usize] {
                        n += 1;
                    }
                }
            }
            next[r][c] = matches!((grid[r][c], n), (true, 2) | (true, 3) | (false, 3));
        }
    }
    next
}

pub fn topo_sort(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut indeg = vec![0usize; n];
    let mut adj = vec![Vec::new(); n];
    for &(u, v) in edges {
        adj[u].push(v);
        indeg[v] += 1;
    }
    let mut q: VecDeque<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut order = Vec::new();
    while let Some(u) = q.pop_front() {
        order.push(u);
        for &v in &adj[u] {
            indeg[v] -= 1;
            if indeg[v] == 0 {
                q.push_back(v);
            }
        }
    }
    order
}
