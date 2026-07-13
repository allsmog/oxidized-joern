package io.joern.csharpsrc2cpg.passes.ast

import io.joern.csharpsrc2cpg.astcreation.BuiltinTypes
import io.joern.csharpsrc2cpg.parser.DotNetJsonAst.LiteralExpr
import io.joern.csharpsrc2cpg.testfixtures.CSharpCode2CpgFixture
import io.shiftleft.codepropertygraph.generated.Operators
import io.shiftleft.codepropertygraph.generated.nodes.{Identifier, Literal}
import io.shiftleft.semanticcpg.language.*

class OperatorsTests extends CSharpCode2CpgFixture {
  "AST nodes for operators" should {
    "be created for unary operators" in {
      val cpg = code(
        basicBoilerplate("""
          |int i = 3;
          |i++;
          |i--;
          |++i;
          |--i;
          |!i;
          |~i;
          |+5;
          |-5;
          |&i;
          |""".stripMargin),
        "Program.cs"
      )

      val operatorCalls = cpg.method("Main").ast.isCall.nameNot(Operators.assignment).l
      operatorCalls.size shouldBe 9
      operatorCalls.name.l shouldBe List(
        "<operator>.postIncrement",
        "<operator>.postDecrement",
        "<operator>.preIncrement",
        "<operator>.preDecrement",
        "<operator>.logicalNot",
        "<operator>.not",
        "<operator>.plus",
        "<operator>.minus",
        "<operator>.addressOf"
      )
      operatorCalls.code.l shouldBe List("i++", "i--", "++i", "--i", "!i", "~i", "+5", "-5", "&i")
      inside(operatorCalls.nameExact(Operators.postDecrement).astChildren.l) { case List(ident: Identifier) =>
        ident.code shouldBe "i"
      }
    }
  }

  "be created for binary operators" in {
    val cpg = code(
      basicBoilerplate("""
        |int a = 3;
        |int b = 5;
        |uint u = 8;
        |a+b;
        |a-b;
        |a/b;
        |a%b;
        |a==b;
        |a!=b;
        |a&&b;
        |a||b;
        |a&b;
        |a|b;
        |a^b;
        |a<<b;
        |a>>b;
        |u>>>b;
        |""".stripMargin),
      fileName = "Program.cs"
    )
    val operatorCalls = cpg.method("Main").ast.isCall.nameNot(Operators.assignment).l
    operatorCalls.size shouldBe 14
    operatorCalls.name.l shouldBe List(
      "<operator>.addition",
      "<operator>.subtraction",
      "<operator>.division",
      "<operator>.modulo",
      "<operator>.equals",
      "<operator>.notEquals",
      "<operator>.logicalAnd",
      "<operator>.logicalOr",
      "<operator>.and",
      "<operator>.or",
      "<operator>.xor",
      Operators.shiftLeft,
      Operators.logicalShiftRight,
      Operators.arithmeticShiftRight
    )
    operatorCalls.code.l shouldBe List(
      "a+b",
      "a-b",
      "a/b",
      "a%b",
      "a==b",
      "a!=b",
      "a&&b",
      "a||b",
      "a&b",
      "a|b",
      "a^b",
      "a<<b",
      "a>>b",
      "u>>>b"
    )

    inside(operatorCalls.nameExact(Operators.addition).astChildren.l) { case List(lhs: Identifier, rhs: Identifier) =>
      lhs.code shouldBe "a"
      rhs.code shouldBe "b"
    }
  }

  "be created for shorthand assignment operators" in {
    val cpg = code(
      basicBoilerplate("""
        |int a = 3;
        |int b = 5;
        |uint u = 8;
        |a+=b;
        |a-=b;
        |a*=b;
        |a/=b;
        |a%=b;
          |a&=b;
          |a|=b;
        |a^=b;
        |a>>=b;
        |a<<=b;
        |u>>>=b;
        |""".stripMargin),
      fileName = "Program.cs"
    )
    val operatorCalls = cpg.method("Main").ast.isCall.nameNot(Operators.assignment).l
    operatorCalls.size shouldBe 11
    operatorCalls.name.l shouldBe List(
      "<operator>.assignmentPlus",
      "<operator>.assignmentMinus",
      "<operator>.assignmentMultiplication",
      "<operator>.assignmentDivision",
      "<operators>.assignmentModulo",
      "<operators>.assignmentAnd",
      "<operators>.assignmentOr",
      "<operators>.assignmentXor",
      "<operators>.assignmentLogicalShiftRight",
      "<operators>.assignmentShiftLeft",
      Operators.assignmentArithmeticShiftRight
    )
    operatorCalls.code.l shouldBe List(
      "a+=b",
      "a-=b",
      "a*=b",
      "a/=b",
      "a%=b",
      "a&=b",
      "a|=b",
      "a^=b",
      "a>>=b",
      "a<<=b",
      "u>>>=b"
    )

    inside(operatorCalls.nameExact(Operators.assignmentPlus).astChildren.l) {
      case List(lhs: Identifier, rhs: Identifier) =>
        lhs.code shouldBe "a"
        rhs.code shouldBe "b"
    }
  }

