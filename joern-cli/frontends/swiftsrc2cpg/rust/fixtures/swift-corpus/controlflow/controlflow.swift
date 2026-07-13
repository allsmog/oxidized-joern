func cleanup() {}

func process() {
  defer {
    cleanup()
  }
  outer: for i in 0..<10 {
    while i > 5 {
      break outer
    }
    continue outer
  }
  var n = 0
  repeat {
    n += 1
  } while n < 3
}

func emptyLoops(_ n: Int) {
  for _ in 0..<n {}
  var i = 0
  while i < n {}
}
