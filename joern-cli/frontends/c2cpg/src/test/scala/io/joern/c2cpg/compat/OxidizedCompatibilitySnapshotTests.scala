package io.joern.c2cpg.compat

import io.joern.c2cpg.{C2Cpg, Config}
import io.joern.c2cpg.parser.ParserBackend
import io.joern.c2cpg.testfixtures.C2CpgSuite
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, Operators}
import io.shiftleft.semanticcpg.language.*
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.FileUtil.*

import java.nio.file.{Files, Paths}

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

    "capture counted loops, jumps, and indexed expressions from the Rust parser backend" in {
      val cpg = code("""
          |int sum(int *xs, int n) {
          |  int total = 0;
          |  for (int i = 0; i < n; i++) {
          |    if (!xs[i]) {
          |      continue;
          |    }
          |    total = total + xs[i];
          |  }
          |  return total;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("sum").controlStructure.controlStructureType.l shouldBe
        List(ControlStructureTypes.FOR, ControlStructureTypes.IF, ControlStructureTypes.CONTINUE)
      cpg.method.nameExact("sum").forBlock.condition.code.l shouldBe List("i < n")
      cpg.call.nameExact(Operators.postIncrement).code.l shouldBe List("i++")
      cpg.call.nameExact(Operators.logicalNot).code.l shouldBe List("!xs[i]")
      cpg.call.nameExact(Operators.indirectIndexAccess).code.l.sorted shouldBe List("xs[i]", "xs[i]")
      cpg.call.nameExact(Operators.assignment).code.l.sorted shouldBe
        List("i = 0", "total = 0", "total = total + xs[i]")
    }

    "capture switch, do-while, labels, and gotos from the Rust parser backend" in {
      val cpg = code("""
          |int route(int x) {
          |retry:
          |  do {
          |    x = x - 1;
          |  } while (x > 3);
          |  switch (x) {
          |    case 1:
          |      goto retry;
          |    default:
          |      break;
          |  }
          |  return x;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("route").controlStructure.controlStructureType.l.sorted shouldBe
        List(ControlStructureTypes.BREAK, ControlStructureTypes.DO, ControlStructureTypes.GOTO, ControlStructureTypes.SWITCH)
      cpg.method.nameExact("route").doBlock.condition.code.l shouldBe List("x > 3")
      cpg.jumpTarget.name.l.sorted shouldBe List("case", "default", "retry")
      cpg.jumpTarget.code.l.sorted shouldBe List("case 1:", "default:", "retry:")
      cpg.call.nameExact(Operators.subtraction).code.l shouldBe List("x - 1")
    }

    "capture casts, sizeof, ternaries, and compound assignment from the Rust parser backend" in {
      val cpg = code("""
          |int score(int x) {
          |  int y = (int)sizeof(x);
          |  y += x > 0 ? x : -x;
          |  return y;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.call.nameExact(Operators.assignment).code.l shouldBe List("y = (int)sizeof(x)")
      cpg.call.nameExact(Operators.cast).code.l shouldBe List("(int)sizeof(x)")
      cpg.call.nameExact(Operators.sizeOf).code.l shouldBe List("sizeof(x)")
      cpg.call.nameExact(Operators.assignmentPlus).code.l shouldBe List("y += x > 0 ? x : -x")
      cpg.call.nameExact(Operators.conditional).code.l shouldBe List("x > 0 ? x : -x")
      cpg.call.nameExact(Operators.greaterThan).code.l shouldBe List("x > 0")
      cpg.call.nameExact(Operators.minus).code.l shouldBe List("-x")
    }

    "preserve block scope when locals shadow outer declarations" in {
      val cpg = code("""
          |int shadow(int x) {
          |  int y = x;
          |  if (x) {
          |    int y = 1;
          |    y = y + 1;
          |  }
          |  return y;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val outerY = cpg.local.nameExact("y").lineNumber(3).head
      val innerY = cpg.local.nameExact("y").lineNumber(5).head
      cpg.identifier.nameExact("y").lineNumber(6).refsTo.dedup.l shouldBe List(innerY)
      cpg.identifier.nameExact("y").lineNumber(8).refsTo.l shouldBe List(outerY)
    }

    "preserve pointer and array type names from declarators" in {
      val cpg = code("""
          |struct Holder {
          |  int *next;
          |  int values[4];
          |};
          |int first(int *xs) {
          |  int local[4];
          |  int *p = xs;
          |  return p[0];
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.typeDecl.nameExact("Holder").member.nameExact("next").typeFullName.l shouldBe List("int*")
      cpg.typeDecl.nameExact("Holder").member.nameExact("values").typeFullName.l shouldBe List("int[]")
      cpg.method.nameExact("first").parameter.nameExact("xs").typeFullName.l shouldBe List("int*")
      cpg.method.nameExact("first").local.nameExact("local").typeFullName.l shouldBe List("int[]")
      cpg.method.nameExact("first").local.nameExact("p").typeFullName.l shouldBe List("int*")
    }

    "capture global variables and local shadow references" in {
      val cpg = code("""
          |int global = 1;
          |static int *ptr;
          |int read() {
          |  return global;
          |}
          |int shadow() {
          |  int global = 2;
          |  return global;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val globalLocal = cpg.local.nameExact("global").filter(_.code == "int global").head
      val readGlobal  = cpg.method.nameExact("read").local.nameExact("global").head
      val shadowLocal = cpg.method.nameExact("shadow").local.nameExact("global").head
      globalLocal.typeFullName shouldBe "int"
      readGlobal.code shouldBe "<global> global"
      readGlobal.typeFullName shouldBe "int"
      readGlobal.closureBindingId shouldBe Some("Test0.c:read:global")
      cpg.local.nameExact("ptr").typeFullName.l shouldBe List("int*")
      cpg.call.nameExact(Operators.assignment).code.l should contain("global = 1")
      cpg.method.nameExact("read").block.ast.isIdentifier.nameExact("global").refsTo.l shouldBe List(readGlobal)
      cpg.method.nameExact("shadow").block.ast.isIdentifier.nameExact("global").refsTo.dedup.l shouldBe List(shadowLocal)
      val List(binding) = globalLocal.closureBinding.l
      binding.closureBindingId shouldBe readGlobal.closureBindingId
      binding._localViaRefOut.get shouldBe globalLocal
      binding._captureIn.l shouldBe cpg.methodRef.methodFullNameExact("read").l
    }

    "capture typedef aliases" in {
      val cpg = code("""
          |typedef const char * foo;
          |typedef foo * bar;
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.typeDecl.nameExact("foo").aliasTypeFullName.l shouldBe List("char*")
      cpg.typeDecl.nameExact("bar").aliasTypeFullName.l shouldBe List("char**")
    }

    "capture typedef aggregate aliases" in {
      val cpg = code("""
          |typedef struct foo {
          |  int x;
          |} abc;
          |typedef struct {
          |  int y;
          |} Foo;
          |typedef enum mode {
          |  MODE_A,
          |} Mode;
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.typeDecl.nameExact("foo").aliasTypeFullName.l shouldBe List("abc")
      cpg.typeDecl.nameExact("abc").aliasTypeFullName.l shouldBe List("foo")
      cpg.typeDecl.nameExact("Foo").aliasTypeFullName.l shouldBe Nil
      cpg.typeDecl.nameExact("Foo").member.name.l shouldBe List("y")
      cpg.typeDecl.nameExact("mode").aliasTypeFullName.l shouldBe List("Mode")
      cpg.typeDecl.nameExact("Mode").aliasTypeFullName.l shouldBe List("mode")
    }

    "capture initializer lists and designated initializers" in {
      val cpg = code("""
          |struct Fs { int open; };
          |int global[] = {0, 1};
          |int opener(int fd) { return fd; }
          |static const struct Ops ops = { .open = opener };
          |int init() {
          |  int local[2] = {2, 3};
          |  struct Fs fs = { .open = 7 };
          |  int ranged[10] = { [3 ... 9] = 15 };
          |  return local[1] + fs.open + ranged[3] + global[0];
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.local.nameExact("global").filter(_.code == "int global[]").typeFullName.l shouldBe List("int[]")
      cpg.local.nameExact("local").typeFullName.l shouldBe List("int[]")
      cpg.local.nameExact("ranged").typeFullName.l shouldBe List("int[]")
      val initializerCodes = cpg.call.nameExact(Operators.arrayInitializer).code.l
      initializerCodes.contains("{0, 1}") shouldBe true
      initializerCodes.contains("{2, 3}") shouldBe true
      initializerCodes.contains("{ .open = 7 }") shouldBe true
      initializerCodes.contains("{ .open = opener }") shouldBe true
      initializerCodes.contains("{ [3 ... 9] = 15 }") shouldBe true
      initializerCodes.contains("[3 ... 9]") shouldBe true

      val fieldInit = cpg.call.nameExact(Operators.assignment).filter(_.code == ".open = 7").head
      fieldInit.argument.code.l shouldBe List("open", "7")
      fieldInit.argument(1).start.isIdentifier.refsTo.l shouldBe Nil

      val methodInit = cpg.call.nameExact(Operators.assignment).filter(_.code == ".open = opener").head
      methodInit.argument(2).start.isMethodRef.methodFullName.l shouldBe List("opener")
      methodInit.argument(2).start.isMethodRef.typeFullName.l shouldBe List("int")

      val rangeInit = cpg.call.nameExact(Operators.assignment).filter(_.code == "[3 ... 9] = 15").head
      rangeInit.argument(1).start.isCall.code.l shouldBe List("[3 ... 9]")
      rangeInit.argument(1).start.isCall.argument.code.l shouldBe List("3", "9")
      rangeInit.argument(2).code shouldBe "15"
    }

    "capture function prototypes as external methods" in {
      val cpg = code("""
          |int external(int value);
          |int external(int value);
          |int unnamed(int, char *);
          |int defined(int value);
          |int defined(int value) {
          |  return external(value);
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("external").external.signature.l shouldBe List("int(int)")
      cpg.method.nameExact("defined").internal.signature.l shouldBe List("int(int)")
      cpg.method.nameExact("unnamed").external.parameter.name.l shouldBe List("param1", "param2")
      cpg.method.nameExact("unnamed").external.parameter.typeFullName.l shouldBe List("int", "char*")
      cpg.call.nameExact("external").methodFullName.l shouldBe List("external")
      cpg.call.nameExact("external").signature.l shouldBe List("int(int)")
    }

    "honor compile database source selection through the Rust backend" in {
      FileUtil.usingTemporaryDirectory("oxidizedCompatibilitySnapshot") { dir =>
        val selected = dir / "selected.c"
        val ignored  = dir / "ignored.c"
        Files.writeString(selected, "int selected() { return FROM_DB; }\n")
        Files.writeString(ignored, "int ignored() { return 0; }\n")

        val compileCommands = dir / "compile_commands.json"
        Files.writeString(
          compileCommands,
          s"""
             |[
             |  {
             |    "directory": "${dir.toString}",
             |    "arguments": ["clang", "-DFROM_DB=7", "-c", "selected.c"],
             |    "file": "${selected.toString}"
             |  }
             |]
             |""".stripMargin.replace("\\", "\\\\")
        )

        val cpg = new C2Cpg()
          .createCpg(
            Config(parserBackend = ParserBackend.Oxidized)
              .withInputPath(dir.toString)
              .withCompilationDatabase((Paths.get(dir.toString) / "compile_commands.json").toString)
          )
          .get

        try {
          cpg.method.nameExact("selected").name.l shouldBe List("selected")
          cpg.method.nameExact("ignored").name.l shouldBe Nil
        } finally {
          cpg.close()
        }
      }
    }

  }

}
