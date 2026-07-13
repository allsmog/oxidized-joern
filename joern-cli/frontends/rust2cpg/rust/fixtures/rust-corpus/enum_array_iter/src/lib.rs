pub enum Shape {
    Point,
    Circle(f64),
    Rect { w: f64, h: f64 },
}

pub fn area(s: &Shape) -> f64 {
    match s {
        Shape::Point => 0.0,
        Shape::Circle(r) => std::f64::consts::PI * r * r,
        Shape::Rect { w, h } => w * h,
    }
}

pub fn squared(arr: [i32; 4]) -> [i32; 4] {
    arr.map(|x| x * x)
}

pub fn shortest<'a>(words: &'a [&str]) -> Option<&'a &'a str> {
    words.iter().min_by_key(|s| s.len())
}

pub fn flatten(nested: Vec<Vec<i32>>) -> Vec<i32> {
    nested.into_iter().flatten().collect()
}

pub fn maybe_len(opt: Option<String>) -> Option<usize> {
    opt.as_deref().map(|s| s.len())
}

pub fn positive_or_none(x: i32) -> Option<i32> {
    (x > 0).then_some(x)
}

pub fn extremes() -> i64 {
    let big = i32::MAX as i64;
    let small = i32::MIN as i64;
    big + small
}
