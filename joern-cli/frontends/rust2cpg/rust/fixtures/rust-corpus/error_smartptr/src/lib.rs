use std::cell::RefCell;
use std::num::ParseIntError;
use std::rc::Rc;

#[derive(Debug)]
pub enum AppError {
    Parse(ParseIntError),
    Empty,
}

impl From<ParseIntError> for AppError {
    fn from(e: ParseIntError) -> Self {
        AppError::Parse(e)
    }
}

pub fn parse_sum(parts: &[&str]) -> Result<i32, AppError> {
    if parts.is_empty() {
        return Err(AppError::Empty);
    }
    let mut total = 0;
    for p in parts {
        let n: i32 = p.parse()?;
        total += n;
    }
    Ok(total)
}

pub fn shared_counter() -> i32 {
    let counter = Rc::new(RefCell::new(0));
    let clone = Rc::clone(&counter);
    *clone.borrow_mut() += 5;
    let value = *counter.borrow();
    value
}

pub fn boxed() -> Box<i32> {
    let b = Box::new(42);
    b
}

pub fn first_or(values: Option<&[i32]>) -> i32 {
    values.and_then(|v| v.first()).copied().unwrap_or(-1)
}
