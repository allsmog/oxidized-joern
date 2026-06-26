use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;

pub fn min_heap_sort(values: Vec<i32>) -> Vec<i32> {
    let mut heap = BinaryHeap::new();
    for v in values {
        heap.push(Reverse(v));
    }
    let mut out = Vec::new();
    while let Some(Reverse(v)) = heap.pop() {
        out.push(v);
    }
    out
}

#[derive(PartialEq, Eq)]
pub struct Task {
    priority: u32,
    name: String,
}

impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Task) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Task {
    fn cmp(&self, other: &Task) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.name.cmp(&other.name))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Pixel {
    r: u8,
    g: u8,
    b: u8,
}

pub struct Matrix {
    rows: usize,
    cols: usize,
}

impl fmt::Debug for Matrix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Matrix({}x{})", self.rows, self.cols)
    }
}
