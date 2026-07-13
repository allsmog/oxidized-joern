enum Direction: Int {
  case north = 0
  case south = 1
  case east
  case west
}

enum Barcode {
  case upc(Int, Int, Int)
  case qrCode(String)
}

indirect enum Tree {
  case leaf(Int)
  case node(Tree, Tree)
}
