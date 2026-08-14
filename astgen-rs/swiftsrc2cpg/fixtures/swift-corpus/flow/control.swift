func classify(_ n: Int) -> String {
    if n < 0 {
        return "negative"
    } else if n == 0 {
        return "zero"
    }

    switch n {
    case 1:
        return "one"
    case let x where x > 100:
        return "big"
    default:
        return "many"
    }
}

func process(_ items: [Int]) -> Int {
    var total = 0
    for item in items where item > 0 {
        total += item
    }
    let doubled = items.map { $0 * 2 }.filter { $0 > 4 }
    return total + doubled.count
}

func maybe(_ value: Int?) -> Int {
    guard let v = value else {
        return 0
    }
    return v
}
