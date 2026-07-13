use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub fn quicksort<T: Ord + Clone>(arr: &[T]) -> Vec<T> {
    if arr.len() <= 1 {
        return arr.to_vec();
    }
    let pivot = arr[arr.len() / 2].clone();
    let less: Vec<T> = arr.iter().filter(|x| **x < pivot).cloned().collect();
    let equal: Vec<T> = arr.iter().filter(|x| **x == pivot).cloned().collect();
    let greater: Vec<T> = arr.iter().filter(|x| **x > pivot).cloned().collect();
    let mut result = quicksort(&less);
    result.extend(equal);
    result.extend(quicksort(&greater));
    result
}

pub enum Tree {
    Leaf,
    Node(Box<Tree>, i32, Box<Tree>),
}

impl Tree {
    pub fn insert(self, v: i32) -> Tree {
        match self {
            Tree::Leaf => Tree::Node(Box::new(Tree::Leaf), v, Box::new(Tree::Leaf)),
            Tree::Node(l, x, r) => {
                if v < x {
                    Tree::Node(Box::new(l.insert(v)), x, r)
                } else {
                    Tree::Node(l, x, Box::new(r.insert(v)))
                }
            }
        }
    }
    pub fn inorder(&self, out: &mut Vec<i32>) {
        if let Tree::Node(l, x, r) = self {
            l.inorder(out);
            out.push(*x);
            r.inorder(out);
        }
    }
}

pub fn dijkstra(adj: &[Vec<(usize, u32)>], start: usize) -> Vec<u32> {
    let mut dist = vec![u32::MAX; adj.len()];
    dist[start] = 0;
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0u32, start)));
    while let Some(Reverse((d, u))) = heap.pop() {
        if d > dist[u] {
            continue;
        }
        for &(v, w) in &adj[u] {
            let nd = d + w;
            if nd < dist[v] {
                dist[v] = nd;
                heap.push(Reverse((nd, v)));
            }
        }
    }
    dist
}

pub struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    pub fn new(n: usize) -> Self {
        DisjointSet {
            parent: (0..n).collect(),
        }
    }
    pub fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            let root = self.find(self.parent[x]);
            self.parent[x] = root;
        }
        self.parent[x]
    }
    pub fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        self.parent[ra] = rb;
    }
}
