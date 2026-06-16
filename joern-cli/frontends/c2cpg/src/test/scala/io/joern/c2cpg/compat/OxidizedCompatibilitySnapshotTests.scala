package io.joern.c2cpg.compat

import io.joern.c2cpg.Config
import io.joern.c2cpg.parser.ParserBackend
import io.joern.c2cpg.testfixtures.C2CpgSuite

class OxidizedCompatibilitySnapshotTests extends C2CpgSuite {

  "The oxidized compatibility snapshot harness" should {

    "capture a tiny C slice through the Rust parser backend" in {
      val cpg = code("""
          |#define INC(x) ((x) + 1)
          |
          |struct Box { int value; };
          |
          |int add(int x, int y) {
          |  int total = x + y;
          |  return total;
          |}
          |
          |int main() {
          |  Box box;
          |  box.value = INC(add(1, 2));
          |  return box.value;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      CompatibilitySnapshot.render(cpg, typeNames = Seq("Box")) shouldBe
        """[METHODS]
          |METHOD|<operator>.addition|<operator>.addition||?
          |METHOD|<operator>.assignment|<operator>.assignment||?
          |METHOD|<operator>.fieldAccess|<operator>.fieldAccess||?
          |METHOD|INC|Test0.c:INC:ANY(1)|ANY(1)|2
          |METHOD|add|add|int(int,int)|6
          |METHOD|main|main|int()|11
          |[TYPES]
          |TYPE|Box|Box|Test0.c|4
          |[LOCALS]
          |LOCAL|box|Box|Box box|12
          |LOCAL|total|int|int total|7
          |[CALLS]
          |CALL|<operator>.addition|<operator>.addition|x + y|7
          |CALL|<operator>.assignment|<operator>.assignment|box.value = INC(add(1, 2))|13
          |CALL|<operator>.assignment|<operator>.assignment|total = x + y|7
          |CALL|<operator>.fieldAccess|<operator>.fieldAccess|box.value|13
          |CALL|<operator>.fieldAccess|<operator>.fieldAccess|box.value|14
          |CALL|INC|Test0.c:INC:ANY(1)|INC(add(1, 2))|13
          |CALL|add|add|add(1, 2)|13""".stripMargin
    }

  }

}
