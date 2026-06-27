pub struct MinStack {
    stack: Vec<i32>,
    mins: Vec<i32>,
}

impl MinStack {
    pub fn new() -> Self {
        MinStack {
            stack: Vec::new(),
            mins: Vec::new(),
        }
    }
    pub fn push(&mut self, x: i32) {
        let m = self.mins.last().map_or(x, |&m| m.min(x));
        self.stack.push(x);
        self.mins.push(m);
    }
    pub fn pop(&mut self) -> Option<i32> {
        self.mins.pop();
        self.stack.pop()
    }
    pub fn min(&self) -> Option<i32> {
        self.mins.last().copied()
    }
}

pub struct Queue {
    inbox: Vec<i32>,
    outbox: Vec<i32>,
}

impl Queue {
    pub fn new() -> Self {
        Queue {
            inbox: Vec::new(),
            outbox: Vec::new(),
        }
    }
    pub fn enqueue(&mut self, x: i32) {
        self.inbox.push(x);
    }
    pub fn dequeue(&mut self) -> Option<i32> {
        if self.outbox.is_empty() {
            while let Some(x) = self.inbox.pop() {
                self.outbox.push(x);
            }
        }
        self.outbox.pop()
    }
}

pub struct Account {
    balance: i64,
}

impl Account {
    pub fn new() -> Self {
        Account { balance: 0 }
    }
    pub fn deposit(&mut self, amount: i64) {
        self.balance += amount;
    }
    pub fn withdraw(&mut self, amount: i64) -> Result<(), String> {
        if amount > self.balance {
            Err("insufficient funds".to_string())
        } else {
            self.balance -= amount;
            Ok(())
        }
    }
    pub fn balance(&self) -> i64 {
        self.balance
    }
}

pub enum Light {
    Red,
    Yellow,
    Green,
}

impl Light {
    pub fn next(self) -> Light {
        match self {
            Light::Red => Light::Green,
            Light::Green => Light::Yellow,
            Light::Yellow => Light::Red,
        }
    }
    pub fn duration(&self) -> u32 {
        match self {
            Light::Red => 30,
            Light::Yellow => 5,
            Light::Green => 25,
        }
    }
}
