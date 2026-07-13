pub struct Countdown {
    n: u32,
}

impl Iterator for Countdown {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.n > 0 {
            self.n -= 1;
            Some(self.n)
        } else {
            None
        }
    }
}

impl ExactSizeIterator for Countdown {
    fn len(&self) -> usize {
        self.n as usize
    }
}

pub fn count_down(from: u32) -> Vec<u32> {
    Countdown { n: from }.collect()
}

pub fn string_stats(s: &str) -> usize {
    let count = s.matches("ab").count();
    let indices: Vec<(usize, &str)> = s.match_indices('/').collect();
    let split = s.split_once('=');
    count + indices.len() + split.map(|(a, _)| a.len()).unwrap_or(0)
}

pub fn float_ops(x: f64, y: f64) -> f64 {
    x.round() + x.ceil() + x.trunc() + x.fract() + x.hypot(y) + y.atan2(x)
}

pub struct Item {
    x: i32,
}

pub fn array_sum() -> i32 {
    let items: [Item; 3] = [Item { x: 1 }, Item { x: 2 }, Item { x: 3 }];
    items.iter().map(|i| i.x).sum()
}

pub trait Greeter {
    fn greet(&self) -> String;
}

pub struct English;

impl Greeter for English {
    fn greet(&self) -> String {
        "hi".into()
    }
}

pub fn make_greeter() -> impl Greeter {
    English
}
