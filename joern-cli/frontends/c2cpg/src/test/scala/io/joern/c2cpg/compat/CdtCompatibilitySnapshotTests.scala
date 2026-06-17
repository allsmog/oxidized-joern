package io.joern.c2cpg.compat

import io.joern.c2cpg.{C2Cpg, Config}
import io.joern.c2cpg.parser.ParserBackend
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

class BackendParitySnapshotTests extends C2CpgSuite {

  "The C/C++ backend parity snapshot harness" should {

    "compare normalized core C and C++ slices across CDT and oxidized" in {
      val cases = Seq(
        BackendParitySnapshot.Case(
          "core methods, locals, calls, and returns",
          """
            |int add(int x, int y) {
            |  int total = x + y;
            |  return total;
            |}
            |
            |int main() {
            |  int result = add(1, 2);
            |  return result;
            |}
            |""".stripMargin
        ),
        BackendParitySnapshot.Case(
          "local reassignment and arithmetic",
          """
            |int bump(int x) {
            |  int y = 0;
            |  y = y + x;
            |  return y;
            |}
            |""".stripMargin
        ),
        BackendParitySnapshot.Case(
          "nested function calls and multiplication",
          """
            |int square(int x) {
            |  return x * x;
            |}
            |
            |int use_square(int x) {
            |  return square(x) + square(2);
            |}
            |""".stripMargin
        ),
        BackendParitySnapshot.Case(
          "C++ namespace function call",
          """
            |namespace Core {
            |int twice(int x) {
            |  return x + x;
            |}
            |}
            |
            |int main() {
            |  int value = Core::twice(21);
            |  return value;
            |}
            |""".stripMargin,
          filename = "Test0.cpp"
        ),
        BackendParitySnapshot.Case(
          "C++ struct local and field access",
          """
            |struct Box {
            |  int value;
            |};
            |
            |int read(Box box) {
            |  return box.value;
            |}
            |
            |int main() {
            |  Box box;
            |  box.value = 1;
            |  return read(box);
            |}
            |""".stripMargin,
          filename = "Test0.cpp",
          options = CompatibilitySnapshot.RenderOptions(typeNames = Seq("Box"), includeReturns = true)
        ),
        BackendParitySnapshot.Case(
          "C++ member method call",
          """
            |struct Counter {
            |  int value;
            |  int get() { return value; }
            |};
            |
            |int main() {
            |  Counter counter;
            |  counter.value = 1;
            |  return counter.get();
            |}
            |""".stripMargin,
          filename = "Test0.cpp",
          options =
            CompatibilitySnapshot.RenderOptions(typeNames = Seq("Counter"), includeReturns = true, includeCallDetails = true)
        ),
        BackendParitySnapshot.Case(
          "C++ virtual member dispatch",
          """
            |struct Base {
            |  virtual int get() { return 1; }
            |};
            |struct Derived : public Base {
            |  int get() override { return 2; }
            |};
            |
            |int main() {
            |  Derived derived;
            |  Base *base = &derived;
            |  return base->get();
            |}
            |""".stripMargin,
          filename = "Test0.cpp",
          options = CompatibilitySnapshot.RenderOptions(
            typeNames = Seq("Base", "Derived"),
            includeReturns = true,
            includeCallDetails = true
          )
        ),
        BackendParitySnapshot.Case(
          "C++ static member access",
          """
            |struct Counter {
            |  static int total;
            |  int value;
            |};
            |
            |int main() {
            |  Counter counter;
            |  Counter::total = 1;
            |  counter.value = Counter::total;
            |  return counter.value;
            |}
            |""".stripMargin,
          filename = "Test0.cpp",
          options =
            CompatibilitySnapshot.RenderOptions(typeNames = Seq("Counter"), includeReturns = true, includeCallDetails = true)
        ),
        BackendParitySnapshot.Case(
          "C++ overloaded operators",
          """
            |struct Box {
            |  int value;
            |  Box& operator=(const Box& other) { value = other.value; return *this; }
            |  int operator+(const Box& other) const { return value + other.value; }
            |  int operator[](int index) const { return value + index; }
            |  int operator()(int delta) const { return value + delta; }
            |};
            |
            |int main() {
            |  Box left;
            |  Box right;
            |  left = right;
            |  return left + right + left[1] + left(2);
            |}
            |""".stripMargin,
          filename = "Test0.cpp",
          options = CompatibilitySnapshot.RenderOptions(typeNames = Seq("Box"), includeReturns = true, includeCallDetails = true)
        ),
        BackendParitySnapshot.Case(
          "C++ template declarations and instantiated receivers",
          """
            |namespace Core {
            |template <typename T>
            |T pick(T value) { return value; }
            |template <typename T>
            |struct Holder {
            |  T value;
            |  T get() { return value; }
            |};
            |}
            |int use(Core::Holder<int> holder) {
            |  return holder.value + holder.get() + Core::pick<int>(1);
            |}
            |""".stripMargin,
          filename = "Test0.cpp",
          options = CompatibilitySnapshot.RenderOptions(
            typeNames = Seq("Holder"),
            includeReturns = true,
            includeCallDetails = true
          )
        )
      )

      cases.foreach(assertBackendParity)
    }
  }

