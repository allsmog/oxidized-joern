macro_rules! max {
    ($a:expr, $b:expr) => {
        if $a > $b { $a } else { $b }
    };
    ($a:expr, $($rest:expr),+) => {
        max!($a, max!($($rest),+))
    };
}

pub fn biggest() -> i32 {
    max!(3, 7, 2, 9, 1)
}

pub fn literals() -> u64 {
    let million = 1_000_000u64;
    let byte = 0xFF_u8;
    let octal = 0o755;
    let binary = 0b1010_1010;
    let float = 3.141_592_f64;
    let exp = 1e9;
    million + byte as u64 + octal + binary + float as u64 + exp as u64
}

pub fn strings() -> usize {
    let raw = r"C:\path\to\file";
    let hashed = r#"contains "quotes" inside"#;
    let bytes = b"binary";
    raw.len() + hashed.len() + bytes.len()
}

pub fn labeled() -> i32 {
    let result = 'search: loop {
        for i in 0..10 {
            if i == 5 {
                break 'search i * 2;
            }
        }
        break 0;
    };
    result
}

pub fn bindings(x: i32) -> i32 {
    match x {
        n @ 1..=5 => n * 10,
        0 | 6 | 7 => 0,
        _ => -1,
    }
}
