package io.joern.c2cpg.compat

import io.joern.c2cpg.{C2Cpg, Config}
import io.joern.c2cpg.testfixtures.C2CpgSuite
import io.shiftleft.codepropertygraph.generated.Cpg
import io.shiftleft.semanticcpg.language.*
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.FileUtil.*

import java.nio.file.{Files, Paths}

class CdtCompatibilitySnapshotTests extends C2CpgSuite {

  "The CDT compatibility snapshot harness" should {

    "capture core C methods, calls, locals, and types" in {
      val cpg = code("""
          |#define INC(x) ((x) + 1)
          |
          |enum Mode { MODE_A = 1, MODE_B = 2 };
          |struct Box { int value; };
          |
          |int add(int x, int y) {
          |  int total = x + y;
          |  return total;
          |}
          |
          |int main() {
          |  struct Box box;
          |  box.value = INC(add(1, 2));
          |  return box.value;
          |}
          |""".stripMargin)

      CompatibilitySnapshot.render(cpg, typeNames = Seq("Box", "Mode")) shouldBe
        """[METHODS]
          |METHOD|<clinit>|Mode.<clinit>:Mode()||4
          |METHOD|<operator>.addition|<operator>.addition||?
          |METHOD|<operator>.assignment|<operator>.assignment||?
          |METHOD|<operator>.fieldAccess|<operator>.fieldAccess||?
          |METHOD|INC|Test0.c:INC:ANY(1)|ANY(1)|2
          |METHOD|add|add|int(int,int)|7
          |METHOD|main|main|int()|12
          |[TYPES]
          |TYPE|Box|Box|Test0.c|5
          |TYPE|Mode|Mode|Test0.c|4
          |[LOCALS]
          |LOCAL|MODE_A|ANY|MODE_A|4
          |LOCAL|MODE_B|ANY|MODE_B|4
          |LOCAL|box|Box|struct Box box|13
          |LOCAL|total|int|int total|8
          |[CALLS]
          |CALL|<operator>.addition|<operator>.addition|(add(1, 2)) + 1|14
          |CALL|<operator>.addition|<operator>.addition|x + y|8
          |CALL|<operator>.assignment|<operator>.assignment|MODE_A = 1|4
          |CALL|<operator>.assignment|<operator>.assignment|MODE_B = 2|4
          |CALL|<operator>.assignment|<operator>.assignment|box.value = INC(add(1, 2))|14
          |CALL|<operator>.assignment|<operator>.assignment|total = x + y|8
          |CALL|<operator>.fieldAccess|<operator>.fieldAccess|box.value|14
          |CALL|<operator>.fieldAccess|<operator>.fieldAccess|box.value|15
          |CALL|INC|Test0.c:INC:ANY(1)|INC(add(1, 2))|14
          |CALL|add|add|add(1, 2)|14""".stripMargin
    }

    "capture header-driven declarations and types" in {
      val cpg = code(
        """
          |#ifndef SNAPSHOT_MATH_H
          |#define SNAPSHOT_MATH_H
          |
          |struct HeaderBox { int value; };
          |int header_add(int x, int y);
          |
          |#endif
          |""".stripMargin,
        "include/snapshot_math.h"
      ).moreCode(
        """
          |#include "include/snapshot_math.h"
          |
          |int header_add(int x, int y) {
          |  return x + y;
          |}
          |
          |int use_header(int input) {
          |  struct HeaderBox box;
          |  box.value = header_add(input, 3);
          |  return box.value;
          |}
          |""".stripMargin,
        "main.c"
      )

      CompatibilitySnapshot.render(cpg, typeNames = Seq("HeaderBox")) shouldBe
        """[METHODS]
          |METHOD|<operator>.addition|<operator>.addition||?
          |METHOD|<operator>.assignment|<operator>.assignment||?
          |METHOD|<operator>.fieldAccess|<operator>.fieldAccess||?
          |METHOD|header_add|header_add|int(int,int)|4
          |METHOD|use_header|use_header|int(int)|8
          |[TYPES]
          |TYPE|HeaderBox|HeaderBox|include/snapshot_math.h|5
          |[LOCALS]
          |LOCAL|box|HeaderBox|struct HeaderBox box|9
          |[CALLS]
          |CALL|<operator>.addition|<operator>.addition|x + y|5
          |CALL|<operator>.assignment|<operator>.assignment|box.value = header_add(input, 3)|10
          |CALL|<operator>.fieldAccess|<operator>.fieldAccess|box.value|10
          |CALL|<operator>.fieldAccess|<operator>.fieldAccess|box.value|11
          |CALL|header_add|header_add|header_add(input, 3)|10""".stripMargin
    }

    "capture compile database defines and include paths" in {
      FileUtil.usingTemporaryDirectory("c2cpgCompatibilitySnapshot") { dir =>
        val includeDir = dir / "include"
        Files.createDirectories(includeDir)
        Files.writeString(
          includeDir / "feature.h",
          """
            |#define FEATURE_VALUE 7
            |""".stripMargin
        )

        val source = dir / "main.c"
        Files.writeString(
          source,
          """
            |#include "feature.h"
            |
            |int selected() {
            |#ifdef FEATURE
            |  return FEATURE_VALUE;
            |#else
            |  return 0;
            |#endif
            |}
            |""".stripMargin
        )

        val compileCommands = dir / "compile_commands.json"
        Files.writeString(
          compileCommands,
          s"""
             |[
             |  {
             |    "directory": "${dir.toString}",
             |    "arguments": ["clang", "-I${includeDir.toString}", "-DFEATURE", "-c", "main.c"],
             |    "file": "${source.toString}"
             |  }
             |]
             |""".stripMargin.replace("\\", "\\\\")
        )

        val cpg = new C2Cpg()
          .createCpg(
            Config()
              .withInputPath(dir.toString)
              .withCompilationDatabase((Paths.get(dir.toString) / "compile_commands.json").toString)
          )
          .get

        try {
          CompatibilitySnapshot.render(cpg) shouldBe
            """[METHODS]
              |METHOD|FEATURE_VALUE|<tmp>/feature.h:FEATURE_VALUE:int(0)|int(0)|2
              |METHOD|selected|selected|int()|4
              |[TYPES]
              |<empty>
              |[LOCALS]
              |<empty>
              |[CALLS]
              |CALL|FEATURE_VALUE|<tmp>/feature.h:FEATURE_VALUE:int(0)|FEATURE_VALUE|6""".stripMargin
        } finally {
          cpg.close()
        }
      }
    }

  }

}