  private def assertBackendParity(testCase: BackendParitySnapshot.Case): Unit = {
    val cdt      = code(testCase.source, testCase.filename).withConfig(Config(parserBackend = ParserBackend.Cdt))
    val oxidized = code(testCase.source, testCase.filename).withConfig(Config(parserBackend = ParserBackend.Oxidized))
    val cdtSnapshot      = CompatibilitySnapshot.render(cdt, testCase.options)
    val oxidizedSnapshot = CompatibilitySnapshot.render(oxidized, testCase.options)

    withClue(s"${testCase.name} parity snapshot differed\n${CompatibilitySnapshot.diff(oxidizedSnapshot, cdtSnapshot)}") {
      oxidizedSnapshot shouldBe cdtSnapshot
    }
  }

}

object CompatibilitySnapshot {

  final case class RenderOptions(
    typeNames: Seq[String] = Seq.empty,
    includeReturns: Boolean = false,
    includeCallDetails: Boolean = false
  )

  private val MacTempPath  = """/var/folders/.+?/T/c2cpgCompatibilitySnapshot\d+/""".r
  private val UnixTempPath = """/tmp/c2cpgCompatibilitySnapshot\d+/""".r

  def render(cpg: Cpg, typeNames: Seq[String] = Seq.empty, includeReturns: Boolean = false): String = {
    render(cpg, RenderOptions(typeNames, includeReturns))
  }

  def render(cpg: Cpg, options: RenderOptions): String = {
    val rawMethods = cpg.method.nameNot("<global>").l
    val genericMethodFullNames = rawMethods
      .map(method => comparableMethodFullName(method.fullName))
      .filter(isGenericTemplateMethodFullName)
      .toSet
    val methods = rawMethods
      .filterNot(method =>
        options.includeCallDetails && (
          isSyntheticOperatorMethod(method.name) ||
            isSyntheticTemplateInstantiation(
              method.lineNumber.isEmpty,
              comparableTemplateMethodFullName(method.fullName, genericMethodFullNames),
              genericMethodFullNames
            )
        )
      )
      .map { method =>
        val methodFullName = comparableTemplateMethodFullName(method.fullName, genericMethodFullNames)
        line(
          "METHOD",
          comparableMethodName(method.name),
          methodFullName,
          comparableSignature(method.signature),
          method.lineNumber.map(_.toString).getOrElse("?")
        )
      }

    val typeDecls =
      if (options.typeNames.isEmpty) Seq.empty
      else {
        cpg.typeDecl.nameExact(options.typeNames*).l.map { typeDecl =>
          line(
            "TYPE",
            typeDecl.name,
            comparableTypeDeclFullName(typeDecl.fullName, typeDecl.filename),
            typeDecl.filename,
            typeDecl.lineNumber.map(_.toString).getOrElse("?")
          )
        }
      }

    val locals = cpg.local.l
      .filterNot(local => isTypeOwnerLocal(cpg, local.name, local.typeFullName, local.code))
      .map { local =>
        line(
          "LOCAL",
          local.name,
          local.typeFullName,
          local.code,
          local.lineNumber.map(_.toString).getOrElse("?")
        )
      }

    val returns =
      if (!options.includeReturns) Seq.empty
      else {
        cpg.ret.l.map { ret =>
          line("RETURN", statementCode(ret.code), ret.lineNumber.map(_.toString).getOrElse("?"))
        }
      }

    val calls = cpg.call.l.map { call =>
      val values =
        if (options.includeCallDetails) {
          val name = comparableCallName(call.name)
          val methodFullName =
            comparableTemplateMethodFullName(
              comparableCallMethodFullName(call.name, call.methodFullName),
              genericMethodFullNames
            )
          Seq(
            name,
            methodFullName,
            call.dispatchType,
            comparableCallTypeFullName(name, methodFullName, call.typeFullName),
            call.code,
            call.lineNumber.map(_.toString).getOrElse("?")
          )
        } else
          Seq(
            call.name,
            call.methodFullName,
            call.code,
            call.lineNumber.map(_.toString).getOrElse("?")
          )
      line("CALL", values*)
    }

    val sections = Seq(
      section("METHODS", methods),
      section("TYPES", typeDecls),
      section("LOCALS", locals)
    ) ++ Option.when(options.includeReturns)(section("RETURNS", returns)) ++ Seq(
      section("CALLS", calls)
    )
    sections.mkString("\n")
  }

  def diff(actual: String, expected: String): String = {
    val actualLines   = actual.linesIterator.toSeq
    val expectedLines = expected.linesIterator.toSeq
    val actualOnly    = actualLines.diff(expectedLines).map(line => s"+ $line")
    val expectedOnly  = expectedLines.diff(actualLines).map(line => s"- $line")
    (actualOnly ++ expectedOnly).mkString("\n", "\n", "\n")
  }

  private def comparableCallTypeFullName(name: String, methodFullName: String, typeFullName: String): String = {
    if (name.startsWith("<operator>.")) "?" else methodReturnType(methodFullName).getOrElse(typeFullName)
  }

