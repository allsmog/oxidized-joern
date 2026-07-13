pub fn raw_examples() -> usize {
    let a: &str = r###"contains "## inside"###;
    let b: &str = r"simple";
    a.len() + b.len()
}

pub fn split_counts(s: &str) -> usize {
    let a: Vec<&str> = s.split(',').collect();
    let b: Vec<&str> = s.rsplit('/').collect();
    let c: Vec<&str> = s.splitn(2, '=').collect();
    a.len() + b.len() + c.len()
}

pub fn running(values: &[i32]) -> Vec<i32> {
    values
        .iter()
        .scan(0, |state, &x| {
            *state += x;
            Some(*state)
        })
        .skip_while(|&x| x < 3)
        .map_while(|x| if x < 100 { Some(x) } else { None })
        .inspect(|x| {
            let _ = x;
        })
        .collect()
}

pub fn nested(flag: bool) -> Result<Option<Vec<i32>>, String> {
    if flag {
        Ok(Some(vec![1, 2, 3]))
    } else {
        Ok(None)
    }
}

pub fn unwrap_nested(r: Result<Option<Vec<i32>>, String>) -> i32 {
    match r {
        Ok(Some(v)) => v.iter().sum(),
        Ok(None) => 0,
        Err(_) => -1,
    }
}
