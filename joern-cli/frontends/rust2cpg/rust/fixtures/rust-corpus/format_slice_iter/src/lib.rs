pub fn debug_forms(v: &[i32]) -> (String, String) {
    let compact = format!("{:?}", v);
    let pretty = format!("{:#?}", v);
    (compact, pretty)
}

pub fn rearrange(v: &mut [i32]) {
    v.fill(7);
    if v.len() >= 2 {
        v.rotate_right(2);
        let last = v.len() - 1;
        v.swap(0, last);
    }
}

pub fn zeros(n: usize) -> Vec<i32> {
    vec![0; n]
}

pub fn grid() -> Vec<Vec<i32>> {
    vec![vec![1, 2]; 3]
}

pub struct Wrapper(Vec<i32>);

impl Wrapper {
    pub fn total(&self) -> i32 {
        self.0.iter().sum()
    }
    pub fn count(&self) -> usize {
        self.0.len()
    }
}

pub struct Counter {
    lo: u32,
    hi: u32,
}

impl Iterator for Counter {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.lo < self.hi {
            self.lo += 1;
            Some(self.lo - 1)
        } else {
            None
        }
    }
}

impl DoubleEndedIterator for Counter {
    fn next_back(&mut self) -> Option<u32> {
        if self.lo < self.hi {
            self.hi -= 1;
            Some(self.hi)
        } else {
            None
        }
    }
}

pub fn reversed_range(lo: u32, hi: u32) -> Vec<u32> {
    Counter { lo, hi }.rev().collect()
}
