use std::str::FromStr;

pub struct Point {
    x: i32,
    y: i32,
}

impl FromStr for Point {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (a, b) = s.split_once(',').ok_or("missing comma")?;
        Ok(Point {
            x: a.trim().parse().map_err(|_| "bad x")?,
            y: b.trim().parse().map_err(|_| "bad y")?,
        })
    }
}

pub struct Node {
    value: i32,
    children: Vec<Node>,
}

impl Node {
    pub fn sum(&self) -> i32 {
        self.value + self.children.iter().map(|c| c.sum()).sum::<i32>()
    }
    pub fn depth(&self) -> usize {
        1 + self.children.iter().map(|c| c.depth()).max().unwrap_or(0)
    }
}

pub enum Shape {
    Dot(Point),
    Line(Point, Point),
}

pub fn measure(s: &Shape) -> i32 {
    match s {
        Shape::Dot(Point { x, y }) => x + y,
        Shape::Line(Point { x: x1, .. }, Point { x: x2, .. }) => x2 - x1,
    }
}
