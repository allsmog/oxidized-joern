pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Either<L, R> {
    pub fn is_left(&self) -> bool {
        matches!(self, Either::Left(_))
    }
    pub fn left(self) -> Option<L> {
        match self {
            Either::Left(l) => Some(l),
            Either::Right(_) => None,
        }
    }
}

pub fn classify(n: i32) -> Either<i32, String> {
    if n >= 0 {
        Either::Left(n)
    } else {
        Either::Right(format!("negative: {n}"))
    }
}

pub struct Calculator<F: Fn(i32) -> i32> {
    op: F,
}

impl<F: Fn(i32) -> i32> Calculator<F> {
    pub fn new(op: F) -> Self {
        Calculator { op }
    }
    pub fn apply(&self, x: i32) -> i32 {
        (self.op)(x)
    }
}

pub fn make_doubler() -> Calculator<impl Fn(i32) -> i32> {
    Calculator::new(|x| x * 2)
}
