use std::any::Any;
use std::fmt::Debug;

pub struct Wrapper<T> {
    inner: T,
}

impl<T> Wrapper<T>
where
    T: Clone + Debug,
{
    pub fn show(&self) -> String {
        format!("{:?}", self.inner.clone())
    }
}

pub trait Draw {
    fn draw(&self) -> i32;
}

pub fn total(items: &[&dyn Draw]) -> i32 {
    items.iter().map(|d| d.draw()).sum()
}

pub enum Tree<T> {
    Leaf(T),
    Branch(Box<Tree<T>>, Box<Tree<T>>),
}

impl<T: Copy> Tree<T> {
    pub fn leaf_value(&self) -> Option<T> {
        match self {
            Tree::Leaf(v) => Some(*v),
            _ => None,
        }
    }
}

pub struct Pair<T>(pub T, pub T);

impl<T: std::ops::Add<Output = T> + Copy> Pair<T> {
    pub fn sum(&self) -> T {
        self.0 + self.1
    }
}

pub fn as_i32(x: &dyn Any) -> Option<i32> {
    x.downcast_ref::<i32>().copied()
}
