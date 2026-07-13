pub struct Subject {
    observers: Vec<Box<dyn Fn(i32)>>,
}

impl Subject {
    pub fn new() -> Self {
        Subject {
            observers: Vec::new(),
        }
    }
    pub fn subscribe(&mut self, f: Box<dyn Fn(i32)>) {
        self.observers.push(f);
    }
    pub fn notify(&self, event: i32) {
        for obs in &self.observers {
            obs(event);
        }
    }
}

pub fn matches_pattern(text: &str, pat: &str) -> bool {
    let t: Vec<char> = text.chars().collect();
    let p: Vec<char> = pat.chars().collect();
    fn go(t: &[char], p: &[char]) -> bool {
        if p.is_empty() {
            return t.is_empty();
        }
        let first = !t.is_empty() && (p[0] == '.' || p[0] == t[0]);
        if p.len() >= 2 && p[1] == '*' {
            go(t, &p[2..]) || (first && go(&t[1..], p))
        } else {
            first && go(&t[1..], &p[1..])
        }
    }
    go(&t, &p)
}

pub fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn to_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
