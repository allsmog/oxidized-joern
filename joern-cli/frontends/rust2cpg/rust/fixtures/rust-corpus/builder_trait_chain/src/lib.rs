#[derive(Default)]
pub struct ConfigBuilder {
    name: Option<String>,
    age: Option<u32>,
    active: bool,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        ConfigBuilder::default()
    }
    pub fn name(mut self, n: &str) -> Self {
        self.name = Some(n.to_string());
        self
    }
    pub fn age(mut self, a: u32) -> Self {
        self.age = Some(a);
        self
    }
    pub fn active(mut self, flag: bool) -> Self {
        self.active = flag;
        self
    }
    pub fn build(self) -> String {
        format!("{:?} {:?} {}", self.name, self.age, self.active)
    }
}

pub fn configure() -> String {
    ConfigBuilder::new().name("x").age(3).active(true).build()
}

pub trait Base {
    fn base(&self) -> i32;
}

pub trait Middle: Base {
    fn middle(&self) -> i32 {
        self.base() + 1
    }
}

pub trait Top: Middle {
    fn top(&self) -> i32 {
        self.middle() + 1
    }
}

pub struct Impl;

impl Base for Impl {
    fn base(&self) -> i32 {
        1
    }
}
impl Middle for Impl {}
impl Top for Impl {}

pub fn chain(i: &Impl) -> i32 {
    i.top()
}
