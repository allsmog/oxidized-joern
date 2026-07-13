BEGIN { $started = true }
END { puts "done" }

module Greetings
  GREETING = "hi"

  class Speaker
    def initialize(name)
      @name = name
    end

    def speak
      "#{GREETING} #{@name}"
    end
  end

  def self.default
    Speaker.new("world")
  end
end

Greetings::Speaker.new("ruby").speak
