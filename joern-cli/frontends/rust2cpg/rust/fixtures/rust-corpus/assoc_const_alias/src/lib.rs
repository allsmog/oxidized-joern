use std::collections::HashMap;

pub type Registry<V> = HashMap<String, V>;

pub struct Circle {
    pub radius: f64,
}

impl Circle {
    const PI: f64 = 3.14159;
    pub fn area(&self) -> f64 {
        Self::PI * self.radius * self.radius
    }
    pub fn circumference(&self) -> f64 {
        2.0 * Self::PI * self.radius
    }
}

pub fn build_registry() -> Registry<i32> {
    let mut r: Registry<i32> = Registry::new();
    r.insert("one".into(), 1);
    r.insert("two".into(), 2);
    r
}
