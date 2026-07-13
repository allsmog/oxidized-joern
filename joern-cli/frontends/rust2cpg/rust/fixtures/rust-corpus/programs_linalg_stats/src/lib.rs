pub fn determinant(m: &[Vec<f64>]) -> f64 {
    let n = m.len();
    if n == 1 {
        return m[0][0];
    }
    if n == 2 {
        return m[0][0] * m[1][1] - m[0][1] * m[1][0];
    }
    let mut det = 0.0;
    for col in 0..n {
        let minor: Vec<Vec<f64>> = m[1..]
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .filter(|(c, _)| *c != col)
                    .map(|(_, &v)| v)
                    .collect()
            })
            .collect();
        let sign = if col % 2 == 0 { 1.0 } else { -1.0 };
        det += sign * m[0][col] * determinant(&minor);
    }
    det
}

pub fn inverse_2x2(m: [[f64; 2]; 2]) -> Option<[[f64; 2]; 2]> {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [m[1][1] * inv_det, -m[0][1] * inv_det],
        [-m[1][0] * inv_det, m[0][0] * inv_det],
    ])
}

pub fn cramer(a: [[f64; 2]; 2], b: [f64; 2]) -> Option<(f64, f64)> {
    let det = a[0][0] * a[1][1] - a[0][1] * a[1][0];
    if det.abs() < 1e-12 {
        return None;
    }
    let dx = b[0] * a[1][1] - a[0][1] * b[1];
    let dy = a[0][0] * b[1] - b[0] * a[1][0];
    Some((dx / det, dy / det))
}

pub fn mean_variance(data: &[f64]) -> (f64, f64) {
    let n = data.len() as f64;
    let mean = data.iter().sum::<f64>() / n;
    let variance = data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    (mean, variance)
}

pub fn linear_fit(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let sx: f64 = xs.iter().sum();
    let sy: f64 = ys.iter().sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let sxx: f64 = xs.iter().map(|x| x * x).sum();
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    let intercept = (sy - slope * sx) / n;
    (slope, intercept)
}
