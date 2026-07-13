use std::collections::HashSet;

pub fn mutate(values: &mut [i32]) {
    values.rotate_left(1);
    for x in values.iter_mut() {
        *x *= 2;
    }
    if values.len() >= 2 {
        let (a, b) = values.split_at_mut(1);
        std::mem::swap(&mut a[0], &mut b[0]);
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct Key {
    id: u32,
    name: String,
}

pub fn unique_keys(keys: Vec<Key>) -> usize {
    let set: HashSet<Key> = keys.into_iter().collect();
    set.len()
}

pub const GRID: [[i32; 3]; 3] = [[0; 3]; 3];

pub fn grid_cell() -> i32 {
    GRID[1][2] + GRID.len() as i32
}
