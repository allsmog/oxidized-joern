use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;
use std::rc::Rc;

pub struct Cache<K, V> {
    store: HashMap<K, V>,
}

impl<K: Hash + Eq + Clone, V: Clone> Cache<K, V> {
    pub fn new() -> Self {
        Cache {
            store: HashMap::new(),
        }
    }
    pub fn put(&mut self, k: K, v: V) {
        self.store.insert(k, v);
    }
    pub fn get(&self, k: &K) -> Option<V> {
        self.store.get(k).cloned()
    }
}

pub fn longest<'a>(words: &'a [&str]) -> Option<&'a &'a str> {
    words.iter().max_by_key(|s| s.len())
}

pub fn split_sign(v: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
    v.into_iter().partition(|&x| x >= 0)
}

pub fn heap_top(v: Vec<i32>) -> (Option<i32>, Option<i32>) {
    let mut heap: BinaryHeap<i32> = v.into_iter().collect();
    let top = heap.peek().copied();
    let popped = heap.pop();
    (top, popped)
}

pub fn ref_count() -> usize {
    let a = Rc::new(5);
    let _b = Rc::clone(&a);
    let _c = Rc::clone(&a);
    Rc::strong_count(&a)
}
