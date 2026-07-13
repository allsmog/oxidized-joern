package io.joern.swiftsrc2cpg.passes.ast

import io.joern.swiftsrc2cpg.testfixtures.SwiftSrc2CpgSuite
import io.shiftleft.semanticcpg.language.*

class IntegerLiteralTests extends SwiftSrc2CpgSuite {

  "IntegerLiteralTests" should {

    "create a literal node for a top-level integer binding" in {
      val cpg = code("let lucky = 667")
      cpg.literal.code.l should contain("667")
    }

  }
}
