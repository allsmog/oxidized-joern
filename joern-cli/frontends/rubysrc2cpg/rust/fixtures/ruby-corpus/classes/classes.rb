class Speaker
  GREETING = "hi"

  def initialize(name)
    @name = name
  end

  def speak(volume = 1, *rest, **opts, &blk)
    yield @name if block_given?
    "#{GREETING} #{@name} at #{volume}"
  end

  def self.build(name)
    new(name)
  end
end

undef foo rescue nil

ratio = 2r
pattern = /ab+c/ix

result = case ratio
         when String then :string
         when Integer then :integer
         else :other
         end

Speaker.build("world").speak(2) { |m| puts m }
