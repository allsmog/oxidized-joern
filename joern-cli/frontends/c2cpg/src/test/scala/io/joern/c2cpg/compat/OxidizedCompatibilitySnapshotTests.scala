package io.joern.c2cpg.compat

import io.joern.c2cpg.Config
import io.joern.c2cpg.parser.ParserBackend
import io.joern.c2cpg.testfixtures.C2CpgSuite
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, Operators}
import io.shiftleft.semanticcpg.language.*

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

    "capture control flow from the Rust parser backend" in {
      val cpg = code("""
          |int clamp(int x) {
          |  if (x < 0) {
          |    return 0;
          |  } else {
          |    x = 1;
          |  }
          |  while (x > 10) {
          |    x = x - 1;
          |  }
          |  return x;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("clamp").controlStructure.controlStructureType.l shouldBe
        List(ControlStructureTypes.IF, ControlStructureTypes.ELSE, ControlStructureTypes.WHILE)
      cpg.method.nameExact("clamp").ifBlock.condition.code.l shouldBe List("x < 0")
      cpg.method.nameExact("clamp").whileBlock.condition.code.l shouldBe List("x > 10")
      cpg.call.nameExact(Operators.subtraction).code.l shouldBe List("x - 1")
      cpg.call.nameExact(Operators.assignment).code.l.sorted shouldBe List("x = 1", "x = x - 1")
    }

  }

}
