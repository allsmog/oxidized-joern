pub trait Base {
    fn base(&self) -> i32;
}

pub trait Drawable: Base {
    fn draw(&self) -> String;
    fn outline(&self) -> i32 {
        self.base() * 2
    }
}

pub struct Square {
    pub side: i32,
}

impl Base for Square {
    fn base(&self) -> i32 {
        self.side
    }
}

impl Drawable for Square {
    fn draw(&self) -> String {
        format!("square {}", self.side)
    }
}

pub fn render(items: Vec<Box<dyn Drawable>>) -> String {
    let mut out = String::new();
    for it in &items {
        out.push_str(&it.draw());
    }
    out
}

pub fn via_ufcs(s: &Square) -> i32 {
    Base::base(s) + <Square as Drawable>::outline(s)
}

pub trait Printable {
    fn print(&self) -> String;
}

impl<T: std::fmt::Display> Printable for T {
    fn print(&self) -> String {
        format!("{}", self)
    }
}

pub struct Range {
    cur: u32,
    end: u32,
}

impl Iterator for Range {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.cur < self.end {
            self.cur += 1;
            Some(self.cur)
        } else {
            None
        }
    }
}

pub fn sum_range(end: u32) -> u32 {
    let r = Range { cur: 0, end };
    r.sum()
}
