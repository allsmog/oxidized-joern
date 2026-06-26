pub enum Tree {
    Leaf(i32),
    Node(Box<Tree>, Box<Tree>),
}

pub fn sum(t: &Tree) -> i32 {
    match t {
        Tree::Leaf(v) => *v,
        Tree::Node(l, r) => sum(l) + sum(r),
    }
}

pub fn depth(t: &Tree) -> u32 {
    match t {
        Tree::Leaf(_) => 1,
        Tree::Node(l, r) => 1 + depth(l).max(depth(r)),
    }
}

pub fn render(label: &str, value: f64, count: usize) -> String {
    format!("{label:>12} = {value:8.3} ({count:04})")
}

pub fn apply(f: fn(i32) -> i32, x: i32) -> i32 {
    f(x)
}

pub fn doubled(x: i32) -> i32 {
    x * 2
}

pub fn higher_order() -> i32 {
    apply(doubled, 21)
}
