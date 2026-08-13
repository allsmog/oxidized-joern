import sbt.*
import sbt.Keys.*

object Projects {
  val frontendsRoot = file("joern-cli/frontends")

  lazy val joerncli          = project.in(file("joern-cli"))
  lazy val querydb           = project.in(file("querydb"))
  lazy val console           = project.in(file("console"))
  lazy val dataflowengineoss = project.in(file("dataflowengineoss"))
  lazy val macros            = project.in(file("macros"))
  lazy val semanticcpg       = project.in(file("semanticcpg"))

  // The Rust-only project scope removed c2cpg, gosrc2cpg, jssrc2cpg,
  // pysrc2cpg, rubysrc2cpg, and rust2cpg. This is a scope decision, not a
  // claim that every cpg-rs language mode has reached Joern parity.
  // javasrc2cpg remains as a library for kotlin2cpg mixed-source interop.
  lazy val javasrc2cpg   = project.in(frontendsRoot / "javasrc2cpg")
  lazy val ghidra2cpg    = project.in(frontendsRoot / "ghidra2cpg")
  lazy val x2cpg         = project.in(frontendsRoot / "x2cpg")
  lazy val php2cpg       = project.in(frontendsRoot / "php2cpg")
  lazy val swiftsrc2cpg  = project.in(frontendsRoot / "swiftsrc2cpg")
  lazy val jimple2cpg    = project.in(frontendsRoot / "jimple2cpg")
  lazy val kotlin2cpg    = project.in(frontendsRoot / "kotlin2cpg")
  lazy val csharpsrc2cpg = project.in(frontendsRoot / "csharpsrc2cpg")
  lazy val abap2cpg      = project.in(frontendsRoot / "abap2cpg")

  lazy val linterRules = project.in(file("linter-rules"))

}
