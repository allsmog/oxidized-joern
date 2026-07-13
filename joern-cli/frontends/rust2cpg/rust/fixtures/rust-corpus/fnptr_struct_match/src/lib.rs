pub type BinOp = fn(i32, i32) -> i32;

pub fn apply(op: BinOp, a: i32, b: i32) -> i32 {
    op(a, b)
}

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn run() -> i32 {
    apply(add, 2, 3)
}

pub struct Config {
    a: i32,
    b: i32,
    c: i32,
}

pub fn base() -> Config {
    Config { a: 1, b: 2, c: 3 }
}

pub fn overridden() -> Config {
    Config { a: 10, ..base() }
}

pub fn small(x: Option<i32>) -> bool {
    matches!(x, Some(1) | Some(2) | Some(3))
}

pub fn alpha(c: char) -> bool {
    matches!(c, 'a'..='z' | 'A'..='Z')
}

pub fn find(grid: &[[i32; 3]; 3], target: i32) -> Option<(usize, usize)> {
    let mut result = None;
    'search: for i in 0..3 {
        for j in 0..3 {
            if grid[i][j] == target {
                result = Some((i, j));
                break 'search;
            }
        }
    }
    result
}
