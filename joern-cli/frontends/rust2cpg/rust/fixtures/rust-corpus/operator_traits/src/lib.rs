use std::ops::{Index, IndexMut};

pub struct Grid {
    cells: Vec<i32>,
    width: usize,
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        Grid {
            cells: vec![0; width * height],
            width,
        }
    }
}

impl Index<(usize, usize)> for Grid {
    type Output = i32;
    fn index(&self, (row, col): (usize, usize)) -> &i32 {
        &self.cells[row * self.width + col]
    }
}

impl IndexMut<(usize, usize)> for Grid {
    fn index_mut(&mut self, (row, col): (usize, usize)) -> &mut i32 {
        &mut self.cells[row * self.width + col]
    }
}

pub struct CaseInsensitive(pub String);

impl PartialEq for CaseInsensitive {
    fn eq(&self, other: &CaseInsensitive) -> bool {
        self.0.to_lowercase() == other.0.to_lowercase()
    }
}

pub trait Logger {
    fn log(&self, msg: &str);
}

pub struct App {
    logger: Box<dyn Logger>,
}

impl App {
    pub fn new(logger: Box<dyn Logger>) -> Self {
        App { logger }
    }
    pub fn run(&self) {
        self.logger.log("start");
    }
}
