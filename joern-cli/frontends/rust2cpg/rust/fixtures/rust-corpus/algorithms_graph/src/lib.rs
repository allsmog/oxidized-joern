use std::collections::{HashMap, HashSet, VecDeque};

pub fn fib(n: u64, memo: &mut HashMap<u64, u64>) -> u64 {
    if n < 2 {
        return n;
    }
    if let Some(&v) = memo.get(&n) {
        return v;
    }
    let v = fib(n - 1, memo) + fib(n - 2, memo);
    memo.insert(n, v);
    v
}

pub fn bfs(adj: &[Vec<usize>], start: usize) -> Vec<usize> {
    let mut order = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start);
    visited.insert(start);
    while let Some(node) = queue.pop_front() {
        order.push(node);
        for &next in &adj[node] {
            if visited.insert(next) {
                queue.push_back(next);
            }
        }
    }
    order
}

pub fn dfs(adj: &[Vec<usize>], node: usize, visited: &mut Vec<bool>, order: &mut Vec<usize>) {
    visited[node] = true;
    order.push(node);
    for &next in &adj[node] {
        if !visited[next] {
            dfs(adj, next, visited, order);
        }
    }
}

pub fn count_region(grid: &[[bool; 3]; 3], r: usize, c: usize, seen: &mut [[bool; 3]; 3]) -> u32 {
    if r >= 3 || c >= 3 || seen[r][c] || !grid[r][c] {
        return 0;
    }
    seen[r][c] = true;
    1 + count_region(grid, r + 1, c, seen) + count_region(grid, r, c + 1, seen)
}
