pub fn transpose(x: Option<Result<i32, String>>) -> Result<Option<i32>, String> {
    x.transpose()
}

pub fn aggregate(values: &[i32]) -> (i32, usize) {
    let product: i32 = values.iter().product();
    let split = values.partition_point(|&x| x < 5);
    (product, split)
}

pub fn mutate(mut v: Vec<i32>) -> Vec<i32> {
    v.rotate_right(1);
    v.truncate(8);
    if !v.is_empty() {
        v.swap_remove(0);
    }
    v.retain_mut(|x| {
        *x += 1;
        *x < 100
    });
    v
}

pub fn trim_url(s: &str) -> usize {
    let a = s.strip_prefix("http://").unwrap_or(s);
    let b = a.strip_suffix("/").unwrap_or(a);
    let first_dot = b.find('.').unwrap_or(0);
    let last_dot = b.rfind('.').unwrap_or(0);
    let repeated = "ab".repeat(3);
    first_dot + last_dot + repeated.len()
}

pub fn char_codes(c: char, code: u32) -> u32 {
    let upper = c.to_ascii_uppercase();
    let from = char::from_u32(code).unwrap_or('?');
    let digit = char::from_digit(5, 10).unwrap_or('?');
    (upper as u32) + (from as u32) + (digit as u32)
}

pub fn bits(x: u32) -> u32 {
    let ones = x.count_ones();
    let lz = x.leading_zeros();
    let rotated = x.rotate_left(4);
    let parsed = u32::from_str_radix("ff", 16).unwrap_or(0);
    ones + lz + rotated + parsed
}
