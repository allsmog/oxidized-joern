use std::convert::TryFrom;
use std::fmt;

pub struct Celsius(pub f64);
pub struct Fahrenheit(pub f64);

impl From<Celsius> for Fahrenheit {
    fn from(c: Celsius) -> Self {
        Fahrenheit(c.0 * 9.0 / 5.0 + 32.0)
    }
}

pub fn to_fahrenheit(c: Celsius) -> Fahrenheit {
    c.into()
}

pub struct Even(pub i32);

impl TryFrom<i32> for Even {
    type Error = String;
    fn try_from(v: i32) -> Result<Self, String> {
        if v % 2 == 0 {
            Ok(Even(v))
        } else {
            Err(format!("{v} is odd"))
        }
    }
}

pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

pub enum Direction {
    North,
    South,
    East,
    West,
}

impl Direction {
    pub fn opposite(&self) -> Direction {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Direction::North => "N",
            Direction::South => "S",
            Direction::East => "E",
            Direction::West => "W",
        }
    }
}
