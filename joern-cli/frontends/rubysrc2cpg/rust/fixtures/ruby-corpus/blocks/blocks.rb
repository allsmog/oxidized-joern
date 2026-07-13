proc_value = proc { |x| x + 1 }
lambda_value = ->(y) { y * 2 }

[1, 2, 3].each do |item|
  next if item == 2
  redo if false
  puts item
end

def stream(values)
  values.each do |line|
    print line if (line =~ /start/)...(line =~ /stop/)
  end
rescue StandardError => e
  retry if e.message.empty?
ensure
  puts "cleaned"
end

def select_inclusive(lines)
  lines.each { |line| print line if (line == "begin")..(line == "end") }
end

puts proc_value.call(lambda_value.call(2))
