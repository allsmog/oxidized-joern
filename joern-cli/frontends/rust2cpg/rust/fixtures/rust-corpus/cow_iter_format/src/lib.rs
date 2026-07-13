use std::borrow::Cow;

pub fn measure(c: &Cow<str>) -> usize {
    match c {
        Cow::Borrowed(s) => s.len(),
        Cow::Owned(s) => s.len() + 1,
    }
}

pub struct Bag {
    items: Vec<i32>,
}

impl Bag {
    pub fn positives(&self) -> impl Iterator<Item = &i32> {
        self.items.iter().filter(|&&x| x > 0)
    }
}

pub fn count_positive(b: &Bag) -> usize {
    b.positives().count()
}

pub fn csv(n: u32) -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < n {
        s.push_str(&i.to_string());
        s.push(',');
        i += 1;
    }
    s
}

pub fn rounded(x: f64, prec: usize) -> String {
    format!("{:.prec$}", x, prec = prec)
}

pub fn rounded_positional(x: f64, p: usize) -> String {
    format!("{:.1$}", x, p)
}

pub fn even_after_inc(mut v: Vec<i32>) -> Vec<i32> {
    v.retain_mut(|x| {
        *x += 1;
        *x % 2 == 0
    });
    v.dedup_by(|a, b| a == b);
    v
}
