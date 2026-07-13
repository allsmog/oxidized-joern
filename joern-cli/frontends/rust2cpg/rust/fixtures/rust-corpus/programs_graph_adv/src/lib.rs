use std::collections::VecDeque;

pub fn bellman_ford(n: usize, edges: &[(usize, usize, i32)], src: usize) -> Option<Vec<i32>> {
    let mut dist = vec![i32::MAX; n];
    dist[src] = 0;
    for _ in 0..n - 1 {
        for &(u, v, w) in edges {
            if dist[u] != i32::MAX && dist[u] + w < dist[v] {
                dist[v] = dist[u] + w;
            }
        }
    }
    for &(u, v, w) in edges {
        if dist[u] != i32::MAX && dist[u] + w < dist[v] {
            return None;
        }
    }
    Some(dist)
}

pub fn floyd_warshall(mut dist: Vec<Vec<i32>>) -> Vec<Vec<i32>> {
    let n = dist.len();
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if dist[i][k] != i32::MAX
                    && dist[k][j] != i32::MAX
                    && dist[i][k] + dist[k][j] < dist[i][j]
                {
                    dist[i][j] = dist[i][k] + dist[k][j];
                }
            }
        }
    }
    dist
}

pub fn is_bipartite(adj: &[Vec<usize>]) -> bool {
    let mut color = vec![-1i32; adj.len()];
    for start in 0..adj.len() {
        if color[start] != -1 {
            continue;
        }
        color[start] = 0;
        let mut q = VecDeque::new();
        q.push_back(start);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if color[v] == -1 {
                    color[v] = 1 - color[u];
                    q.push_back(v);
                } else if color[v] == color[u] {
                    return false;
                }
            }
        }
    }
    true
}

pub struct Fenwick {
    tree: Vec<i64>,
}

impl Fenwick {
    pub fn new(n: usize) -> Self {
        Fenwick {
            tree: vec![0; n + 1],
        }
    }
    pub fn update(&mut self, mut i: usize, delta: i64) {
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += i & i.wrapping_neg();
        }
    }
    pub fn query(&self, mut i: usize) -> i64 {
        let mut sum = 0;
        while i > 0 {
            sum += self.tree[i];
            i -= i & i.wrapping_neg();
        }
        sum
    }
}

pub fn z_function(s: &str) -> Vec<usize> {
    let s: Vec<char> = s.chars().collect();
    let n = s.len();
    let mut z = vec![0; n];
    let (mut l, mut r) = (0, 0);
    for i in 1..n {
        if i < r {
            z[i] = (r - i).min(z[i - l]);
        }
        while i + z[i] < n && s[z[i]] == s[i + z[i]] {
            z[i] += 1;
        }
        if i + z[i] > r {
            l = i;
            r = i + z[i];
        }
    }
    z
}
