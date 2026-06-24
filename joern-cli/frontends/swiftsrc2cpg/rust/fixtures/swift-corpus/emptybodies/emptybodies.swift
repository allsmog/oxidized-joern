func ifEmpty(_ c: Bool) {
  if c {}
}

func ifElseEmpty(_ c: Bool) {
  if c {} else {}
}

func closureEmpty() {
  let g = {}
  g()
}

func doEmpty() {
  do {} catch {}
}