  private def comparableMethodName(name: String): String = {
    name match {
      case "operator()" => "()"
      case "operator+"  => "+"
      case "operator="  => "="
      case "operator[]" => "[]"
      case _            => name
    }
  }

  private def comparableMethodFullName(fullName: String): String = {
    val operatorNormalized =
      fullName
      .replace(".operator():", ".():")
      .replace(".operator+:", ".+:")
      .replace(".operator=:", ".=:")
      .replace(".operator[]:", ".[]:")
    normalizeFullNameSignature(operatorNormalized)
  }

  private def comparableCallName(name: String): String = {
    name match {
      case "operator()" => "<operator>()"
      case "operator+"  => "<operator>.addition"
      case "operator="  => "<operator>.assignment"
      case "operator[]" => "<operator>.indirectIndexAccess"
      case _            => eraseTemplateArguments(name)
    }
  }

  private def comparableCallMethodFullName(name: String, methodFullName: String): String = {
    name match {
      case "operator()" => methodFullName.replace(".operator():", ".<operator>():").replace("<const>", "")
      case "operator+"  => "<operator>.addition"
      case "operator="  => "<operator>.assignment"
      case "operator[]" => "<operator>.indirectIndexAccess"
      case _            => methodFullName
    }
  }

  private def comparableTemplateMethodFullName(fullName: String, genericMethodFullNames: Set[String]): String = {
    val comparable = comparableMethodFullName(fullName)
    val generic    = methodFullNameWithReturnType(comparable, "T")
    generic.filter(genericMethodFullNames.contains).getOrElse(comparable)
  }

  private def comparableSignature(signature: String): String = {
    normalizeTypeReferences(signature)
  }

  private def comparableTypeDeclFullName(fullName: String, filename: String): String = {
    val erased = eraseTemplateArguments(fullName)
    if (fullName.contains("<") || filename == "<includes>") simpleTypeName(erased) else erased
  }

  private def isGenericTemplateMethodFullName(fullName: String): Boolean = {
    methodReturnType(fullName).contains("T")
  }

  private def isSyntheticTemplateInstantiation(
    hasNoLineNumber: Boolean,
    methodFullName: String,
    genericMethodFullNames: Set[String]
  ): Boolean = {
    hasNoLineNumber && genericMethodFullNames.contains(methodFullName)
  }

  private def isSyntheticOperatorMethod(name: String): Boolean = {
    name == "<operator>()" || name == "<operator>.indirectIndexAccess"
  }

  private def normalizeFullNameSignature(fullName: String): String = {
    val signatureStart = fullName.indexOf(':')
    if (signatureStart < 0) fullName
    else s"${fullName.take(signatureStart + 1)}${normalizeTypeReferences(fullName.drop(signatureStart + 1))}"
  }

  private def normalizeTypeReferences(value: String): String = {
    eraseTemplateArguments(value)
      .replace("::", ".")
      .replaceAll("""\b(?:[A-Za-z_]\w*\.)+([A-Za-z_]\w*)""", "$1")
  }

  private def eraseTemplateArguments(value: String): String = {
    if (value.contains("<operator>")) {
      value
    } else {
      val builder = new StringBuilder
      var depth   = 0
      value.foreach {
        case '<' =>
          depth += 1
        case '>' if depth > 0 =>
          depth -= 1
        case ch if depth == 0 =>
          builder.append(ch)
        case _ =>
      }
      if (depth == 0) builder.toString else value
    }
  }

  private def simpleTypeName(typeFullName: String): String = {
    typeFullName.replace("::", ".").split('.').lastOption.getOrElse(typeFullName)
  }

  private def methodReturnType(methodFullName: String): Option[String] = {
    val signatureStart = methodFullName.indexOf(':')
    val paramsStart    = methodFullName.indexOf('(', signatureStart + 1)
    Option.when(signatureStart >= 0 && paramsStart > signatureStart) {
      methodFullName.substring(signatureStart + 1, paramsStart)
    }
  }

  private def methodFullNameWithReturnType(methodFullName: String, returnType: String): Option[String] = {
    val signatureStart = methodFullName.indexOf(':')
    val paramsStart    = methodFullName.indexOf('(', signatureStart + 1)
    Option.when(signatureStart >= 0 && paramsStart > signatureStart) {
      s"${methodFullName.take(signatureStart + 1)}$returnType${methodFullName.drop(paramsStart)}"
    }
  }

  private def isTypeOwnerLocal(cpg: Cpg, name: String, typeFullName: String, code: String): Boolean = {
    name == typeFullName && code == name && cpg.typeDecl.fullNameExact(typeFullName).nonEmpty
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

  private def statementCode(value: String): String = normalize(value).stripSuffix(";").trim

}

object BackendParitySnapshot {

  final case class Case(
    name: String,
    source: String,
    filename: String = "Test0.c",
    options: CompatibilitySnapshot.RenderOptions = CompatibilitySnapshot.RenderOptions(includeReturns = true)
  )

}
