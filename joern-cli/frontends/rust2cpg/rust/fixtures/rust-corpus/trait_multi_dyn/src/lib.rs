pub trait Speak {
    fn say(&self) -> String;
}

impl Speak for i32 {
    fn say(&self) -> String {
        format!("int {}", self)
    }
}

impl Speak for String {
    fn say(&self) -> String {
        format!("str {}", self)
    }
}

impl Speak for bool {
    fn say(&self) -> String {
        self.to_string()
    }
}

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Circle(pub f64);
pub struct Rectangle(pub f64, pub f64);

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.0 * self.0
    }
}

impl Shape for Rectangle {
    fn area(&self) -> f64 {
        self.0 * self.1
    }
}

pub fn total_area(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

pub fn describe_all(items: &[Box<dyn Speak>]) -> Vec<String> {
    items.iter().map(|i| i.say()).collect()
}
