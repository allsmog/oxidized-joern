package io.joern.swiftsrc2cpg.passes.ast

import io.joern.swiftsrc2cpg.testfixtures.SwiftSrc2CpgSuite
import io.joern.x2cpg.frontendspecific.swiftsrc2cpg.Defines
import io.shiftleft.semanticcpg.language.*

class RegexLiteralTests extends SwiftSrc2CpgSuite {

  "RegexLiteralTests" should {

    "testRegexLiteral" in {
      val cpg = code("##/abc/#def/##")
      cpg.literal.code.l shouldBe List("##/abc/#def/##")
      cpg.literal.typeFullName.l shouldBe List(Defines.String)
      cpg.unknown.code.l shouldBe empty
    }

  }
}
