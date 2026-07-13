use std::borrow::Borrow;

pub struct Settings {
    level: u32,
    name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            level: 1,
            name: "default".to_string(),
        }
    }
}

pub fn make_settings() -> Settings {
    Settings::default()
}

pub fn longest<S: Borrow<str>>(items: &[S]) -> usize {
    items.iter().map(|s| s.borrow().len()).max().unwrap_or(0)
}

pub fn make_iter(flag: bool) -> Box<dyn Iterator<Item = i32>> {
    if flag {
        Box::new((0..5).map(|x| x * 2))
    } else {
        Box::new(vec![1, 3, 5].into_iter())
    }
}

pub const fn factorial(n: u64) -> u64 {
    let mut result = 1;
    let mut i = 1;
    while i <= n {
        result *= i;
        i += 1;
    }
    result
}

pub const FACT5: u64 = factorial(5);
