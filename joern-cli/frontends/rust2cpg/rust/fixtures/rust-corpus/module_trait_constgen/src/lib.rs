pub mod outer {
    pub mod inner {
        pub(in crate::outer) fn restricted() -> i32 {
            1
        }
        pub fn open() -> i32 {
            restricted() + 1
        }
    }
    pub fn use_inner() -> i32 {
        inner::open()
    }
}

pub trait Shape {
    const SIDES: u32;
    fn area(&self) -> f64;
    fn describe(&self) -> String {
        format!("{}-sided, area {}", Self::SIDES, self.area())
    }
}

pub struct Square {
    side: f64,
}

impl Shape for Square {
    const SIDES: u32 = 4;
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

pub fn identity_matrix<const N: usize>() -> [[u8; N]; N] {
    let mut m = [[0u8; N]; N];
    let mut i = 0;
    while i < N {
        m[i][i] = 1;
        i += 1;
    }
    m
}

pub fn three() -> [[u8; 3]; 3] {
    identity_matrix::<3>()
}
