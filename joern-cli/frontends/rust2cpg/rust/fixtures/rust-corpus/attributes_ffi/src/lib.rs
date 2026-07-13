/// A point in 2D space.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub type Pair = (i32, i32);
pub type Callback = fn(i32) -> i32;

extern "C" {
    fn abs(input: i32) -> i32;
}

#[no_mangle]
pub extern "C" fn doubled(a: i32) -> i32 {
    a * 2
}

pub union IntOrFloat {
    i: i32,
    f: f32,
}

pub unsafe fn read_int(u: &IntOrFloat) -> i32 {
    u.i
}

pub fn r#match(value: i32) -> i32 {
    let r#type = value + 1;
    r#type
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(1 + 1, 2);
    }
}
