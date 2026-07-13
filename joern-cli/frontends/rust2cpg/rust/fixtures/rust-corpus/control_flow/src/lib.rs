pub enum Shape {
    Circle(f64),
    Rect { w: f64, h: f64 },
    Empty,
}

pub fn area(s: &Shape) -> f64 {
    match s {
        Shape::Circle(r) if *r > 0.0 => 3.14 * r * r,
        Shape::Circle(_) => 0.0,
        Shape::Rect { w, h } => w * h,
        Shape::Empty => 0.0,
    }
}

pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "negative"
    } else if n == 0 {
        "zero"
    } else {
        "positive"
    }
}

pub fn sum_to(limit: u32) -> u32 {
    let mut total = 0;
    let mut i = 0;
    while i < limit {
        total += i;
        i += 1;
    }
    for j in 0..limit {
        total += j;
    }
    let mut acc = 0;
    loop {
        if acc >= limit {
            break;
        }
        acc += 1;
    }
    total + acc
}

pub fn first_even(values: &[i32]) -> Option<i32> {
    for &v in values {
        if v % 2 == 0 {
            return Some(v);
        }
    }
    None
}
