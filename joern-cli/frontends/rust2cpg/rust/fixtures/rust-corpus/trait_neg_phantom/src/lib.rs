use std::marker::PhantomData;
use std::ops::{Neg, Not};

pub trait Codec {
    const VERSION: u32;
    type Buffer;
    fn encode(&self) -> Self::Buffer;
    fn name(&self) -> &str {
        "codec"
    }
}

pub struct Raw;

impl Codec for Raw {
    const VERSION: u32 = 1;
    type Buffer = Vec<u8>;
    fn encode(&self) -> Vec<u8> {
        vec![]
    }
}

#[derive(Clone, Copy)]
pub struct Flag(bool);

impl Not for Flag {
    type Output = Flag;
    fn not(self) -> Flag {
        Flag(!self.0)
    }
}

#[derive(Clone, Copy)]
pub struct Num(i32);

impl Neg for Num {
    type Output = Num;
    fn neg(self) -> Num {
        Num(-self.0)
    }
}

#[derive(Default)]
pub struct Typed<T> {
    count: u32,
    _marker: PhantomData<T>,
}

pub fn make_typed() -> Typed<String> {
    Typed::default()
}
