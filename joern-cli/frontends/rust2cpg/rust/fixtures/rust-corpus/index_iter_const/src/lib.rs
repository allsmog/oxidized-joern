use std::fmt;
use std::ops::Index;

pub struct Ring {
    data: Vec<i32>,
}

impl Index<std::ops::Range<usize>> for Ring {
    type Output = [i32];
    fn index(&self, r: std::ops::Range<usize>) -> &[i32] {
        &self.data[r]
    }
}

pub fn evens(n: u32) -> impl Iterator<Item = u32> {
    (0..n).filter(|x| x % 2 == 0)
}

pub fn collect_evens() -> Vec<u32> {
    evens(10).collect()
}

pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }
}

pub const BLACK: Color = Color::rgb(0, 0, 0);

pub struct Row {
    name: String,
    value: i32,
}

impl fmt::Display for Row {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:<10}{:>6}", self.name, self.value)
    }
}

pub struct Repeater {
    item: i32,
    count: usize,
}

impl Iterator for Repeater {
    type Item = i32;
    fn next(&mut self) -> Option<i32> {
        if self.count > 0 {
            self.count -= 1;
            Some(self.item)
        } else {
            None
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count, Some(self.count))
    }
}
