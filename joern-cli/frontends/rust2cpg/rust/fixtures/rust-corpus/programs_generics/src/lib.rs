pub struct Stack<T> {
    items: Vec<T>,
}

impl<T> Stack<T> {
    pub fn new() -> Self {
        Stack { items: Vec::new() }
    }
    pub fn push(&mut self, x: T) {
        self.items.push(x);
    }
    pub fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn max_of<T: PartialOrd + Copy>(items: &[T]) -> Option<T> {
    let mut it = items.iter();
    let mut best = *it.next()?;
    for &x in it {
        if x > best {
            best = x;
        }
    }
    Some(best)
}

pub struct Pair<A, B> {
    pub first: A,
    pub second: B,
}

impl<A: Clone, B: Clone> Pair<A, B> {
    pub fn new(first: A, second: B) -> Self {
        Pair { first, second }
    }
    pub fn swap(self) -> Pair<B, A> {
        Pair {
            first: self.second,
            second: self.first,
        }
    }
}

pub struct Person {
    pub name: String,
    pub age: u32,
}

impl Person {
    pub fn sort_by_age(people: &mut [Person]) {
        people.sort_by(|a, b| a.age.cmp(&b.age).then(a.name.cmp(&b.name)));
    }
}

pub struct Meters(pub f64);
pub struct Feet(pub f64);

impl Meters {
    pub fn to_feet(&self) -> Feet {
        Feet(self.0 * 3.28084)
    }
    pub fn add(&self, other: &Meters) -> Meters {
        Meters(self.0 + other.0)
    }
}