private object CompatibilitySnapshot {

  private val MacTempPath  = """/var/folders/.+?/T/c2cpgCompatibilitySnapshot\d+/""".r
  private val UnixTempPath = """/tmp/c2cpgCompatibilitySnapshot\d+/""".r

  def render(cpg: Cpg, typeNames: Seq[String] = Seq.empty): String = {
    val methods = cpg.method.nameNot("<global>").l.map { method =>
      line(
        "METHOD",
        method.name,
        method.fullName,
        method.signature,
        method.lineNumber.map(_.toString).getOrElse("?")
      )
    }

    val typeDecls =
      if (typeNames.isEmpty) Seq.empty
      else {
        cpg.typeDecl.nameExact(typeNames*).l.map { typeDecl =>
          line(
            "TYPE",
            typeDecl.name,
            typeDecl.fullName,
            typeDecl.filename,
            typeDecl.lineNumber.map(_.toString).getOrElse("?")
          )
        }
      }

    val locals = cpg.local.l.map { local =>
      line(
        "LOCAL",
        local.name,
        local.typeFullName,
        local.code,
        local.lineNumber.map(_.toString).getOrElse("?")
      )
    }

    val calls = cpg.call.l.map { call =>
      line(
        "CALL",
        call.name,
        call.methodFullName,
        call.code,
        call.lineNumber.map(_.toString).getOrElse("?")
      )
    }

    Seq(
      section("METHODS", methods),
      section("TYPES", typeDecls),
      section("LOCALS", locals),
      section("CALLS", calls)
    ).mkString("\n")
  }

  private def section(name: String, lines: Seq[String]): String = {
    val body = if (lines.isEmpty) Seq("<empty>") else lines.sorted
    (s"[$name]" +: body).mkString("\n")
  }

  private def line(kind: String, values: String*): String = {
    (kind +: values.map(normalize)).mkString("|")
  }

  private def normalize(value: String): String = {
    val unixSeparators = value.replace('\\', '/')
    val tempNormalized = UnixTempPath.replaceAllIn(MacTempPath.replaceAllIn(unixSeparators, "<tmp>/"), "<tmp>/")
    tempNormalized.replaceAll("\\s+", " ").trim
  }

}
