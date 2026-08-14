let doubleOptional: Int?? = nil
let tripleOptional: String??? = nil
let optionalArray: [Int?] = []
let optionalDict: [String: Int?] = [:]
let arrayOfOptionalArray: [[Int?]?] = []

func unwrap(_ x: Int??) -> Int? {
  return x ?? nil
}
