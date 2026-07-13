func register<T>(_ type: T.Type) {
  print(type)
}

func make(_ meta: Int.Type) -> Int {
  return 0
}

protocol Drawable {}

func anyProto(_ p: Drawable.Protocol) {
  print(p)
}

let metatype: Int.Type = Int.self
