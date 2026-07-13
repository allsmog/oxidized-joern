use std::collections::{BTreeMap, HashSet};

pub fn sum_doubled(inputs: &[&str]) -> Result<i32, std::num::ParseIntError> {
    let parse = |s: &str| -> Result<i32, std::num::ParseIntError> {
        let n: i32 = s.parse()?;
        Ok(n * 2)
    };
    let mut total = 0;
    for s in inputs {
        total += parse(s)?;
    }
    Ok(total)
}

pub trait Item {
    fn weight(&self) -> u32;
}

pub fn by_weight(mut items: Vec<Box<dyn Item>>) -> Vec<Box<dyn Item>> {
    items.sort_by_key(|i| i.weight());
    items
}

pub fn adjust(m: &mut BTreeMap<i32, i32>) {
    for (_, v) in m.range_mut(2..5) {
        *v += 100;
    }
    for v in m.values_mut() {
        *v *= 2;
    }
}

pub fn sym_diff_count(a: &HashSet<i32>, b: &HashSet<i32>) -> usize {
    a.symmetric_difference(b).count()
}
