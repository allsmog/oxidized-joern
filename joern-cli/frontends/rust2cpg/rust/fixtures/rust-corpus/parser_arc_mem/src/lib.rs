use std::collections::VecDeque;
use std::sync::Arc;

pub fn eval(expr: &str) -> i32 {
    let mut acc = 0;
    let mut sign = 1;
    for tok in expr.split_whitespace() {
        match tok {
            "+" => sign = 1,
            "-" => sign = -1,
            n => acc += sign * n.parse::<i32>().unwrap_or(0),
        }
    }
    acc
}

pub trait Handler: Send + Sync {
    fn handle(&self) -> i32;
}

pub struct Echo;

impl Handler for Echo {
    fn handle(&self) -> i32 {
        1
    }
}

pub fn dispatch() -> i32 {
    let h: Arc<dyn Handler + Send + Sync> = Arc::new(Echo);
    h.handle()
}

pub struct Buffer {
    data: Vec<u8>,
}

impl Buffer {
    pub fn flush(&mut self) -> Vec<u8> {
        std::mem::replace(&mut self.data, Vec::new())
    }
}

pub fn deque_ops() -> Option<i32> {
    let mut d: VecDeque<i32> = VecDeque::new();
    d.push_front(1);
    d.push_back(2);
    d.push_front(0);
    let _front = d.front().copied();
    let _back = d.back().copied();
    d.pop_back()
}

pub fn parse_float(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(f64::NAN)
}

pub fn parse_binary(s: &str) -> Result<u64, String> {
    u64::from_str_radix(s, 2).map_err(|e| e.to_string())
}
