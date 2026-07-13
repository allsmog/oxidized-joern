pub fn kruskal(n: usize, mut edges: Vec<(u32, usize, usize)>) -> u32 {
    edges.sort();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    let mut total = 0;
    for (w, a, b) in edges {
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra != rb {
            parent[ra] = rb;
            total += w;
        }
    }
    total
}

pub fn kth_smallest(mut v: Vec<i32>, k: usize) -> Option<i32> {
    if k >= v.len() {
        return None;
    }
    fn select(v: &mut [i32], k: usize) -> i32 {
        let pivot = v[v.len() / 2];
        let less: Vec<i32> = v.iter().filter(|&&x| x < pivot).copied().collect();
        let eq = v.iter().filter(|&&x| x == pivot).count();
        if k < less.len() {
            select(&mut less.clone(), k)
        } else if k < less.len() + eq {
            pivot
        } else {
            let mut greater: Vec<i32> = v.iter().filter(|&&x| x > pivot).copied().collect();
            select(&mut greater, k - less.len() - eq)
        }
    }
    Some(select(&mut v, k))
}

pub struct Ring {
    buf: Vec<i32>,
    head: usize,
    len: usize,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Ring {
            buf: vec![0; cap],
            head: 0,
            len: 0,
        }
    }
    pub fn push(&mut self, x: i32) {
        let cap = self.buf.len();
        let idx = (self.head + self.len) % cap;
        self.buf[idx] = x;
        if self.len < cap {
            self.len += 1;
        } else {
            self.head = (self.head + 1) % cap;
        }
    }
}
