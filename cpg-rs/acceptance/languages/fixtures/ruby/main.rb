def source(value)
  value
end

def transform(value)
  value
end

def sink(value)
  puts(value)
end

def main(user)
  raw = source(user)
  clean = transform(raw)
  sink(clean)
end
