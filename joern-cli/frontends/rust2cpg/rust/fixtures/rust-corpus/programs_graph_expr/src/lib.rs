pub fn count_components(n: usize, edges: &[(usize, usize)]) -> usize {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    for &(a, b) in edges {
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        parent[ra] = rb;
    }
    (0..n).filter(|&i| find(&mut parent, i) == i).count()
}

pub fn has_cycle(adj: &[Vec<usize>]) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }
    fn dfs(u: usize, adj: &[Vec<usize>], colors: &mut Vec<Color>) -> bool {
        colors[u] = Color::Gray;
        for &v in &adj[u] {
            if colors[v] == Color::Gray {
                return true;
            }
            if colors[v] == Color::White && dfs(v, adj, colors) {
                return true;
            }
        }
        colors[u] = Color::Black;
        false
    }
    let mut colors = vec![Color::White; adj.len()];
    (0..adj.len()).any(|i| colors[i] == Color::White && dfs(i, adj, &mut colors))
}

pub enum Expr {
    Num(f64),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
}

pub fn eval(e: &Expr) -> f64 {
    match e {
        Expr::Num(n) => *n,
        Expr::Add(a, b) => eval(a) + eval(b),
        Expr::Sub(a, b) => eval(a) - eval(b),
        Expr::Mul(a, b) => eval(a) * eval(b),
    }
}

pub fn poly_eval(coeffs: &[f64], x: f64) -> f64 {
    coeffs.iter().rev().fold(0.0, |acc, &c| acc * x + c)
}
