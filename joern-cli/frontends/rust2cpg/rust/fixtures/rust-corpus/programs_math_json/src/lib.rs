pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
}

pub fn render(v: &Value, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => n.to_string(),
        Value::Str(s) => format!("\"{}\"", s),
        Value::Arr(items) => {
            let inner: Vec<String> = items
                .iter()
                .map(|x| format!("{}  {}", pad, render(x, indent + 1)))
                .collect();
            format!("[\n{}\n{}]", inner.join(",\n"), pad)
        }
    }
}

pub fn to_gray(n: u32) -> u32 {
    n ^ (n >> 1)
}

pub fn from_gray(mut g: u32) -> u32 {
    let mut n = 0;
    while g != 0 {
        n ^= g;
        g >>= 1;
    }
    n
}

pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub fn cross(&self, o: &V3) -> V3 {
        V3 {
            x: self.y * o.z - self.z * o.y,
            y: self.z * o.x - self.x * o.z,
            z: self.x * o.y - self.y * o.x,
        }
    }
    pub fn dot(&self, o: &V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
}

pub fn sqrt_newton(x: f64) -> f64 {
    if x < 0.0 {
        return f64::NAN;
    }
    let mut guess = x;
    for _ in 0..20 {
        guess = (guess + x / guess) / 2.0;
    }
    guess
}
