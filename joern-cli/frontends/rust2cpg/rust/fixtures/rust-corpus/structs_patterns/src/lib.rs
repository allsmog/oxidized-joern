pub struct Unit;

pub struct Wrapper(pub i32, String);

pub struct Named {
    pub a: i32,
    b: bool,
}

pub fn tuple_struct() -> i32 {
    let w = Wrapper(1, String::new());
    w.0
}

pub fn struct_rest(n: Named) -> i32 {
    let Named { a, .. } = n;
    a
}

pub fn slice_middle(s: &[i32]) -> i32 {
    match s {
        [first, .., last] => first + last,
        [only] => *only,
        [] => 0,
    }
}

pub fn matrix() -> [[i32; 3]; 2] {
    let zeros = [0; 3];
    [zeros, [1, 2, 3]]
}

pub fn index_matrix(m: &[[i32; 3]; 2]) -> i32 {
    m[0][1] + m[1][2]
}

pub fn sum_array<const N: usize>(arr: [i32; N]) -> i32 {
    let mut total = 0;
    for x in arr {
        total += x;
    }
    total
}

pub fn use_const_generic() -> i32 {
    sum_array([1, 2, 3, 4])
}
