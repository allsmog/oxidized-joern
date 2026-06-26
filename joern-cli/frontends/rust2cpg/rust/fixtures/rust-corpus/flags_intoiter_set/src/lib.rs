use std::collections::HashSet;

pub struct Flags(u8);

impl Flags {
    pub const READ: u8 = 1 << 0;
    pub const WRITE: u8 = 1 << 1;
    pub const EXEC: u8 = 1 << 2;

    pub fn new(bits: u8) -> Self {
        Flags(bits)
    }
    pub fn can_read(&self) -> bool {
        self.0 & Self::READ != 0
    }
    pub fn can_write(&self) -> bool {
        self.0 & Self::WRITE != 0
    }
}

pub struct Grid {
    cells: Vec<i32>,
}

impl<'a> IntoIterator for &'a Grid {
    type Item = &'a i32;
    type IntoIter = std::slice::Iter<'a, i32>;
    fn into_iter(self) -> Self::IntoIter {
        self.cells.iter()
    }
}

pub fn grid_sum(g: &Grid) -> i32 {
    let mut s = 0;
    for c in g {
        s += c;
    }
    s
}

pub fn set_ops(a: &HashSet<i32>, b: &HashSet<i32>) -> usize {
    let inter: HashSet<_> = a.intersection(b).collect();
    let uni: HashSet<_> = a.union(b).collect();
    let diff: HashSet<_> = a.difference(b).collect();
    inter.len() + uni.len() + diff.len()
}
