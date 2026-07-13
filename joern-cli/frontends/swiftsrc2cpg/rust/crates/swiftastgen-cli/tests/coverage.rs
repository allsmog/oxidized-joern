//! Construct-coverage harness for the `SwiftAstGen` CLI.
//!
//! Runs the real CLI over an inline Swift fixture that exercises the common
//! language constructs the Scala `swiftsrc2cpg` frontend cares about, then
//! asserts two things:
//!   1. the run succeeds, and
//!   2. nothing degraded to a placeholder, i.e. the CLI prints no
//!      `swiftastgen: N unsupported node(s) degraded to placeholders: ...`
//!      summary on stderr (see `report_unsupported_nodes` in `src/main.rs`).
//!
//! Every construct below currently maps to a precise SwiftSyntax node. If a
//! future construct legitimately degrades, exclude it from `FIXTURE` and note
//! why in a comment so this stays a green, meaningful guard rather than a
//! silenced one.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

/// Inline Swift source covering the constructs we want coverage over. Each
/// labelled section names the construct(s) it exercises. Kept deliberately
/// dense so the single CLI run touches a broad slice of the emitter.
const FIXTURE: &str = r#"
import Foundation

// MARK: protocol, protocol composition (A & B), associated requirements
protocol Named {
    var name: String { get }
}

protocol Aged {
    var age: Int { get set }
}

// protocol composition `A & B` used as a parameter type
func describe(_ value: Named & Aged) -> String {
    return "\(value.name): \(value.age)"
}

// MARK: struct with stored + computed properties
struct Point {
    var x: Double
    var y: Double

    // computed property
    var magnitude: Double {
        return (x * x + y * y).squareRoot()
    }

    // stored property with default + property observer
    var label: String = "point" {
        didSet {
            print(label)
        }
    }
}

// MARK: class, inheritance, designated init, self, deinit
class Animal: Named {
    let name: String
    init(name: String) {
        self.name = name
    }
    deinit {
        print("bye")
    }
}

class Dog: Animal {
    func bark() -> String { "woof" }
}

// MARK: enum with associated + raw values, switch with patterns
enum Shape {
    case circle(radius: Double)
    case rectangle(width: Double, height: Double)
    case point
}

enum Direction: Int {
    case north = 0
    case south = 1
}

func area(of shape: Shape) -> Double {
    // switch with value-binding patterns, tuple pattern, where clause, wildcard
    switch shape {
    case .circle(let radius):
        return 3.14 * radius * radius
    case .rectangle(let width, let height) where width == height:
        return width * height
    case .rectangle(let width, let height):
        return width * height
    case .point:
        return 0
    }
}

// MARK: generics with `where` clause + protocol constraints
func firstEqual<T: Equatable>(_ items: [T], to target: T) -> T? where T: Hashable {
    for item in items {
        if item == target {
            return item
        }
    }
    return nil
}

// MARK: optionals, optional chaining, guard, if-let, optional binding
func greeting(for dog: Dog?) -> String {
    // guard with optional binding
    guard let dog = dog else {
        return "no dog"
    }
    // optional chaining + nil-coalescing
    let count = dog.name.first?.isLetter ?? false
    if count {
        return dog.bark()
    }
    return dog.name
}

// MARK: for-in with where, ranges, closures, trailing closures, higher-order
func sumOfSquares(_ values: [Int]) -> Int {
    var total = 0
    // for-in with a where clause and a range
    for value in 0 ..< values.count where values[value] > 0 {
        total += values[value] * values[value]
    }
    // closure passed inline + trailing closure form
    let mapped = values.map({ (n: Int) -> Int in n * n })
    let filtered = mapped.filter { $0 > 1 }
    // bare operator reference as a first-class function value
    return total + filtered.reduce(0, +)
}

// MARK: async / await, throws / try, do-catch
enum NetworkError: Error {
    case offline
}

func fetch() async throws -> String {
    throw NetworkError.offline
}

func load() async -> String {
    do {
        // try with await
        let body = try await fetch()
        return body
    } catch {
        return "failed"
    }
}

// MARK: #keyPath(...), discard `_ = x`
class Person: NSObject {
    @objc var fullName: String = ""
}

func keyPaths() {
    let kp = #keyPath(Person.fullName)
    // explicit discard assignment
    _ = kp
    _ = area(of: .point)
}

// MARK: bracket-qualified types
func bracketQualifiedTypes(_ item: [Point].Element, _ keys: [String: Point].Keys?) {
    _ = item
    _ = keys
}

// MARK: extension with a method + computed property
extension Point {
    func translated(byX dx: Double, byY dy: Double) -> Point {
        return Point(x: x + dx, y: y + dy)
    }

    var isOrigin: Bool {
        x == 0 && y == 0
    }
}
"#;

/// Marker emitted by `report_unsupported_nodes` in `src/main.rs`. Its presence
/// on stderr means at least one tree-sitter node kind degraded to a
/// `Missing*` placeholder for this run.
const UNSUPPORTED_MARKER: &str = "unsupported node(s) degraded to placeholders";

#[test]
fn common_swift_constructs_map_without_degrading_to_placeholders() {
    let input = TempDir::new().unwrap();
    let output = TempDir::new().unwrap();
    let source = input.path().join("coverage.swift");
    fs::write(&source, FIXTURE).unwrap();

    let assert = Command::cargo_bin("SwiftAstGen")
        .unwrap()
        .args(["-o"])
        .arg(output.path())
        .arg(input.path())
        .assert()
        .success();

    // The CLI prints the unsupported-node summary (if any) to stderr at the end
    // of the run. Fail loudly listing the degraded kinds so regressions name the
    // construct that started falling through.
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        !stderr.contains(UNSUPPORTED_MARKER),
        "SwiftAstGen degraded one or more nodes to placeholders for the coverage \
         fixture; every covered construct is expected to map precisely.\nstderr:\n{stderr}"
    );

    // Sanity check that the run actually produced AST output for the fixture.
    assert!(
        output.path().join("coverage.swift.json").exists(),
        "expected coverage.swift.json to be generated under {}",
        output.path().display()
    );
}
