use std::ops::AddAssign;

#[derive(Clone, Copy)]
pub struct Accum(i32);

impl AddAssign for Accum {
    fn add_assign(&mut self, other: Accum) {
        self.0 += other.0;
    }
}

pub fn total(mut a: Accum, b: Accum) -> Accum {
    a += b;
    a
}

pub fn sorted<T: Ord + Clone>(items: &[T]) -> Vec<T> {
    let mut v = items.to_vec();
    v.sort();
    v
}

pub fn count_digits(s: &str) -> u32 {
    let mut chars = s.chars();
    let mut count = 0;
    while let Some(c) = chars.next() {
        if c.is_numeric() {
            count += 1;
        }
    }
    count
}

pub fn sum_nonneg_rows(matrix: &[[i32; 3]; 3]) -> i32 {
    let mut total = 0;
    'rows: for row in matrix {
        for &cell in row {
            if cell < 0 {
                continue 'rows;
            }
            total += cell;
        }
    }
    total
}
