use std::fmt;

#[derive(Debug)]
pub enum MyError {
    NotFound,
    Invalid(String),
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MyError::NotFound => write!(f, "not found"),
            MyError::Invalid(s) => write!(f, "invalid: {s}"),
        }
    }
}

impl std::error::Error for MyError {}

pub struct Bag {
    items: Vec<i32>,
}

impl IntoIterator for Bag {
    type Item = i32;
    type IntoIter = std::vec::IntoIter<i32>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

pub struct Nums(Vec<i32>);

impl FromIterator<i32> for Nums {
    fn from_iter<I: IntoIterator<Item = i32>>(iter: I) -> Self {
        Nums(iter.into_iter().collect())
    }
}

pub fn bag_sum(b: Bag) -> i32 {
    b.into_iter().sum()
}

pub fn collect_nums() -> Nums {
    (0..5).collect()
}

pub fn measure<S: AsRef<str>>(s: S) -> usize {
    s.as_ref().len()
}
