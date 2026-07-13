use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub fn step(grid: &[Vec<bool>]) -> Vec<Vec<bool>> {
    let (rows, cols) = (grid.len(), grid[0].len());
    let mut next = vec![vec![false; cols]; rows];
    for r in 0..rows {
        for c in 0..cols {
            let mut neighbors = 0;
            for dr in -1i32..=1 {
                for dc in -1i32..=1 {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let nr = r as i32 + dr;
                    let nc = c as i32 + dc;
                    if nr >= 0
                        && nr < rows as i32
                        && nc >= 0
                        && nc < cols as i32
                        && grid[nr as usize][nc as usize]
                    {
                        neighbors += 1;
                    }
                }
            }
            next[r][c] = matches!((grid[r][c], neighbors), (true, 2) | (_, 3));
        }
    }
    next
}

pub fn langton(steps: u32, size: usize) -> Vec<Vec<bool>> {
    let mut grid = vec![vec![false; size]; size];
    let (mut r, mut c) = (size / 2, size / 2);
    let (mut dr, mut dc): (i32, i32) = (-1, 0);
    for _ in 0..steps {
        if grid[r][c] {
            let t = dr;
            dr = dc;
            dc = -t;
        } else {
            let t = dr;
            dr = -dc;
            dc = t;
        }
        grid[r][c] = !grid[r][c];
        r = ((r as i32 + dr).rem_euclid(size as i32)) as usize;
        c = ((c as i32 + dc).rem_euclid(size as i32)) as usize;
    }
    grid
}

pub fn shortest(grid: &[Vec<u32>]) -> u32 {
    let (rows, cols) = (grid.len(), grid[0].len());
    let mut dist = vec![vec![u32::MAX; cols]; rows];
    let mut heap = BinaryHeap::new();
    dist[0][0] = grid[0][0];
    heap.push(Reverse((grid[0][0], 0usize, 0usize)));
    while let Some(Reverse((d, r, c))) = heap.pop() {
        if d > dist[r][c] {
            continue;
        }
        let dirs = [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)];
        for (dr, dc) in dirs {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr >= 0 && nr < rows as i32 && nc >= 0 && nc < cols as i32 {
                let (nr, nc) = (nr as usize, nc as usize);
                let nd = d + grid[nr][nc];
                if nd < dist[nr][nc] {
                    dist[nr][nc] = nd;
                    heap.push(Reverse((nd, nr, nc)));
                }
            }
        }
    }
    dist[rows - 1][cols - 1]
}

pub fn transpose<T: Copy>(m: &[Vec<T>]) -> Vec<Vec<T>> {
    if m.is_empty() {
        return Vec::new();
    }
    let (rows, cols) = (m.len(), m[0].len());
    let mut out = Vec::with_capacity(cols);
    for c in 0..cols {
        let mut row = Vec::with_capacity(rows);
        for r in 0..rows {
            row.push(m[r][c]);
        }
        out.push(row);
    }
    out
}

pub fn astar(grid: &[Vec<bool>], start: (usize, usize), goal: (usize, usize)) -> Option<u32> {
    let h = |r: usize, c: usize| {
        ((r as i32 - goal.0 as i32).abs() + (c as i32 - goal.1 as i32).abs()) as u32
    };
    let (rows, cols) = (grid.len(), grid[0].len());
    let mut heap = BinaryHeap::new();
    let mut best = vec![vec![u32::MAX; cols]; rows];
    heap.push(Reverse((h(start.0, start.1), 0u32, start.0, start.1)));
    while let Some(Reverse((_, g, r, c))) = heap.pop() {
        if (r, c) == goal {
            return Some(g);
        }
        if g > best[r][c] {
            continue;
        }
        for (dr, dc) in [(0i32, 1i32), (1, 0), (0, -1), (-1, 0)] {
            let (nr, nc) = (r as i32 + dr, c as i32 + dc);
            if nr >= 0
                && nr < rows as i32
                && nc >= 0
                && nc < cols as i32
                && !grid[nr as usize][nc as usize]
            {
                let (nr, nc) = (nr as usize, nc as usize);
                let ng = g + 1;
                if ng < best[nr][nc] {
                    best[nr][nc] = ng;
                    heap.push(Reverse((ng + h(nr, nc), ng, nr, nc)));
                }
            }
        }
    }
    None
}
