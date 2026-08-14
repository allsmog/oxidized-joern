infix operator <+>
infix operator <->

func <+> (a: Int, b: Int) -> Int { return a + b }
func <-> (a: Int, b: Int) -> Int { return a - b }

let chain = 1 <+> 2 <+> 3 <+> 4
let mixed = 10 <+> 5 <-> 2
let single = 1 <+> 2
