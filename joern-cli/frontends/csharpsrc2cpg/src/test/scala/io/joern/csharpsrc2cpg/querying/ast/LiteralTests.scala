package io.joern.csharpsrc2cpg.querying.ast

import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.shiftleft.semanticcpg.language.*

class LiteralTests extends CSharpCode2CpgFixture {
  "inner text in string literals" in {
    val cpg = code(basicBoilerplate("""
        |var a = "abc";
        |var b = "\"abc";
        |var c = "abc\"";
        |var d = "\"abc\"";
        |var e = "a\"b\"c";
        |""".stripMargin))

    cpg.literal.strippedCode.l shouldBe List("abc", "\\\"abc", "abc\\\"", "\\\"abc\\\"", "a\\\"b\\\"c")
  }

  "inner text in raw string literals" in {
    val rawLiteral = "\"\"\"abc\"\"\""
    val cpg        = code(basicBoilerplate(s"var raw = $rawLiteral;"))

    cpg.literal.code.l shouldBe List(rawLiteral)
    cpg.literal.strippedCode.l shouldBe List("abc")
  }
}
