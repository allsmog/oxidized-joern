pub fn three_sum(mut nums: Vec<i32>) -> Vec<(i32, i32, i32)> {
    nums.sort();
    let mut out = Vec::new();
    for i in 0..nums.len() {
        let (mut lo, mut hi) = (i + 1, nums.len().saturating_sub(1));
        while lo < hi {
            let sum = nums[i] + nums[lo] + nums[hi];
            if sum == 0 {
                out.push((nums[i], nums[lo], nums[hi]));
                lo += 1;
                hi -= 1;
            } else if sum < 0 {
                lo += 1;
            } else {
                hi -= 1;
            }
        }
    }
    out
}

pub fn max_area(heights: &[i32]) -> i32 {
    let (mut lo, mut hi) = (0, heights.len() - 1);
    let mut best = 0;
    while lo < hi {
        let area = (hi - lo) as i32 * heights[lo].min(heights[hi]);
        best = best.max(area);
        if heights[lo] < heights[hi] {
            lo += 1;
        } else {
            hi -= 1;
        }
    }
    best
}

pub fn trap(h: &[i32]) -> i32 {
    let (mut lo, mut hi) = (0, h.len().saturating_sub(1));
    let (mut lmax, mut rmax, mut total) = (0, 0, 0);
    while lo < hi {
        if h[lo] < h[hi] {
            lmax = lmax.max(h[lo]);
            total += lmax - h[lo];
            lo += 1;
        } else {
            rmax = rmax.max(h[hi]);
            total += rmax - h[hi];
            hi -= 1;
        }
    }
    total
}

pub fn can_jump(nums: &[i32]) -> bool {
    let mut reach = 0;
    for (i, &n) in nums.iter().enumerate() {
        if i > reach {
            return false;
        }
        reach = reach.max(i + n as usize);
    }
    true
}

pub fn max_product(nums: &[i32]) -> i32 {
    let (mut max_p, mut min_p, mut result) = (nums[0], nums[0], nums[0]);
    for &n in &nums[1..] {
        if n < 0 {
            std::mem::swap(&mut max_p, &mut min_p);
        }
        max_p = n.max(max_p * n);
        min_p = n.min(min_p * n);
        result = result.max(max_p);
    }
    result
}
