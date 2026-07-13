use std::cell::RefCell;
use std::rc::Rc;

pub fn sort_dedup(mut v: Vec<(String, i32)>) -> Vec<(String, i32)> {
    v.sort_by_key(|x| x.1);
    v.dedup_by_key(|x| x.1);
    v
}

pub fn or_none(o: Option<i32>) -> Result<i32, String> {
    o.ok_or_else(|| "none".to_string())
}

pub fn zip_opts(a: Option<i32>, b: Option<i32>) -> Option<(i32, i32)> {
    a.zip(b)
}

pub fn recover(r: Result<i32, i32>) -> Result<i32, i32> {
    r.or_else(|e| Ok(e + 1))
}

pub fn checked_sum(values: &[i32]) -> Option<i32> {
    values.iter().try_fold(0i32, |acc, &x| acc.checked_add(x))
}

pub fn shared_push() -> usize {
    let shared = Rc::new(RefCell::new(Vec::new()));
    shared.borrow_mut().push(1);
    shared.borrow_mut().push(2);
    let len = shared.borrow().len();
    len
}

pub fn make_counter() -> impl FnMut() -> u32 {
    let mut count = 0u32;
    move || {
        count += 1;
        count
    }
}

pub fn count_three() -> u32 {
    let mut c = make_counter();
    c() + c() + c()
}
