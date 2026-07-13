pub async fn fetch(id: u32) -> u32 {
    id * 2
}

pub async fn pipeline() -> u32 {
    let a = fetch(1).await;
    let b = fetch(a).await;
    let c = fetch(b).await;
    a + b + c
}

pub enum Switch {
    On,
    Off,
}

pub enum Mode {
    Auto,
    Manual,
}

pub fn combine(s: Switch, m: Mode) -> u8 {
    match (s, m) {
        (Switch::On, Mode::Auto) => 3,
        (Switch::On, Mode::Manual) => 2,
        (Switch::Off, _) => 0,
    }
}

pub fn pick(x: Option<i32>, y: Result<i32, String>) -> i32 {
    if let Some(v) = x {
        v
    } else if let Ok(w) = y {
        w
    } else {
        -1
    }
}

pub fn drain(mut it: std::vec::IntoIter<i32>) -> i32 {
    let mut sum = 0;
    while let Some(v) = it.next() {
        sum += v;
    }
    sum
}
