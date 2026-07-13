pub fn spiral(m: &[Vec<i32>]) -> Vec<i32> {
    let mut out = Vec::new();
    if m.is_empty() {
        return out;
    }
    let (mut top, mut bot, mut left, mut right) =
        (0i32, m.len() as i32 - 1, 0i32, m[0].len() as i32 - 1);
    while top <= bot && left <= right {
        for c in left..=right {
            out.push(m[top as usize][c as usize]);
        }
        top += 1;
        for r in top..=bot {
            out.push(m[r as usize][right as usize]);
        }
        right -= 1;
        if top <= bot {
            for c in (left..=right).rev() {
                out.push(m[bot as usize][c as usize]);
            }
            bot -= 1;
        }
        if left <= right {
            for r in (top..=bot).rev() {
                out.push(m[r as usize][left as usize]);
            }
            left += 1;
        }
    }
    out
}

pub fn search_rotated(nums: &[i32], target: i32) -> Option<usize> {
    let (mut lo, mut hi) = (0, nums.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if nums[mid] == target {
            return Some(mid);
        }
        if nums[lo] <= nums[mid] {
            if nums[lo] <= target && target < nums[mid] {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        } else if nums[mid] < target && target <= nums[hi - 1] {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    None
}

pub fn single_number(nums: &[i32]) -> i32 {
    nums.iter().fold(0, |acc, &n| acc ^ n)
}

pub fn missing(nums: &[i32]) -> i32 {
    let n = nums.len() as i32;
    (0..=n).sum::<i32>() - nums.iter().sum::<i32>()
}

pub fn majority(nums: &[i32]) -> i32 {
    let mut count = 0;
    let mut candidate = nums[0];
    for &n in nums {
        if count == 0 {
            candidate = n;
        }
        count += if n == candidate { 1 } else { -1 };
    }
    candidate
}

pub fn product_except_self(nums: &[i32]) -> Vec<i32> {
    let n = nums.len();
    let mut out = vec![1; n];
    let mut left = 1;
    for i in 0..n {
        out[i] = left;
        left *= nums[i];
    }
    let mut right = 1;
    for i in (0..n).rev() {
        out[i] *= right;
        right *= nums[i];
    }
    out
}
