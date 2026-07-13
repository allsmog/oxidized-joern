pub fn analyze(text: &str) -> usize {
    let words: Vec<&str> = text.split_whitespace().collect();
    let lines = text.lines().count();
    let has_x = text.find('x').is_some();
    let starts = text.starts_with("a");
    words.len() + lines + (has_x as usize) + (starts as usize)
}

pub fn normalize(s: &str) -> String {
    s.split(',')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("|")
}

pub fn geometry(x: f64) -> f64 {
    x.sqrt() + x.powi(2) + x.abs() + x.floor() + x.max(0.0)
}

pub fn combine(x: Option<i32>, y: Option<i32>) -> Option<i32> {
    x.zip(y).map(|(a, b)| a + b)
}

pub fn transform(r: Result<i32, String>) -> Result<i32, usize> {
    r.map_err(|e| e.len()).and_then(|v| Ok(v * 2))
}
