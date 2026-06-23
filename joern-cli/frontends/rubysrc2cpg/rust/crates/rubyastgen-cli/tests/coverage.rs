//! Coverage gate for the `rubyastgen` CLI.
//!
//! Runs the real binary over an inline Ruby fixture that exercises the common
//! constructs plus the recently-mapped ones (pattern matching, flip-flops,
//! `BEGIN`/`END`, `redo`/`retry`, rationals, `undef`, interpolation, heredocs,
//! regex options, …). The fixture is held at *zero* `__unknown` fallbacks: the
//! CLI prints a `rubyastgen: N unmapped node(s): …` summary to stderr whenever a
//! `lib-ruby-parser` node falls through to the `__unknown` catch-all, and this
//! test fails (listing the offending variants) if that summary ever appears.
//!
//! Constructs that map to an existing node kind rather than a dedicated type
//! string (and so do not surface their own `"type"` literal) are still covered
//! because they parse and lower cleanly without an `__unknown` fallback:
//!   * heredocs (`<<~TEXT`) lower to `str`/`dstr` (see `lower_heredoc`);
//!   * regex options (`/…/ix`) are folded into the `regexp` node, so the
//!     `regopt` child is not re-emitted as a standalone node.
//!
//! No construct is intentionally excluded from the gate: every node the fixture
//! produces is mapped, keeping the unmapped tally at zero.

use assert_cmd::Command;
use tempfile::tempdir;

/// Inline Ruby exercising common + newly-mapped constructs. Kept in one file so
/// a single CLI run drains the whole unmapped-node tally.
const COVERAGE_FIXTURE: &str = r##"BEGIN { $started = true }
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
"##;

#[test]
fn coverage_fixture_emits_no_unmapped_nodes() {
    let tmp = tempdir().expect("creating temp dir");
    let input = tmp.path().join("coverage.rb");
    let out = tmp.path().join("out");
    std::fs::write(&input, COVERAGE_FIXTURE).expect("writing fixture");

    let assert = Command::cargo_bin("rubyastgen")
        .expect("locating rubyastgen binary")
        .arg("-o")
        .arg(&out)
        .arg(&input)
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    let unmapped: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("rubyastgen:") && line.contains("unmapped node(s)"))
        .collect();
    assert!(
        unmapped.is_empty(),
        "CLI reported unmapped nodes for the coverage fixture; either map the \
         construct or exclude it from the fixture:\n{}",
        unmapped.join("\n")
    );

    // Sanity: the JSON output exists and never contains the `__unknown`
    // fallback node, independent of the stderr summary.
    let json =
        std::fs::read_to_string(out.join("coverage.rb.json")).expect("reading emitted JSON output");
    assert!(
        !json.contains("__unknown"),
        "emitted JSON contains an __unknown fallback node"
    );
}
