func onMain(_ work: @escaping () -> Void) {
  work()
}

func assertion(_ condition: @autoclosure () -> Bool) {
  _ = condition()
}

func combined(_ transform: @escaping (Int) -> Int, value: Int) -> Int {
  return transform(value)
}
