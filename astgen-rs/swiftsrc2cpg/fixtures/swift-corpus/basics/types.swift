import Foundation

struct Point {
    var x: Int
    var y: Int

    func magnitudeSquared() -> Int {
        return x * x + y * y
    }
}

class Shape {
    var name: String

    init(name: String) {
        self.name = name
    }

    func describe() -> String {
        return "Shape: \(name)"
    }
}

enum Direction {
    case north
    case south
    case custom(angle: Double)
}

protocol Drawable {
    func draw()
}

extension Point: Drawable {
    func draw() {
        print("Point(\(x), \(y))")
    }
}
