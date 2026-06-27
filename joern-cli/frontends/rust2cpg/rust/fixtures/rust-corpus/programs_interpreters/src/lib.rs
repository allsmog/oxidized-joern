pub enum Token {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
}

pub fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => {
                chars.next();
            }
            '+' => {
                chars.next();
                tokens.push(Token::Plus);
            }
            '-' => {
                chars.next();
                tokens.push(Token::Minus);
            }
            '*' => {
                chars.next();
                tokens.push(Token::Star);
            }
            '/' => {
                chars.next();
                tokens.push(Token::Slash);
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            _ if c.is_ascii_digit() || c == '.' => {
                let mut num = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        num.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Num(num.parse().unwrap_or(0.0)));
            }
            _ => {
                chars.next();
            }
        }
    }
    tokens
}

pub fn eval_rpn(tokens: &[&str]) -> Option<f64> {
    let mut stack: Vec<f64> = Vec::new();
    for &tok in tokens {
        match tok {
            "+" | "-" | "*" | "/" => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(match tok {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    _ => a / b,
                });
            }
            n => stack.push(n.parse().ok()?),
        }
    }
    if stack.len() == 1 {
        stack.pop()
    } else {
        None
    }
}

pub fn to_rpn(tokens: &[&str]) -> Vec<String> {
    fn prec(op: &str) -> u8 {
        match op {
            "+" | "-" => 1,
            "*" | "/" => 2,
            _ => 0,
        }
    }
    let mut output = Vec::new();
    let mut ops: Vec<String> = Vec::new();
    for &tok in tokens {
        match tok {
            "+" | "-" | "*" | "/" => {
                while let Some(top) = ops.last() {
                    if prec(top) >= prec(tok) {
                        output.push(ops.pop().unwrap());
                    } else {
                        break;
                    }
                }
                ops.push(tok.to_string());
            }
            "(" => ops.push(tok.to_string()),
            ")" => {
                while let Some(top) = ops.last() {
                    if top != "(" {
                        output.push(ops.pop().unwrap());
                    } else {
                        break;
                    }
                }
                ops.pop();
            }
            n => output.push(n.to_string()),
        }
    }
    while let Some(op) = ops.pop() {
        output.push(op);
    }
    output
}

pub enum Sexpr {
    Atom(String),
    List(Vec<Sexpr>),
}

pub fn parse(tokens: &[String], pos: &mut usize) -> Option<Sexpr> {
    if *pos >= tokens.len() {
        return None;
    }
    let tok = &tokens[*pos];
    *pos += 1;
    if tok == "(" {
        let mut list = Vec::new();
        while *pos < tokens.len() && tokens[*pos] != ")" {
            if let Some(e) = parse(tokens, pos) {
                list.push(e);
            } else {
                break;
            }
        }
        *pos += 1;
        Some(Sexpr::List(list))
    } else {
        Some(Sexpr::Atom(tok.clone()))
    }
}

pub struct Parser<'a> {
    tokens: &'a [f64],
    ops: &'a [char],
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn expr(&mut self) -> f64 {
        let mut val = self.term();
        while self.pos < self.ops.len() && (self.ops[self.pos] == '+' || self.ops[self.pos] == '-') {
            let op = self.ops[self.pos];
            self.pos += 1;
            let rhs = self.term();
            val = if op == '+' { val + rhs } else { val - rhs };
        }
        val
    }
    fn term(&mut self) -> f64 {
        self.tokens[self.pos]
    }
}
