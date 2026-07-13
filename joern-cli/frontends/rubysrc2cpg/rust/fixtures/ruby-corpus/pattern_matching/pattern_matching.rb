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
