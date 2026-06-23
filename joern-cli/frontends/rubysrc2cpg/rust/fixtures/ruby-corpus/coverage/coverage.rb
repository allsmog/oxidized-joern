BEGIN { $started = true }
END { puts "done" }

module Greetings
  GREETING = "hi"

  class Speaker
    def initialize(name)
      @name = name
    end

    def speak(volume = 1, *rest, **opts, &blk)
      message = <<~TEXT
        Hello #{@name}!
        Volume: #{volume}
      TEXT
      yield message if block_given?
      message
    end

    def self.build(name)
      new(name)
    end
  end
end

undef foo rescue nil

proc_value = proc { |x| x + 1 }
lambda_value = ->(y) { y * 2 }
[1, 2, 3].each do |item|
  next if item == 2
  redo if false
  puts item
end

symbol = :"dynamic_#{1 + 1}"
interp = "value is #{symbol.inspect} and #{GREETING}"

ratio = 2r
pattern = /ab+c/ix

result = case interp
         when String then :string
         when Integer then :integer
         else :other
         end

config = { name: "ruby", tags: [1, 2, 3], meta: { level: 2 } }
case config
in { name: String => found_name, tags: [first, *] } if first.positive?
  puts "#{found_name} #{first}"
in { name: String } unless config.empty?
  puts "named"
in [*, 2, *post]
  puts post.inspect
in Integer | Float => number
  puts number
else
  puts "no match"
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

Greetings::Speaker.build("world").speak(2) { |m| puts m }
