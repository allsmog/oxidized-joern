package io.joern.pysrc2cpg.cpg

import io.joern.pysrc2cpg.testfixtures.PySrc2CpgFixture
import io.shiftleft.semanticcpg.language.*
import org.scalatest.matchers.should.Matchers

class YieldCpgTests extends PySrc2CpgFixture with Matchers {
  // In the oxidized model `yield` is lowered to a call to `<operator>.yield`
  // (and `yield from` to `<operator>.yieldFrom`) carrying the yielded value as
  // its argument, rather than to a RETURN node. See passes/YieldTests.scala.
  "bare yield" should {
    val cpg = code("""def gen():
        |  yield
        |""".stripMargin)

    "test yield call node properties" in {
      val yieldCall = cpg.call.nameExact("<operator>.yield").head
      yieldCall.code shouldBe "yield"
      yieldCall.lineNumber shouldBe Some(2)
    }

    "have no arguments" in {
      cpg.call.nameExact("<operator>.yield").argument.l shouldBe empty
    }
  }

  "yield with value" should {
    val cpg = code("""def gen():
        |  yield x
        |""".stripMargin)

    "test yield call node properties" in {
      val yieldCall = cpg.call.nameExact("<operator>.yield").head
      yieldCall.code shouldBe "yield x"
      yieldCall.lineNumber shouldBe Some(2)
    }

    "test yield call argument" in {
      cpg.call.nameExact("<operator>.yield").argument(1).isIdentifier.head.code shouldBe "x"
    }
  }

  "yield in a loop" should {
    val cpg = code("""def gen(items):
        |  for x in items:
        |    yield x
        |""".stripMargin)

    "have a yield call with yield code" in {
      val yieldCall = cpg.call.nameExact("<operator>.yield").head
      yieldCall.code shouldBe "yield x"
    }
  }

  "yield from" should {
    val cpg = code("""def gen():
        |  yield from other_gen()
        |""".stripMargin)

    "test yieldFrom call node properties" in {
      val yieldFromCall = cpg.call.nameExact("<operator>.yieldFrom").head
      yieldFromCall.code shouldBe "yield from other_gen()"
      yieldFromCall.lineNumber shouldBe Some(2)
    }

    "test yieldFrom call argument" in {
      cpg.call.nameExact("<operator>.yieldFrom").argument(1).isCall.head.code shouldBe "other_gen()"
    }
  }

  "yield from with identifier" should {
    val cpg = code("""def gen(items):
        |  yield from items
        |""".stripMargin)

    "test yieldFrom call node properties" in {
      val yieldFromCall = cpg.call.nameExact("<operator>.yieldFrom").head
      yieldFromCall.code shouldBe "yield from items"
      yieldFromCall.lineNumber shouldBe Some(2)
    }

    "test yieldFrom call argument" in {
      cpg.call.nameExact("<operator>.yieldFrom").argument(1).isIdentifier.head.code shouldBe "items"
    }
  }
}
