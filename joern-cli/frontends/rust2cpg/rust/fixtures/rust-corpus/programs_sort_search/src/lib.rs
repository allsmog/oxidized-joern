pub fn selection_sort(v: &mut Vec<i32>) {
    for i in 0..v.len() {
        let mut min = i;
        for j in (i + 1)..v.len() {
            if v[j] < v[min] {
                min = j;
            }
        }
        v.swap(i, min);
    }
}

pub fn lower_bound(v: &[i32], target: i32) -> usize {
    let (mut lo, mut hi) = (0, v.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if v[mid] < target {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

pub fn sort_colors(v: &mut Vec<i32>) {
    let (mut lo, mut mid, mut hi) = (0, 0, v.len());
    while mid < hi {
        match v[mid] {
            0 => {
                v.swap(lo, mid);
                lo += 1;
                mid += 1;
            }
            2 => {
                hi -= 1;
                v.swap(mid, hi);
            }
            _ => mid += 1,
        }
    }
}

pub fn prefix_sums(v: &[i32]) -> Vec<i32> {
    let mut out = Vec::with_capacity(v.len() + 1);
    out.push(0);
    for &x in v {
        out.push(out.last().unwrap() + x);
    }
    out
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

pub fn vec_distance(a: &[bool], b: &[bool]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}
