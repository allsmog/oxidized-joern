package io.joern.pysrc2cpg.passes

import io.joern.dataflowengineoss.language.toExtendedCfgNode
import io.joern.pysrc2cpg.testfixtures.PySrc2CpgFixture
import io.shiftleft.semanticcpg.language.*

class YieldTests extends PySrc2CpgFixture(withOssDataflow = true) {

  "yield expressions" should {
    lazy val cpg = code("""
        |def gen(x):
        |    secret = 42
        |    yield secret
        |    yield
        |    yield from range(x)
        |""".stripMargin)

    "model `yield value` as an <operator>.yield call carrying the value" in {
      val yields = cpg.call.nameExact("<operator>.yield").l
      yields.size shouldBe 2 // `yield secret` and bare `yield`

      val withArg = yields.filter(_.argument.nonEmpty)
      withArg.size shouldBe 1
      withArg.head.argument(1).code shouldBe "secret"
      withArg.head.code shouldBe "yield secret"
    }

    "model `yield from iterable` as an <operator>.yieldFrom call" in {
      val List(yf) = cpg.call.nameExact("<operator>.yieldFrom").l
      yf.code shouldBe "yield from range(x)"
      yf.argument(1).code shouldBe "range(x)"
    }

    "track data flow through a yield" in {
      val source = cpg.literal("42")
      val sink   = cpg.call.nameExact("<operator>.yield").argument
      sink.reachableByFlows(source).size shouldBe 1
    }
  }
}
