def render(name, volume)
  message = <<~TEXT
    Hello #{name}!
    Volume: #{volume}
  TEXT
  message
end

symbol = :"dynamic_#{1 + 1}"
interp = "value is #{symbol.inspect} and #{render('x', 1)}"
puts interp
