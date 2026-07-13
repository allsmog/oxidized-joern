pub fn option_chains(mut x: Option<i32>) -> i32 {
    let a = x.get_or_insert_with(|| 10);
    *a += 1;
    let b = Some(5).filter(|&n| n > 3).map_or(0, |n| n * 2);
    let c = Some(1).xor(None::<i32>).unwrap_or(0);
    x.unwrap_or(0) + b + c
}

pub fn result_chains(r: Result<i32, String>) -> i32 {
    let ok = r.is_ok_and(|n| n > 0);
    let mapped: i32 = Ok::<i32, String>(5).map_or(0, |n| n);
    (ok as i32) + mapped
}

pub fn reversed(v: &[i32]) -> Vec<i32> {
    let mut it = v.iter();
    let mut out = Vec::new();
    while let Some(&x) = it.next_back() {
        out.push(x);
    }
    out
}

pub fn lookup(sorted: &[(i32, &str)]) -> Result<usize, usize> {
    sorted.binary_search_by_key(&5, |&(k, _)| k)
}

pub fn pair_count(v: &[i32]) -> usize {
    v.chunks_exact(2).count()
}

pub enum Big {
    A = 1_000_000_000,
    B = -5,
    C = 42,
}

pub fn big_value(b: Big) -> i64 {
    b as i64
}