  "be created for comparison operators" in {
    val cpg = code(
      basicBoilerplate("""
          |int a = 3;
          |int b = 5;
          |a > b;
          |a < b;
          |a == b;
          |a >= b;
          |a <= b;
          |""".stripMargin),
      fileName = "Program.cs"
    )
    val operatorCalls = cpg.method("Main").ast.isCall.nameNot(Operators.assignment).l
    operatorCalls.size shouldBe 5
    operatorCalls.name.l shouldBe List(
      "<operator>.greaterThan",
      "<operator>.lessThan",
      "<operator>.equals",
      "<operator>.greaterEqualsThan",
      "<operator>.lessEqualsThan"
    )
    operatorCalls.code.l shouldBe List("a > b", "a < b", "a == b", "a >= b", "a <= b")

    inside(operatorCalls.nameExact(Operators.greaterThan).astChildren.l) {
      case List(lhs: Identifier, rhs: Identifier) =>
        lhs.code shouldBe "a"
        rhs.code shouldBe "b"
    }
  }

  "be created for pointer indirection operators" in {
    val cpg = code(basicBoilerplate("""
        |unsafe
        |{
        |  int* p = stackalloc int[1];
        |  var value = *p;
        |}
        |""".stripMargin))

    inside(cpg.call.nameExact(Operators.indirection).l) { case indirection :: Nil =>
      indirection.code shouldBe "*p"
      indirection.methodFullName shouldBe Operators.indirection

      inside(indirection.argument.l) { case (pointer: Identifier) :: Nil =>
        pointer.code shouldBe "p"
      }
    }
  }

  "be created for `formatString` operator" in {
    val cpg = code(basicBoilerplate("""
        |var world = "World!";
        |var foo = $"Hello, {world}";
        |""".stripMargin))

    inside(cpg.call(Operators.formatString).l) { case interpolatedString :: Nil =>
      interpolatedString.code shouldBe "$\"Hello, {world}\""
      interpolatedString.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.String)
      interpolatedString.methodFullName shouldBe Operators.formatString

      inside(interpolatedString.argument.l) { case (hello: Literal) :: (world: Identifier) :: Nil =>
        hello.code shouldBe "Hello,"
        hello.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.String)

        world.code shouldBe "world"
        world.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.String)
      }

      inside(cpg.local("foo").l) { case foo :: Nil =>
        foo.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.String)
      }
    }
  }

  "preserve interpolation alignment and format operands" in {
    val cpg = code(basicBoilerplate("""
        |int value = 12;
        |var foo = $"Value {value,10:X2}!";
        |""".stripMargin))

    inside(cpg.call(Operators.formatString).l) { case interpolatedString :: Nil =>
      interpolatedString.code shouldBe "$\"Value {value,10:X2}!\""
      interpolatedString.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.String)

      inside(interpolatedString.argument.l) {
        case (prefix: Literal) :: (value: Identifier) :: (alignment: Literal) :: (format: Literal) :: (suffix: Literal) :: Nil =>
          prefix.code shouldBe "Value"
          value.code shouldBe "value"
          alignment.code shouldBe "10"
          format.code shouldBe "X2"
          suffix.code shouldBe "!"
      }
    }
  }

  "preserve raw interpolated string code" in {
    val rawInterpolated = "$$\"\"\"Value {{value}}\"\"\""
    val cpg = code(basicBoilerplate(s"""
        |int value = 7;
        |var raw = $rawInterpolated;
        |""".stripMargin))

    inside(cpg.call(Operators.formatString).l) { case interpolatedString :: Nil =>
      interpolatedString.code shouldBe rawInterpolated
      interpolatedString.typeFullName shouldBe BuiltinTypes.DotNetTypeMap(BuiltinTypes.String)

      inside(interpolatedString.argument.l) { case (prefix: Literal) :: (value: Identifier) :: Nil =>
        prefix.code shouldBe "Value"
        value.code shouldBe "value"
      }
    }
  }
}
