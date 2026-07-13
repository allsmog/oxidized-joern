pub mod outer {
    pub(crate) fn helper() -> i32 {
        1
    }
    pub mod inner {
        pub(super) fn nested() -> i32 {
            super::helper() + 1
        }
        pub fn public() -> i32 {
            nested() * 2
        }
    }
    pub use inner::public as exported;
}

pub(crate) struct Internal {
    value: i32,
}

#[repr(u8)]
pub enum Flag {
    On = 1,
    Off = 0,
    Toggle = 255,
}

pub fn flag_value(f: Flag) -> u8 {
    f as u8
}

pub fn use_modules() -> i32 {
    outer::helper() + outer::inner::public() + outer::exported()
}
