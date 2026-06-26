pub struct Buffer {
    data: Vec<u8>,
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

pub fn compute() -> i32 {
    let op: fn(i32, i32) -> i32 = |a, b| a + b;
    let g: Box<dyn Fn(i32) -> i32> = Box::new(|x| x * 2);
    op(2, 3) + g(5)
}

pub fn sorted(mut v: Vec<(i32, String)>) -> Vec<(i32, String)> {
    v.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    v.sort_by_cached_key(|x| x.1.len());
    v
}

pub fn split_drain(mut v: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
    let drained: Vec<i32> = if v.len() >= 3 {
        v.drain(1..3).collect()
    } else {
        Vec::new()
    };
    (v, drained)
}
