use std::borrow::Cow;

pub fn with_extra(input: &[i32]) -> Vec<i32> {
    let mut data: Cow<[i32]> = Cow::Borrowed(input);
    data.to_mut().push(99);
    data.into_owned()
}

pub fn emoji_name(c: char) -> &'static str {
    match c {
        '\u{1F600}' => "grin",
        '\u{1F62D}' => "cry",
        _ => "other",
    }
}

pub fn leak_string(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn to_bits(x: f32) -> u32 {
    x.to_bits()
}

pub fn from_bits(bits: u64) -> f64 {
    f64::from_bits(bits)
}

pub fn integer_ops(n: i64) -> i64 {
    n.abs() + n.pow(2) + n.signum()
}

pub trait Animal {
    fn name(&self) -> String;
}

pub trait Pet: Animal {
    fn owner(&self) -> String;
}

pub fn describe(p: &dyn Pet) -> String {
    format!("{} owned by {}", p.name(), p.owner())
}

pub fn byte_to_char(b: u8) -> char {
    b as char
}
