@main def main() = {
  // Frontends for cpg-rs-covered languages (c/cpp/java-src/js/python/go/
  // ruby/rust) are removed from the Scala distribution — cpg-rs covers them.
  assert(importCode.ghidra.isAvailable, "GHIDRA frontend should be available, but isn't")
  assert(importCode.kotlin.isAvailable, "KOTLIN frontend should be available, but isn't")
  assert(importCode.jvm.isAvailable, "JVM frontend should be available, but isn't")
  assert(importCode.php.isAvailable, "PHP frontend should be available, but isn't")

  println("frontends smoketest successful: all required frontends are available")

}
