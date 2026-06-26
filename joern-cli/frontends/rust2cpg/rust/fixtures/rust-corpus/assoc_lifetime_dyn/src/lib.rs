pub trait Producer {
    type Output: Clone;
    fn produce(&self) -> Self::Output;
}

pub struct IntProducer;

impl Producer for IntProducer {
    type Output = i32;
    fn produce(&self) -> i32 {
        42
    }
}

pub trait Source {
    fn get<'a>(&'a self) -> &'a str;
}

pub struct Holder {
    data: String,
}

impl Source for Holder {
    fn get<'a>(&'a self) -> &'a str {
        &self.data
    }
}

pub trait Task {
    fn run(&self) -> i32;
}

pub fn spawn(t: &(dyn Task + Send)) -> i32 {
    t.run()
}

pub struct Widget {
    id: u32,
}

impl Widget {
    pub fn id(&self) -> u32 {
        self.id
    }
}

impl Widget {
    pub fn doubled(&self) -> u32 {
        self.id * 2
    }
}

impl Default for Widget {
    fn default() -> Self {
        Widget { id: 0 }
    }
}
