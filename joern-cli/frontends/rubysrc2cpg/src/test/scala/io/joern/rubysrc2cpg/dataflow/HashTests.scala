package io.joern.rubysrc2cpg.dataflow

import io.joern.dataflowengineoss.language.*
import io.joern.rubysrc2cpg.testfixtures.RubyCode2CpgFixture
import io.shiftleft.semanticcpg.language.*

class HashTests extends RubyCode2CpgFixture(withPostProcessing = true, withDataFlow = true) {
  // Works in deprecated
  "Data flow through hash constructor" in {
    val cpg = code("""
                     |def foo(arg)
                     |hash = {1 => arg, 2 => arg}
                     |puts hash
                     |end
                     |
                     |x = 3
                     |foo(x)
                     |""".stripMargin)

    val source = cpg.identifier.name("x").l
    val sink   = cpg.call.name("puts").l
    sink.reachableByFlows(source).l.size shouldBe 2
  }

  // Works in deprecated - syntax error on new frontend
  "flow through hash containing splatting literal" in {
    val cpg = code("""
                     |x={:y=>1}
                     |z = {
                     |**x
                     |}
                     |puts z
                     |""".stripMargin)
    val source = cpg.identifier.name("x").l
    val sink   = cpg.call.name("puts").l
    sink.reachableByFlows(source).map(flowToResultPairs).toSet shouldBe Set(
      List(("**x", 4), ("<tmp-1>[<unknown>] = **x", 4), ("<tmp-1>", 3), ("z = {\n**x\n}", 3), ("puts z", 6)),
      List(("<tmp-1>[<unknown>] = **x", 4), ("<tmp-1>", 3), ("z = {\n**x\n}", 3), ("puts z", 6)),
      List(
        ("x={:y=>1}", 2),
        ("**x", 4),
        ("<tmp-1>[<unknown>] = **x", 4),
        ("<tmp-1>", 3),
        ("z = {\n**x\n}", 3),
        ("puts z", 6)
      )
    )
  }
}
