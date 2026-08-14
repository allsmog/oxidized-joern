struct Temperature {
  var celsius: Double = 0.0

  var fahrenheit: Double {
    return celsius * 9 / 5 + 32
  }

  var kelvin: Double {
    get {
      return celsius + 273.15
    }
    set {
      celsius = newValue - 273.15
    }
  }

  var label: String = "temp" {
    willSet {
      print(newValue)
    }
    didSet {
      print(oldValue)
    }
  }
}
