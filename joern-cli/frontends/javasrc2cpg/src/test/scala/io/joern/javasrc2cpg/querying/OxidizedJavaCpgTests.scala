package io.joern.javasrc2cpg.querying

import io.joern.javasrc2cpg.{Config, JavaParserBackend}
import io.joern.javasrc2cpg.testfixtures.JavaSrcCode2CpgFixture
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, ModifierTypes, Operators}
import io.shiftleft.codepropertygraph.generated.nodes.{
  Binding,
  Block,
  Call,
  ControlStructure,
  FieldIdentifier,
  Identifier,
  JumpTarget,
  Literal,
  Local,
  MethodRef,
  Modifier,
  Return,
  TypeDecl,
  TypeRef
}
import io.shiftleft.semanticcpg.language.*

class OxidizedJavaCpgTests extends JavaSrcCode2CpgFixture(withOssDataflow = false) {

  "oxidized Java parser backend" should {

    "create structural CPG nodes for packages, classes, fields, constructors, and methods" in {
      val cpg = code(
        """package demo;
          |
          |public class Foo {
          |  private int value;
          |
          |  public Foo(int value) {
          |    this.value = value;
          |  }
          |
          |  public int add(int x) {
          |    return value + x;
          |  }
          |
          |  public static int twice(int x) {
          |    return x * 2;
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      cpg.namespaceBlock.name("demo").filename.l shouldBe List("demo/Foo.java")

      val List(foo) = cpg.typeDecl.nameExact("Foo").l: @unchecked
      foo.fullName shouldBe "demo.Foo"
      foo.code shouldBe "public class Foo"
      foo.inheritsFromTypeFullName should contain("java.lang.Object")
      foo.modifier.modifierType.l shouldBe List(ModifierTypes.PUBLIC)

      val List(value) = cpg.member.nameExact("value").l: @unchecked
      value.code shouldBe "int value"
      value.typeFullName shouldBe "int"
      value.modifier.modifierType.l shouldBe List(ModifierTypes.PRIVATE)

      val List(ctor) = cpg.method.nameExact("<init>").l: @unchecked
      ctor.fullName shouldBe "demo.Foo.<init>:void(int)"
      ctor.signature shouldBe "void(int)"
      ctor.parameter.name.l shouldBe List("this", "value")
      ctor.modifier.modifierType.toSet shouldBe Set(ModifierTypes.PUBLIC, ModifierTypes.CONSTRUCTOR)

      val List(add) = cpg.method.nameExact("add").l: @unchecked
      add.fullName shouldBe "demo.Foo.add:int(int)"
      add.signature shouldBe "int(int)"
      add.parameter.name.l shouldBe List("this", "x")
      add.methodReturn.typeFullName shouldBe "int"
      add.modifier.modifierType.toSet shouldBe Set(ModifierTypes.PUBLIC, ModifierTypes.VIRTUAL)

      val List(twice) = cpg.method.nameExact("twice").l: @unchecked
      twice.fullName shouldBe "demo.Foo.twice:int(int)"
      twice.parameter.name.l shouldBe List("x")
      twice.modifier.modifierType.toSet shouldBe Set(ModifierTypes.PUBLIC, ModifierTypes.STATIC)
    }

    "create method body ASTs for locals, calls, operators, field/index access, and returns" in {
      val cpg = code(
        """package demo;
          |
          |public class Foo {
          |  private int value;
          |
          |  public int add(int[] values, int x) {
          |    int y = x + 1;
          |    int len = values.length;
          |    this.value = y;
          |    values[0] = y;
          |    System.out.println(values[0]);
          |    if (y > 0) {
          |      return value + y;
          |    } else {
          |      return 0;
          |    }
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(addMethod) = cpg.method.nameExact("add").l: @unchecked
      def addAst          = addMethod.ast
      def addCalls        = addAst.collectAll[Call]

      addAst.collectAll[Local].nameExact("y").typeFullName.l shouldBe List("int")
      addAst.collectAll[Local].nameExact("len").typeFullName.l shouldBe List("int")

      addCalls.nameExact(Operators.assignment).code.l should contain allOf (
        "int y = x + 1",
        "int len = values.length",
        "this.value = y",
        "values[0] = y"
      )
      addCalls.nameExact(Operators.addition).code.l should contain allOf ("x + 1", "value + y")
      val List(sizeOfCall: Call) = addCalls.nameExact(Operators.sizeOf).l: @unchecked
      sizeOfCall.code shouldBe "values.length"
      sizeOfCall.typeFullName shouldBe "int"
      inside(sizeOfCall.argument.l) { case List(valuesIdentifier: Identifier) =>
        valuesIdentifier.name shouldBe "values"
        valuesIdentifier.refsTo.l shouldBe addMethod.parameter.nameExact("values").l
      }
      addCalls.nameExact(Operators.fieldAccess).code.l should contain("this.value")
      addCalls.nameExact(Operators.indexAccess).code.l should contain("values[0]")

      val List(printlnCall) = addCalls.nameExact("println").l: @unchecked
      printlnCall.code shouldBe "System.out.println(values[0])"
      printlnCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

      addAst.collectAll[Return].code.l should contain allOf ("return value + y;", "return 0;")
      val List(ifNode) =
        addAst.collectAll[ControlStructure].controlStructureType(ControlStructureTypes.IF).l: @unchecked
      ifNode.code should include("if")
    }

    "type explicit super receivers for superclass calls and field access" in {
      val cpg = code(
        """package demo;
          |
          |class Base {
          |  protected int baseValue;
          |
          |  int compute(int value) {
          |    return value + baseValue;
          |  }
          |}
          |
          |class Child extends Base {
          |  int use(int seed) {
          |    return super.compute(seed) + super.baseValue;
          |  }
          |}
          |""".stripMargin,
        "demo/Child.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.fullNameExact("demo.Child.use:int(int)").l: @unchecked
      val List(superCall) = useMethod.ast.isCall.nameExact("compute").l: @unchecked
      superCall.methodFullName shouldBe "demo.Base.compute:int(int)"
      superCall.signature shouldBe "int(int)"
      superCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
      inside(superCall.receiver.l) { case List(receiver: Identifier) =>
        receiver.name shouldBe "this"
        receiver.code shouldBe "super"
        receiver.typeFullName shouldBe "demo.Base"
        receiver.refsTo.l shouldBe useMethod.parameter.nameExact("this").l
      }

      val List(superField) =
        useMethod.ast.isCall.nameExact(Operators.fieldAccess).codeExact("super.baseValue").l: @unchecked
      superField.typeFullName shouldBe "int"
      inside(superField.argument.l) { case List(receiver: Identifier, field: FieldIdentifier) =>
        receiver.name shouldBe "this"
        receiver.code shouldBe "super"
        receiver.typeFullName shouldBe "demo.Base"
        receiver.refsTo.l shouldBe useMethod.parameter.nameExact("this").l
        field.canonicalName shouldBe "baseValue"
      }
    }

    "resolve inherited methods and fields for explicit subclass receivers" in {
      val cpg = code(
        """package demo;
          |
          |class Base {
          |  protected int baseValue;
          |
          |  int compute(int value) {
          |    return value + baseValue;
          |  }
          |
          |  static String label() {
          |    return "base";
          |  }
          |}
          |
          |class Other {
          |  String baseValue;
          |}
          |
          |class Child extends Base {}
          |
          |class GrandChild extends Child {}
          |
          |class Use {
          |  int use(GrandChild child) {
          |    return child.compute(1) + child.baseValue;
          |  }
          |
          |  String label() {
          |    return GrandChild.label();
          |  }
          |}
          |""".stripMargin,
        "demo/InheritedReceiver.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod)   = cpg.method.fullNameExact("demo.Use.use:int(demo.GrandChild)").l: @unchecked
      val List(computeCall) = useMethod.ast.isCall.nameExact("compute").l: @unchecked
      computeCall.methodFullName shouldBe "demo.GrandChild.compute:int(int)"
      computeCall.signature shouldBe "int(int)"
      computeCall.typeFullName shouldBe "int"
      computeCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
      inside(computeCall.receiver.l) { case List(receiver: Identifier) =>
        receiver.name shouldBe "child"
        receiver.typeFullName shouldBe "demo.GrandChild"
        receiver.refsTo.l shouldBe useMethod.parameter.nameExact("child").l
      }

      val List(baseValueAccess) =
        useMethod.ast.isCall.nameExact(Operators.fieldAccess).codeExact("child.baseValue").l: @unchecked
      baseValueAccess.typeFullName shouldBe "int"
      inside(baseValueAccess.argument.l) { case List(receiver: Identifier, field: FieldIdentifier) =>
        receiver.name shouldBe "child"
        receiver.typeFullName shouldBe "demo.GrandChild"
        receiver.refsTo.l shouldBe useMethod.parameter.nameExact("child").l
        field.canonicalName shouldBe "baseValue"
      }

      val List(labelCall) =
        cpg.method.fullNameExact("demo.Use.label:java.lang.String()").call.nameExact("label").l: @unchecked
      labelCall.methodFullName shouldBe "demo.Base.label:java.lang.String()"
      labelCall.signature shouldBe "java.lang.String()"
      labelCall.typeFullName shouldBe "java.lang.String"
      labelCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
    }

    "lower type-pattern instanceof expressions and bind pattern locals" in {
      val cpg = code(
        """package demo;
          |
          |public class Patterns {
          |  public int use(Object o) {
          |    if (o instanceof String s) {
          |      return s.length();
          |    }
          |    return 0;
          |  }
          |}
          |""".stripMargin,
        "demo/Patterns.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod)           = cpg.method.nameExact("use").l: @unchecked
      val List(patternLocal: Local) = useMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      patternLocal.code shouldBe "String s"
      patternLocal.typeFullName shouldBe "java.lang.String"

      val List(ifNode) =
        useMethod.ast.collectAll[ControlStructure].controlStructureType(ControlStructureTypes.IF).l: @unchecked
      inside(ifNode.condition.l) { case List(andCall: Call) =>
        andCall.name shouldBe Operators.logicalAnd
        andCall.code shouldBe "(o instanceof String) && { s = (String) o; true; }"

        inside(andCall.argument.l) { case List(instanceOfCall: Call, assignmentBlock: Block) =>
          instanceOfCall.name shouldBe Operators.instanceOf
          instanceOfCall.code shouldBe "o instanceof String"
          inside(instanceOfCall.argument.l) { case List(oIdentifier: Identifier, stringType: TypeRef) =>
            oIdentifier.name shouldBe "o"
            oIdentifier.refsTo.l shouldBe useMethod.parameter.nameExact("o").l
            stringType.typeFullName shouldBe "java.lang.String"
          }

          inside(assignmentBlock.astChildren.l) { case List(sAssign: Call, _: Literal) =>
            sAssign.name shouldBe Operators.assignment
            sAssign.code shouldBe "s = (String) o"
            sAssign.typeFullName shouldBe "java.lang.String"
            inside(sAssign.argument.l) { case List(sIdentifier: Identifier, castCall: Call) =>
              sIdentifier.name shouldBe "s"
              sIdentifier.typeFullName shouldBe "java.lang.String"
              sIdentifier.refsTo.l shouldBe List(patternLocal)
              castCall.name shouldBe Operators.cast
              castCall.typeFullName shouldBe "java.lang.String"
            }
          }
        }
      }

      val List(lengthCall) = useMethod.ast.collectAll[Call].nameExact("length").l: @unchecked
      inside(lengthCall.argument.l) { case List(sIdentifier: Identifier) =>
        sIdentifier.name shouldBe "s"
        sIdentifier.typeFullName shouldBe "java.lang.String"
        sIdentifier.refsTo.l shouldBe List(patternLocal)
      }
    }

    "lower type-pattern instanceof call lhs expressions through temporaries" in {
      val cpg = code(
        """class CallLhsPattern {
          |  Object source() {
          |    return "abc";
          |  }
          |
          |  void use() {
          |    if (source() instanceof String s) {
          |      sink(s);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "CallLhsPattern.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      inside(useMethod.body.astChildren.l) {
        case List(tmpLocal: Local, patternLocal: Local, ifNode: ControlStructure) =>
          tmpLocal.name shouldBe "$obj0"
          tmpLocal.typeFullName shouldBe "java.lang.Object"
          patternLocal.name shouldBe "s"
          patternLocal.typeFullName shouldBe "java.lang.String"

          val List(condition: Call) = ifNode.condition.l: @unchecked
          condition.code shouldBe "(($obj0 = source()) instanceof String) && { s = (String) $obj0; true; }"
          inside(condition.argument.l) { case List(instanceOfCall: Call, assignmentBlock: Block) =>
            instanceOfCall.code shouldBe "($obj0 = source()) instanceof String"
            inside(instanceOfCall.argument.l) { case List(tmpAssignment: Call, stringType: TypeRef) =>
              stringType.typeFullName shouldBe "java.lang.String"
              tmpAssignment.code shouldBe "$obj0 = source()"
              tmpAssignment.typeFullName shouldBe "java.lang.Object"
              inside(tmpAssignment.argument.l) { case List(tmpIdentifier: Identifier, sourceCall: Call) =>
                tmpIdentifier.refsTo.l shouldBe List(tmpLocal)
                sourceCall.code shouldBe "source()"
                sourceCall.methodFullName shouldBe "CallLhsPattern.source:java.lang.Object()"
              }
            }

            inside(assignmentBlock.astChildren.l) { case List(sAssignment: Call, _: Literal) =>
              sAssignment.code shouldBe "s = (String) $obj0"
              inside(sAssignment.argument.l) { case List(sIdentifier: Identifier, castCall: Call) =>
                sIdentifier.refsTo.l shouldBe List(patternLocal)
                castCall.code shouldBe "(String) $obj0"
                castCall.argument.isIdentifier.nameExact("$obj0").refsTo.l shouldBe List(tmpLocal)
              }
            }
          }
      }
    }

    "lower type-pattern instanceof call lhs expressions in local initializers through temporaries" in {
      val cpg = code(
        """class InitializerCallLhsPattern {
          |  Object source() {
          |    return "abc";
          |  }
          |
          |  void use() {
          |    boolean matched = source() instanceof String s;
          |  }
          |}
          |""".stripMargin,
        "InitializerCallLhsPattern.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      inside(useMethod.body.astChildren.l) {
        case List(tmpLocal: Local, patternLocal: Local, matchedLocal: Local, matchedAssign: Call) =>
          tmpLocal.name shouldBe "$obj0"
          tmpLocal.typeFullName shouldBe "java.lang.Object"
          patternLocal.name shouldBe "s"
          patternLocal.typeFullName shouldBe "java.lang.String"
          matchedLocal.name shouldBe "matched"
          matchedLocal.typeFullName shouldBe "boolean"

          matchedAssign.code shouldBe "boolean matched = source() instanceof String s"
          inside(matchedAssign.argument.l) { case List(matchedIdentifier: Identifier, condition: Call) =>
            matchedIdentifier.refsTo.l shouldBe List(matchedLocal)
            condition.code shouldBe "(($obj0 = source()) instanceof String) && { s = (String) $obj0; true; }"
            inside(condition.argument.l) { case List(instanceOfCall: Call, assignmentBlock: Block) =>
              inside(instanceOfCall.argument.l) { case List(tmpAssignment: Call, _: TypeRef) =>
                tmpAssignment.code shouldBe "$obj0 = source()"
                tmpAssignment.argument.isIdentifier.nameExact("$obj0").refsTo.l shouldBe List(tmpLocal)
              }
              inside(assignmentBlock.astChildren.l) { case List(sAssignment: Call, _: Literal) =>
                sAssignment.code shouldBe "s = (String) $obj0"
                sAssignment.argument.isIdentifier.nameExact("s").refsTo.l shouldBe List(patternLocal)
              }
            }
          }
      }
    }

    "lower type-pattern locals from instance field initializers into constructors" in {
      val cpg = code(
        """class FieldInitializerPattern {
          |  Object source() {
          |    return "abc";
          |  }
          |
          |  int x = source() instanceof String s ? s.length() : -1;
          |}
          |""".stripMargin,
        "FieldInitializerPattern.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(ctor) = cpg.typeDecl.nameExact("FieldInitializerPattern").method.nameExact("<init>").l: @unchecked
      inside(ctor.body.astChildren.l) { case List(tmpLocal: Local, patternLocal: Local, xAssign: Call) =>
        tmpLocal.name shouldBe "$obj0"
        tmpLocal.typeFullName shouldBe "java.lang.Object"
        patternLocal.name shouldBe "s"
        patternLocal.typeFullName shouldBe "java.lang.String"

        xAssign.name shouldBe Operators.assignment
        xAssign.code shouldBe "this.x = source() instanceof String s ? s.length() : -1"
        inside(xAssign.argument.l) { case List(xFieldAccess: Call, ternary: Call) =>
          xFieldAccess.code shouldBe "this.x"
          ternary.name shouldBe Operators.conditional
          inside(ternary.argument.l) { case List(condition: Call, lengthCall: Call, minusCall: Call) =>
            condition.code shouldBe "(($obj0 = source()) instanceof String) && { s = (String) $obj0; true; }"
            lengthCall.code shouldBe "s.length()"
            lengthCall.argument.isIdentifier.nameExact("s").refsTo.l shouldBe List(patternLocal)
            minusCall.name shouldBe Operators.minus

            inside(condition.argument.l) { case List(instanceOfCall: Call, assignmentBlock: Block) =>
              inside(instanceOfCall.argument.l) { case List(tmpAssignment: Call, _: TypeRef) =>
                tmpAssignment.code shouldBe "$obj0 = source()"
                tmpAssignment.argument.isIdentifier.nameExact("$obj0").refsTo.l shouldBe List(tmpLocal)
              }
              assignmentBlock.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
                "s = (String) $obj0"
              )
            }
          }
        }
      }
    }

    "lower type-pattern locals from static field initializers into clinit" in {
      val cpg = code(
        """class StaticFieldInitializerPattern {
          |  static Object source() {
          |    return "abc";
          |  }
          |
          |  static int x = source() instanceof String s ? s.length() : -1;
          |}
          |""".stripMargin,
        "StaticFieldInitializerPattern.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(clinit) =
        cpg.typeDecl.nameExact("StaticFieldInitializerPattern").method.nameExact("<clinit>").l: @unchecked
      inside(clinit.body.astChildren.l) { case List(tmpLocal: Local, patternLocal: Local, xAssign: Call) =>
        tmpLocal.name shouldBe "$obj0"
        tmpLocal.typeFullName shouldBe "java.lang.Object"
        patternLocal.name shouldBe "s"
        patternLocal.typeFullName shouldBe "java.lang.String"

        xAssign.name shouldBe Operators.assignment
        xAssign.code shouldBe "StaticFieldInitializerPattern.x = source() instanceof String s ? s.length() : -1"
        inside(xAssign.argument.l) { case List(xFieldAccess: Call, ternary: Call) =>
          xFieldAccess.code shouldBe "StaticFieldInitializerPattern.x"
          ternary.name shouldBe Operators.conditional
          inside(ternary.argument.l) { case List(condition: Call, lengthCall: Call, minusCall: Call) =>
            condition.code shouldBe "(($obj0 = source()) instanceof String) && { s = (String) $obj0; true; }"
            lengthCall.code shouldBe "s.length()"
            lengthCall.argument.isIdentifier.nameExact("s").refsTo.l shouldBe List(patternLocal)
            minusCall.name shouldBe Operators.minus

            inside(condition.argument.l) { case List(instanceOfCall: Call, assignmentBlock: Block) =>
              inside(instanceOfCall.argument.l) { case List(tmpAssignment: Call, _: TypeRef) =>
                tmpAssignment.code shouldBe "$obj0 = source()"
                tmpAssignment.argument.isIdentifier.nameExact("$obj0").refsTo.l shouldBe List(tmpLocal)
              }
              assignmentBlock.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
                "s = (String) $obj0"
              )
            }
          }
        }
      }
    }

    "mangle later local declarations that collide with pattern locals in the same block" in {
      val cpg = code(
        """class PatternLocalNameCollision {
          |  static void sink(Object value) {}
          |
          |  static void foo(Object o) {
          |    if (o instanceof String value) {
          |      sink(value);
          |    }
          |    int value = 2;
          |    sink(value);
          |  }
          |}
          |""".stripMargin,
        "PatternLocalNameCollision.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(stringLocal) = cpg.method.nameExact("foo").local.nameExact("value").l: @unchecked
      stringLocal.typeFullName shouldBe "java.lang.String"
      val List(intLocal) = cpg.method.nameExact("foo").local.nameExact("value$0").l: @unchecked
      intLocal.typeFullName shouldBe "int"

      val sinkArgs = cpg.method.nameExact("foo").call.nameExact("sink").argument.argumentIndex(1).isIdentifier.l
      sinkArgs.map(_.name).toSet shouldBe Set("value", "value$0")
      sinkArgs.find(_.name == "value").toList.flatMap(_.refsTo.l) shouldBe List(stringLocal)
      sinkArgs.find(_.name == "value$0").toList.flatMap(_.refsTo.l) shouldBe List(intLocal)
    }

    "mangle later pattern locals that collide with earlier pattern locals in the same block" in {
      val cpg = code(
        """class PatternPatternNameCollision {
          |  static void sink(Object value) {}
          |
          |  static void foo(Object o) {
          |    if (o instanceof String value) {
          |      sink(value);
          |    }
          |    if (o instanceof Integer value) {
          |      sink(value);
          |    }
          |  }
          |}
          |""".stripMargin,
        "PatternPatternNameCollision.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(stringLocal) = cpg.method.nameExact("foo").local.nameExact("value").l: @unchecked
      stringLocal.typeFullName shouldBe "java.lang.String"
      val List(integerLocal) = cpg.method.nameExact("foo").local.nameExact("value$0").l: @unchecked
      integerLocal.typeFullName shouldBe "java.lang.Integer"

      inside(cpg.method.nameExact("foo").call.nameExact("sink").l) { case List(firstSink, secondSink) =>
        val List(firstValue: Identifier) = firstSink.argument.argumentIndex(1).isIdentifier.l: @unchecked
        firstValue.name shouldBe "value"
        firstValue.refsTo.l shouldBe List(stringLocal)

        val List(secondValue: Identifier) = secondSink.argument.argumentIndex(1).isIdentifier.l: @unchecked
        secondValue.name shouldBe "value$0"
        secondValue.refsTo.l shouldBe List(integerLocal)
      }
    }

    "reuse same-type pattern and local declarations with the same source name" in {
      val cpg = code(
        """class SameTypePatternLocalName {
          |  static void sink0(String value) {}
          |  static void sink1(String value) {}
          |  static void sink2(String value) {}
          |
          |  static void foo(Object o) {
          |    if (o instanceof String s) {
          |      sink0(s);
          |    }
          |    if (o instanceof String s) {
          |      sink1(s);
          |    }
          |    String s = "safe";
          |    sink2(s);
          |  }
          |}
          |""".stripMargin,
        "SameTypePatternLocalName.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(sLocal) = cpg.method.nameExact("foo").local.nameExact("s").l: @unchecked
      sLocal.typeFullName shouldBe "java.lang.String"
      cpg.method.nameExact("foo").local.nameExact("s$0").l shouldBe Nil

      List("sink0", "sink1", "sink2").foreach { sinkName =>
        val List(sIdentifier: Identifier) =
          cpg.method.nameExact("foo").call.nameExact(sinkName).argument.argumentIndex(1).isIdentifier.l: @unchecked
        sIdentifier.name shouldBe "s"
        sIdentifier.refsTo.l shouldBe List(sLocal)
      }
    }

    "keep same source names unmangled across sibling blocks" in {
      val cpg = code(
        """class SiblingBlockPatternLocalName {
          |  static void sink(Object value) {}
          |
          |  static void foo(Object o) {
          |    {
          |      if (o instanceof String value) {
          |        sink(value);
          |      }
          |    }
          |    {
          |      int value = 2;
          |      sink(value);
          |    }
          |  }
          |}
          |""".stripMargin,
        "SiblingBlockPatternLocalName.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val valueLocals = cpg.method.nameExact("foo").local.nameExact("value").l
      valueLocals.map(_.typeFullName).toSet shouldBe Set("java.lang.String", "int")
      cpg.method.nameExact("foo").local.nameExact("value$0").l shouldBe Nil

      cpg.method.nameExact("foo").call.nameExact("sink").argument.argumentIndex(1).isIdentifier.name.l shouldBe List(
        "value",
        "value"
      )
    }

    "mangle switch pattern locals when an enclosing pattern local already uses the source name" in {
      val cpg = code(
        """class SwitchPatternNameCollision {
          |  static void sink(Object value) {}
          |
          |  static void foo(Object o) {
          |    if (o instanceof String value) {
          |      sink(value);
          |    }
          |    switch (o) {
          |      case Integer value -> sink(value);
          |      default -> sink("default");
          |    }
          |  }
          |}
          |""".stripMargin,
        "SwitchPatternNameCollision.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(stringLocal) = cpg.method.nameExact("foo").local.nameExact("value").l: @unchecked
      stringLocal.typeFullName shouldBe "java.lang.String"
      val List(integerLocal) = cpg.method.nameExact("foo").local.nameExact("value$0").l: @unchecked
      integerLocal.typeFullName shouldBe "java.lang.Integer"

      val sinkArgs = cpg.method.nameExact("foo").call.nameExact("sink").argument.argumentIndex(1).isIdentifier.l
      sinkArgs.map(_.name).toSet should contain allOf ("value", "value$0")
      sinkArgs.find(_.name == "value").toList.flatMap(_.refsTo.l) shouldBe List(stringLocal)
      sinkArgs.find(_.name == "value$0").toList.flatMap(_.refsTo.l) shouldBe List(integerLocal)
    }

    "keep switch pattern locals with the same source name isolated across case blocks" in {
      val cpg = code(
        """class SwitchCasePatternNameIsolation {
          |  static void sink(Object value) {}
          |
          |  static void foo(Object o) {
          |    switch (o) {
          |      case Integer value -> sink(value);
          |      case Boolean value -> sink(value);
          |      default -> sink("default");
          |    }
          |    if (o instanceof String value) {
          |      sink(value);
          |    }
          |  }
          |}
          |""".stripMargin,
        "SwitchCasePatternNameIsolation.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      cpg.method.nameExact("foo").local.nameExact("value$0").l shouldBe Nil
      cpg.method.nameExact("foo").local.nameExact("value").typeFullName.l.toSet shouldBe Set(
        "java.lang.Integer",
        "java.lang.Boolean",
        "java.lang.String"
      )
      cpg.method
        .nameExact("foo")
        .call
        .nameExact("sink")
        .argument
        .argumentIndex(1)
        .isIdentifier
        .name
        .l
        .toSet shouldBe Set("value")
    }

    "mangle binary RHS references when the left pattern local collides with an earlier local" in {
      val cpg = code(
        """class BinaryPatternNameCollision {
          |  static void sink(Object value) {}
          |
          |  static void foo(Object o) {
          |    if (o instanceof Integer value) {
          |      sink(value);
          |    }
          |    if (o instanceof String value && value.isEmpty()) {
          |      sink(value);
          |    }
          |  }
          |}
          |""".stripMargin,
        "BinaryPatternNameCollision.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(integerLocal) = cpg.method.nameExact("foo").local.nameExact("value").l: @unchecked
      integerLocal.typeFullName shouldBe "java.lang.Integer"
      val List(stringLocal) = cpg.method.nameExact("foo").local.nameExact("value$0").l: @unchecked
      stringLocal.typeFullName shouldBe "java.lang.String"

      val List(isEmptyReceiver: Identifier) =
        cpg.method.nameExact("foo").call.nameExact("isEmpty").receiver.isIdentifier.l: @unchecked
      isEmptyReceiver.name shouldBe "value$0"
      isEmptyReceiver.refsTo.l shouldBe List(stringLocal)
    }

    "mangle local declarations in nested blocks when an enclosing pattern local uses the source name" in {
      val cpg = code(
        """class NestedBlockPatternLocalName {
          |  static void sink(Object value) {}
          |
          |  static void foo(Object o) {
          |    if (o instanceof String value) {
          |      sink(value);
          |    }
          |    {
          |      int value = 2;
          |      sink(value);
          |    }
          |  }
          |}
          |""".stripMargin,
        "NestedBlockPatternLocalName.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(stringLocal) = cpg.method.nameExact("foo").local.nameExact("value").l: @unchecked
      stringLocal.typeFullName shouldBe "java.lang.String"
      val List(intLocal) = cpg.method.nameExact("foo").local.nameExact("value$0").l: @unchecked
      intLocal.typeFullName shouldBe "int"

      val sinkArgs = cpg.method.nameExact("foo").call.nameExact("sink").argument.argumentIndex(1).isIdentifier.l
      sinkArgs.map(_.name).toSet shouldBe Set("value", "value$0")
      sinkArgs.find(_.name == "value").toList.flatMap(_.refsTo.l) shouldBe List(stringLocal)
      sinkArgs.find(_.name == "value$0").toList.flatMap(_.refsTo.l) shouldBe List(intLocal)
    }

    "lower unresolved type-pattern instanceof expressions to ANY without an import fallback" in {
      val cpg = code(
        """class UnresolvedPattern {
          |  void use(Object o) {
          |    if (o instanceof Bar b) {
          |      sink(b);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "UnresolvedPattern.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod)           = cpg.method.nameExact("use").l: @unchecked
      val List(patternLocal: Local) = useMethod.body.astChildren.collectAll[Local].nameExact("b").l: @unchecked
      patternLocal.code shouldBe "Bar b"
      patternLocal.typeFullName shouldBe "ANY"

      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.code shouldBe "(o instanceof Bar) && { b = (Bar) o; true; }"
      inside(condition.argument.l) { case List(instanceOfBar: Call, assignmentBlock: Block) =>
        instanceOfBar.argument.isTypeRef.typeFullName.l shouldBe List("ANY")
        inside(assignmentBlock.astChildren.l) { case List(bAssign: Call, _: Literal) =>
          bAssign.typeFullName shouldBe "ANY"
          inside(bAssign.argument.l) { case List(bIdentifier: Identifier, castCall: Call) =>
            bIdentifier.refsTo.l shouldBe List(patternLocal)
            bIdentifier.typeFullName shouldBe "ANY"
            castCall.name shouldBe Operators.cast
            castCall.typeFullName shouldBe "ANY"
            castCall.argument.isTypeRef.typeFullName.l shouldBe List("ANY")
          }
        }
      }
    }

    "lower unresolved type-pattern instanceof expressions through import fallback" in {
      val cpg = code(
        """import bar.Bar;
          |
          |class ImportedPattern {
          |  void use(Object o) {
          |    if (o instanceof Bar b) {
          |      sink(b);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "ImportedPattern.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod)           = cpg.method.nameExact("use").l: @unchecked
      val List(patternLocal: Local) = useMethod.body.astChildren.collectAll[Local].nameExact("b").l: @unchecked
      patternLocal.typeFullName shouldBe "bar.Bar"

      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.code shouldBe "(o instanceof Bar) && { b = (Bar) o; true; }"
      inside(condition.argument.l) { case List(instanceOfBar: Call, assignmentBlock: Block) =>
        instanceOfBar.argument.isTypeRef.typeFullName.l shouldBe List("bar.Bar")
        inside(assignmentBlock.astChildren.l) { case List(bAssign: Call, _: Literal) =>
          bAssign.typeFullName shouldBe "bar.Bar"
          inside(bAssign.argument.l) { case List(bIdentifier: Identifier, castCall: Call) =>
            bIdentifier.refsTo.l shouldBe List(patternLocal)
            bIdentifier.typeFullName shouldBe "bar.Bar"
            castCall.typeFullName shouldBe "bar.Bar"
            castCall.argument.isTypeRef.typeFullName.l shouldBe List("bar.Bar")
          }
        }
      }
    }

    "lower unresolved nested record patterns through ANY and unknown accessors" in {
      val cpg = code(
        """class UnresolvedRecordPattern {
          |  void use(Object o) {
          |    if (o instanceof Bar(Baz(Qux q))) {
          |      sink(q);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "UnresolvedRecordPattern.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      useMethod.body.astChildren.collectAll[Local].nameExact("$obj0").typeFullName.l shouldBe List("ANY")
      useMethod.body.astChildren.collectAll[Local].nameExact("$obj1").typeFullName.l shouldBe List("ANY")
      val List(patternLocal: Local) = useMethod.body.astChildren.collectAll[Local].nameExact("q").l: @unchecked
      patternLocal.typeFullName shouldBe "ANY"

      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.code shouldBe "((o instanceof Bar) && ((($obj0 = ((Bar) o).<unknownField>()) instanceof Baz) && (($obj1 = ((Baz) $obj0).<unknownField>()) instanceof Qux))) && { q = (Qux) $obj1; true; }"
      useMethod.ast.collectAll[Call].nameExact(Operators.instanceOf).argument.isTypeRef.typeFullName.l shouldBe List(
        "ANY",
        "ANY",
        "ANY"
      )

      val unknownFieldCalls = useMethod.ast.collectAll[Call].nameExact("<unknownField>").l
      unknownFieldCalls.map(_.methodFullName) should contain allOf (
        "<unresolvedNamespace>.Bar.<unknownField>:<unresolvedSignature>(0)",
        "<unresolvedNamespace>.Baz.<unknownField>:<unresolvedSignature>(0)"
      )
      unknownFieldCalls.map(_.typeFullName).distinct shouldBe List("ANY")

      val List(qAssign: Call) =
        useMethod.ast.collectAll[Call].nameExact(Operators.assignment).codeExact("q = (Qux) $obj1").l: @unchecked
      qAssign.typeFullName shouldBe "ANY"
      qAssign.argument.isIdentifier.nameExact("q").refsTo.l shouldBe List(patternLocal)
      qAssign.argument.collectAll[Call].nameExact(Operators.cast).typeFullName.l shouldBe List("ANY")
      qAssign.argument.collectAll[Call].nameExact(Operators.cast).argument.isTypeRef.typeFullName.l shouldBe List("ANY")
    }

    "limit positive type-pattern locals to the then branch" in {
      val cpg = code(
        """package demo;
          |
          |public class PatternScope {
          |  Integer s;
          |
          |  public void use(Object o) {
          |    if (o instanceof String s) {
          |      sink(s);
          |    } else {
          |      sink(s);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/PatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod)           = cpg.method.nameExact("use").l: @unchecked
      val List(patternLocal: Local) = useMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      patternLocal.typeFullName shouldBe "java.lang.String"

      val List(thenSink, elseSink) = useMethod.ast.collectAll[Call].nameExact("sink").l: @unchecked
      inside(thenSink.argument.l) { case List(sIdentifier: Identifier) =>
        sIdentifier.name shouldBe "s"
        sIdentifier.typeFullName shouldBe "java.lang.String"
        sIdentifier.refsTo.l shouldBe List(patternLocal)
      }

      inside(elseSink.argument.l) { case List(fieldAccess: Call) =>
        fieldAccess.name shouldBe Operators.fieldAccess
        fieldAccess.code shouldBe "this.s"
        fieldAccess.typeFullName shouldBe "java.lang.Integer"
        fieldAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List("s")
      }
    }

    "expose negated type-pattern locals after exiting then branches" in {
      val cpg = code(
        """package demo;
          |
          |public class PatternScopeAfter {
          |  Integer s;
          |
          |  public void use(Object o) {
          |    if (!(o instanceof String s)) {
          |      return;
          |    }
          |    sink(s);
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/PatternScopeAfter.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod)           = cpg.method.nameExact("use").l: @unchecked
      val List(patternLocal: Local) = useMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      patternLocal.typeFullName shouldBe "java.lang.String"

      val List(sinkCall) = useMethod.ast.collectAll[Call].nameExact("sink").l: @unchecked
      inside(sinkCall.argument.l) { case List(sIdentifier: Identifier) =>
        sIdentifier.name shouldBe "s"
        sIdentifier.typeFullName shouldBe "java.lang.String"
        sIdentifier.refsTo.l shouldBe List(patternLocal)
      }
    }

    "scope type-pattern locals across while bodies and exits" in {
      val cpg = code(
        """package demo;
          |
          |public class PatternScopeWhile {
          |  Integer s;
          |
          |  public void positive(Object o) {
          |    while (o instanceof String s) {
          |      sink(s);
          |    }
          |    sink(s);
          |  }
          |
          |  public void negated(Object o) {
          |    while (!(o instanceof String s)) {
          |      sink(s);
          |    }
          |    sink(s);
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/PatternScopeWhile.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(positiveMethod) = cpg.method.nameExact("positive").l: @unchecked
      val List(positivePatternLocal: Local) =
        positiveMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      positivePatternLocal.typeFullName shouldBe "java.lang.String"
      val List(positiveBodySink, positiveAfterSink) =
        positiveMethod.ast.collectAll[Call].nameExact("sink").l: @unchecked
      inside(positiveBodySink.argument.l) { case List(sIdentifier: Identifier) =>
        sIdentifier.name shouldBe "s"
        sIdentifier.typeFullName shouldBe "java.lang.String"
        sIdentifier.refsTo.l shouldBe List(positivePatternLocal)
      }
      inside(positiveAfterSink.argument.l) { case List(fieldAccess: Call) =>
        fieldAccess.name shouldBe Operators.fieldAccess
        fieldAccess.code shouldBe "this.s"
        fieldAccess.typeFullName shouldBe "java.lang.Integer"
      }

      val List(negatedMethod) = cpg.method.nameExact("negated").l: @unchecked
      val List(negatedPatternLocal: Local) =
        negatedMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      negatedPatternLocal.typeFullName shouldBe "java.lang.String"
      val List(negatedBodySink, negatedAfterSink) = negatedMethod.ast.collectAll[Call].nameExact("sink").l: @unchecked
      inside(negatedBodySink.argument.l) { case List(fieldAccess: Call) =>
        fieldAccess.name shouldBe Operators.fieldAccess
        fieldAccess.code shouldBe "this.s"
        fieldAccess.typeFullName shouldBe "java.lang.Integer"
      }
      inside(negatedAfterSink.argument.l) { case List(sIdentifier: Identifier) =>
        sIdentifier.name shouldBe "s"
        sIdentifier.typeFullName shouldBe "java.lang.String"
        sIdentifier.refsTo.l shouldBe List(negatedPatternLocal)
      }
    }

    "scope type-pattern locals across do-while exits" in {
      val cpg = code(
        """package demo;
          |
          |public class PatternScopeDo {
          |  Integer s;
          |
          |  public void negated(Object o) {
          |    do {
          |      sink(s);
          |    } while (!(o instanceof String s));
          |    sink(s);
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/PatternScopeDo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(negatedMethod) = cpg.method.nameExact("negated").l: @unchecked
      val List(patternLocal: Local) =
        negatedMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      patternLocal.typeFullName shouldBe "java.lang.String"
      val List(bodySink, afterSink) = negatedMethod.ast.collectAll[Call].nameExact("sink").l: @unchecked
      inside(bodySink.argument.l) { case List(fieldAccess: Call) =>
        fieldAccess.name shouldBe Operators.fieldAccess
        fieldAccess.code shouldBe "this.s"
        fieldAccess.typeFullName shouldBe "java.lang.Integer"
      }
      inside(afterSink.argument.l) { case List(sIdentifier: Identifier) =>
        sIdentifier.name shouldBe "s"
        sIdentifier.typeFullName shouldBe "java.lang.String"
        sIdentifier.refsTo.l shouldBe List(patternLocal)
      }
    }

    "scope type-pattern locals across for-loop bodies, updates, and exits" in {
      val cpg = code(
        """package demo;
          |
          |public class PatternScopeFor {
          |  Integer s;
          |
          |  public void positive(Object o) {
          |    for (; o instanceof String s; sink(s)) {
          |      sink(s);
          |    }
          |    sink(s);
          |  }
          |
          |  public void negated(Object o) {
          |    for (; !(o instanceof String s); sink(s)) {
          |      sink(s);
          |    }
          |    sink(s);
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/PatternScopeFor.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(positiveMethod) = cpg.method.nameExact("positive").l: @unchecked
      val List(positivePatternLocal: Local) =
        positiveMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      positivePatternLocal.typeFullName shouldBe "java.lang.String"
      val positiveSinkArgs = positiveMethod.ast.collectAll[Call].nameExact("sink").argument.l
      val positivePatternArgs = positiveSinkArgs.collect {
        case identifier: Identifier if identifier.name == "s" && identifier.typeFullName == "java.lang.String" =>
          identifier
      }
      val positiveFieldArgs = positiveSinkArgs.collect {
        case fieldAccess: Call if fieldAccess.name == Operators.fieldAccess => fieldAccess
      }
      positivePatternArgs.size shouldBe 2
      positivePatternArgs.foreach(_.refsTo.l shouldBe List(positivePatternLocal))
      positiveFieldArgs.size shouldBe 1
      positiveFieldArgs.head.code shouldBe "this.s"
      positiveFieldArgs.head.typeFullName shouldBe "java.lang.Integer"

      val List(negatedMethod) = cpg.method.nameExact("negated").l: @unchecked
      val List(negatedPatternLocal: Local) =
        negatedMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      negatedPatternLocal.typeFullName shouldBe "java.lang.String"
      val negatedSinkArgs = negatedMethod.ast.collectAll[Call].nameExact("sink").argument.l
      val negatedPatternArgs = negatedSinkArgs.collect {
        case identifier: Identifier if identifier.name == "s" && identifier.typeFullName == "java.lang.String" =>
          identifier
      }
      val negatedFieldArgs = negatedSinkArgs.collect {
        case fieldAccess: Call if fieldAccess.name == Operators.fieldAccess => fieldAccess
      }
      negatedPatternArgs.size shouldBe 1
      negatedPatternArgs.head.refsTo.l shouldBe List(negatedPatternLocal)
      negatedFieldArgs.size shouldBe 2
      negatedFieldArgs.foreach { fieldAccess =>
        fieldAccess.code shouldBe "this.s"
        fieldAccess.typeFullName shouldBe "java.lang.Integer"
      }
    }

    "scope type-pattern locals inside binary and ternary expressions" in {
      val cpg = code(
        """package demo;
          |
          |public class PatternScopeExpressions {
          |  Integer s;
          |
          |  public void positiveAnd(Object o) {
          |    if (o instanceof String s && guard(s)) {
          |      sink(s);
          |    }
          |  }
          |
          |  public void negatedOr(Object o) {
          |    if (!(o instanceof String s) || guard(s)) {
          |      sink(s);
          |    } else {
          |      sink(s);
          |    }
          |  }
          |
          |  public int ternaryThen(Object o) {
          |    return o instanceof String s ? use(s) : use(this.s);
          |  }
          |
          |  public int ternaryElse(Object o) {
          |    return !(o instanceof String s) ? use(this.s) : use(s);
          |  }
          |
          |  static boolean guard(Object value) { return true; }
          |  static void sink(Object value) {}
          |  static int use(Object value) { return 0; }
          |}
          |""".stripMargin,
        "demo/PatternScopeExpressions.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(positiveAndMethod) = cpg.method.nameExact("positiveAnd").l: @unchecked
      val List(positiveAndPatternLocal: Local) =
        positiveAndMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      positiveAndPatternLocal.typeFullName shouldBe "java.lang.String"
      val List(positiveAndGuardArg: Identifier) =
        positiveAndMethod.ast
          .collectAll[Call]
          .nameExact("guard")
          .argument
          .collectAll[Identifier]
          .nameExact("s")
          .l: @unchecked
      positiveAndGuardArg.refsTo.l shouldBe List(positiveAndPatternLocal)
      val List(positiveAndSinkArg: Identifier) =
        positiveAndMethod.ast
          .collectAll[Call]
          .nameExact("sink")
          .argument
          .collectAll[Identifier]
          .nameExact("s")
          .l: @unchecked
      positiveAndSinkArg.refsTo.l shouldBe List(positiveAndPatternLocal)

      val List(negatedOrMethod) = cpg.method.nameExact("negatedOr").l: @unchecked
      val List(negatedOrPatternLocal: Local) =
        negatedOrMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      negatedOrPatternLocal.typeFullName shouldBe "java.lang.String"
      val List(negatedOrGuardArg: Identifier) =
        negatedOrMethod.ast
          .collectAll[Call]
          .nameExact("guard")
          .argument
          .collectAll[Identifier]
          .nameExact("s")
          .l: @unchecked
      negatedOrGuardArg.refsTo.l shouldBe List(negatedOrPatternLocal)
      val negatedOrSinkArgs = negatedOrMethod.ast.collectAll[Call].nameExact("sink").argument.l
      val List(elseSinkArg) =
        negatedOrSinkArgs.collect { case identifier: Identifier if identifier.name == "s" => identifier }: @unchecked
      elseSinkArg.typeFullName shouldBe "java.lang.String"
      elseSinkArg.refsTo.l shouldBe List(negatedOrPatternLocal)
      val List(thenSinkArg) =
        negatedOrSinkArgs.collect {
          case fieldAccess: Call if fieldAccess.name == Operators.fieldAccess => fieldAccess
        }: @unchecked
      thenSinkArg.code shouldBe "this.s"
      thenSinkArg.typeFullName shouldBe "java.lang.Integer"

      val List(ternaryThenMethod) = cpg.method.nameExact("ternaryThen").l: @unchecked
      val List(ternaryThenPatternLocal: Local) =
        ternaryThenMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      val ternaryThenUseArgs = ternaryThenMethod.ast.collectAll[Call].nameExact("use").argument.l
      val List(thenUseArg) =
        ternaryThenUseArgs.collect { case identifier: Identifier if identifier.name == "s" => identifier }: @unchecked
      thenUseArg.typeFullName shouldBe "java.lang.String"
      thenUseArg.refsTo.l shouldBe List(ternaryThenPatternLocal)
      val List(elseUseArg) =
        ternaryThenUseArgs.collect {
          case fieldAccess: Call if fieldAccess.name == Operators.fieldAccess => fieldAccess
        }: @unchecked
      elseUseArg.code shouldBe "this.s"
      elseUseArg.typeFullName shouldBe "java.lang.Integer"

      val List(ternaryElseMethod) = cpg.method.nameExact("ternaryElse").l: @unchecked
      val List(ternaryElsePatternLocal: Local) =
        ternaryElseMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      val ternaryElseUseArgs = ternaryElseMethod.ast.collectAll[Call].nameExact("use").argument.l
      val List(elseUseArgForNegatedCondition) =
        ternaryElseUseArgs.collect { case identifier: Identifier if identifier.name == "s" => identifier }: @unchecked
      elseUseArgForNegatedCondition.typeFullName shouldBe "java.lang.String"
      elseUseArgForNegatedCondition.refsTo.l shouldBe List(ternaryElsePatternLocal)
      val List(thenUseArgForNegatedCondition) =
        ternaryElseUseArgs.collect {
          case fieldAccess: Call if fieldAccess.name == Operators.fieldAccess => fieldAccess
        }: @unchecked
      thenUseArgForNegatedCondition.code shouldBe "this.s"
      thenUseArgForNegatedCondition.typeFullName shouldBe "java.lang.Integer"
    }

    "lower simple record-pattern instanceof expressions and bind component locals" in {
      val cpg = code(
        """package demo;
          |
          |record Box(String value) {}
          |
          |public class RecordPatternScope {
          |  public void use(Object o) {
          |    if (o instanceof Box(String s)) {
          |      sink(s);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/RecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod)           = cpg.method.nameExact("use").l: @unchecked
      val List(patternLocal: Local) = useMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      patternLocal.typeFullName shouldBe "java.lang.String"

      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.name shouldBe Operators.logicalAnd
      condition.code shouldBe "(o instanceof Box) && { s = ((Box) o).value(); true; }"
      inside(condition.argument.l) { case List(instanceOfBox: Call, assignmentBlock: Block) =>
        instanceOfBox.name shouldBe Operators.instanceOf
        instanceOfBox.code shouldBe "o instanceof Box"
        instanceOfBox.typeFullName shouldBe "boolean"
        instanceOfBox.argument.isTypeRef.typeFullName.l shouldBe List("demo.Box")

        inside(assignmentBlock.astChildren.l) { case List(sAssignment: Call, _: Literal) =>
          sAssignment.name shouldBe Operators.assignment
          sAssignment.code shouldBe "s = ((Box) o).value()"
          sAssignment.typeFullName shouldBe "java.lang.String"
          inside(sAssignment.argument.l) { case List(sIdentifier: Identifier, valueCall: Call) =>
            sIdentifier.refsTo.l shouldBe List(patternLocal)
            valueCall.name shouldBe "value"
            valueCall.methodFullName shouldBe "demo.Box.value:java.lang.String()"
            valueCall.code shouldBe "((Box) o).value()"
            valueCall.typeFullName shouldBe "java.lang.String"
          }
        }
      }

      val List(sinkArg: Identifier) =
        useMethod.ast.collectAll[Call].nameExact("sink").argument.collectAll[Identifier].nameExact("s").l: @unchecked
      sinkArg.typeFullName shouldBe "java.lang.String"
      sinkArg.refsTo.l shouldBe List(patternLocal)
    }

    "lower record patterns with match-all and positional components" in {
      val cpg = code(
        """package demo;
          |
          |record Box(String value) {}
          |record Pair(Integer first, String second) {}
          |
          |public class RecordPatternComponents {
          |  public void matchAll(Object o) {
          |    if (o instanceof Box(_)) {
          |      sink("ok");
          |    }
          |  }
          |
          |  public void pair(Object o) {
          |    if (o instanceof Pair(_, String s)) {
          |      sink(s);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/RecordPatternComponents.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(matchAllMethod) = cpg.method.nameExact("matchAll").l: @unchecked
      matchAllMethod.body.astChildren.collectAll[Local].nameExact("s").l shouldBe Nil
      val List(matchAllCondition: Call) =
        matchAllMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      matchAllCondition.name shouldBe Operators.logicalAnd
      matchAllCondition.code shouldBe "(o instanceof Box) && { true; }"
      inside(matchAllCondition.argument.l) { case List(instanceOfBox: Call, trueBlock: Block) =>
        instanceOfBox.name shouldBe Operators.instanceOf
        instanceOfBox.code shouldBe "o instanceof Box"
        instanceOfBox.argument.isTypeRef.typeFullName.l shouldBe List("demo.Box")
        inside(trueBlock.astChildren.l) { case List(trueLiteral: Literal) =>
          trueLiteral.code shouldBe "true"
        }
      }

      val List(pairMethod)          = cpg.method.nameExact("pair").l: @unchecked
      val List(patternLocal: Local) = pairMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      patternLocal.typeFullName shouldBe "java.lang.String"
      val List(pairCondition: Call) =
        pairMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      pairCondition.name shouldBe Operators.logicalAnd
      pairCondition.code shouldBe "(o instanceof Pair) && { s = ((Pair) o).second(); true; }"
      inside(pairCondition.argument.l) { case List(instanceOfPair: Call, assignmentBlock: Block) =>
        instanceOfPair.name shouldBe Operators.instanceOf
        instanceOfPair.code shouldBe "o instanceof Pair"
        instanceOfPair.argument.isTypeRef.typeFullName.l shouldBe List("demo.Pair")

        inside(assignmentBlock.astChildren.l) { case List(sAssignment: Call, _: Literal) =>
          sAssignment.name shouldBe Operators.assignment
          sAssignment.code shouldBe "s = ((Pair) o).second()"
          inside(sAssignment.argument.l) { case List(sIdentifier: Identifier, secondCall: Call) =>
            sIdentifier.refsTo.l shouldBe List(patternLocal)
            secondCall.name shouldBe "second"
            secondCall.methodFullName shouldBe "demo.Pair.second:java.lang.String()"
            secondCall.code shouldBe "((Pair) o).second()"
            secondCall.typeFullName shouldBe "java.lang.String"
          }
        }
      }

      val List(sinkArg: Identifier) =
        pairMethod.ast.collectAll[Call].nameExact("sink").argument.collectAll[Identifier].nameExact("s").l: @unchecked
      sinkArg.refsTo.l shouldBe List(patternLocal)
    }

    "lower nested record-pattern instanceof expressions through temporary accessors" in {
      val cpg = code(
        """package demo;
          |
          |record PairBox(Pair value) {}
          |record Pair(String first, Integer second) {}
          |
          |public class NestedRecordPatternScope {
          |  public void use(Object o) {
          |    if (o instanceof PairBox(Pair(String s, Integer i))) {
          |      sink(s);
          |      sink(i);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/NestedRecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(sLocal)    = useMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      val List(tmpLocal)  = useMethod.body.astChildren.collectAll[Local].nameExact("$obj0").l: @unchecked
      val List(iLocal)    = useMethod.body.astChildren.collectAll[Local].nameExact("i").l: @unchecked
      sLocal.typeFullName shouldBe "java.lang.String"
      tmpLocal.typeFullName shouldBe "demo.Pair"
      iLocal.typeFullName shouldBe "java.lang.Integer"

      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.name shouldBe Operators.logicalAnd
      condition.code shouldBe "(o instanceof PairBox) && { s = ($obj0 = ((PairBox) o).value()).first(); i = $obj0.second(); true; }"
      inside(condition.argument.l) { case List(instanceOfPairBox: Call, assignmentBlock: Block) =>
        instanceOfPairBox.name shouldBe Operators.instanceOf
        instanceOfPairBox.code shouldBe "o instanceof PairBox"
        instanceOfPairBox.argument.isTypeRef.typeFullName.l shouldBe List("demo.PairBox")

        inside(assignmentBlock.astChildren.l) { case List(sAssignment: Call, iAssignment: Call, _: Literal) =>
          sAssignment.name shouldBe Operators.assignment
          sAssignment.code shouldBe "s = ($obj0 = ((PairBox) o).value()).first()"
          sAssignment.typeFullName shouldBe "java.lang.String"
          inside(sAssignment.argument.l) { case List(sIdentifier: Identifier, firstCall: Call) =>
            sIdentifier.refsTo.l shouldBe List(sLocal)
            firstCall.name shouldBe "first"
            firstCall.methodFullName shouldBe "demo.Pair.first:java.lang.String()"
            firstCall.code shouldBe "($obj0 = ((PairBox) o).value()).first()"
            firstCall.typeFullName shouldBe "java.lang.String"
            inside(firstCall.argument.l) { case List(tmpAssignment: Call) =>
              tmpAssignment.name shouldBe Operators.assignment
              tmpAssignment.code shouldBe "$obj0 = ((PairBox) o).value()"
              tmpAssignment.typeFullName shouldBe "demo.Pair"
              inside(tmpAssignment.argument.l) { case List(tmpIdentifier: Identifier, valueCall: Call) =>
                tmpIdentifier.refsTo.l shouldBe List(tmpLocal)
                valueCall.name shouldBe "value"
                valueCall.methodFullName shouldBe "demo.PairBox.value:demo.Pair()"
                valueCall.code shouldBe "((PairBox) o).value()"
                valueCall.typeFullName shouldBe "demo.Pair"
              }
            }
          }

          iAssignment.name shouldBe Operators.assignment
          iAssignment.code shouldBe "i = $obj0.second()"
          iAssignment.typeFullName shouldBe "java.lang.Integer"
          inside(iAssignment.argument.l) { case List(iIdentifier: Identifier, secondCall: Call) =>
            iIdentifier.refsTo.l shouldBe List(iLocal)
            secondCall.name shouldBe "second"
            secondCall.methodFullName shouldBe "demo.Pair.second:java.lang.Integer()"
            secondCall.code shouldBe "$obj0.second()"
            secondCall.typeFullName shouldBe "java.lang.Integer"
            inside(secondCall.argument.l) { case List(tmpIdentifier: Identifier) =>
              tmpIdentifier.refsTo.l shouldBe List(tmpLocal)
            }
          }
        }
      }

      val List(sSinkArg: Identifier) =
        useMethod.ast.collectAll[Call].nameExact("sink").argument.collectAll[Identifier].nameExact("s").l: @unchecked
      sSinkArg.refsTo.l shouldBe List(sLocal)
      val List(iSinkArg: Identifier) =
        useMethod.ast.collectAll[Call].nameExact("sink").argument.collectAll[Identifier].nameExact("i").l: @unchecked
      iSinkArg.refsTo.l shouldBe List(iLocal)
    }

    "lower generic record-pattern components through guarded temporary casts" in {
      val cpg = code(
        """package demo;
          |
          |record Box<T>(T value) {}
          |
          |public class GenericRecordPatternScope {
          |  public void use(Object o) {
          |    if (o instanceof Box(String s)) {
          |      sink(s);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/GenericRecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(tmpLocal)  = useMethod.body.astChildren.collectAll[Local].nameExact("$obj0").l: @unchecked
      val List(sLocal)    = useMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      tmpLocal.typeFullName shouldBe "java.lang.Object"
      sLocal.typeFullName shouldBe "java.lang.String"

      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.name shouldBe Operators.logicalAnd
      condition.code shouldBe "((o instanceof Box) && (($obj0 = ((Box) o).value()) instanceof String)) && { s = (String) $obj0; true; }"
      inside(condition.argument.l) { case List(guardAnd: Call, assignmentBlock: Block) =>
        guardAnd.code shouldBe "(o instanceof Box) && (($obj0 = ((Box) o).value()) instanceof String)"
        inside(guardAnd.argument.l) { case List(_: Call, stringCheck: Call) =>
          stringCheck.name shouldBe Operators.instanceOf
          stringCheck.code shouldBe "($obj0 = ((Box) o).value()) instanceof String"
          inside(stringCheck.argument.l) { case List(tmpAssignment: Call, stringType: TypeRef) =>
            tmpAssignment.code shouldBe "$obj0 = ((Box) o).value()"
            tmpAssignment.typeFullName shouldBe "java.lang.Object"
            stringType.typeFullName shouldBe "java.lang.String"
          }
        }
        inside(assignmentBlock.astChildren.l) { case List(sAssignment: Call, _: Literal) =>
          sAssignment.code shouldBe "s = (String) $obj0"
          inside(sAssignment.argument.l) { case List(sIdentifier: Identifier, stringCast: Call) =>
            sIdentifier.refsTo.l shouldBe List(sLocal)
            stringCast.name shouldBe Operators.cast
            stringCast.typeFullName shouldBe "java.lang.String"
          }
        }
      }
    }

    "lower generic nested record-pattern expressions with guarded nested temporaries" in {
      val cpg = code(
        """package demo;
          |
          |record Box<T>(T value) {}
          |record Pair<U, V>(U first, V second) {}
          |
          |public class GenericNestedRecordPatternScope {
          |  public void use(Object o) {
          |    if (o instanceof Box(Pair(String s, Integer i))) {
          |      sink(s);
          |      sink(i);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/GenericNestedRecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      useMethod.body.astChildren.collectAll[Local].nameExact("$obj0").l.map(_.typeFullName) shouldBe List(
        "java.lang.Object"
      )
      useMethod.body.astChildren.collectAll[Local].nameExact("$obj1").l.map(_.typeFullName) shouldBe List(
        "java.lang.Object"
      )
      useMethod.body.astChildren.collectAll[Local].nameExact("$obj2").l.map(_.typeFullName) shouldBe List(
        "java.lang.Object"
      )
      val List(sLocal) = useMethod.body.astChildren.collectAll[Local].nameExact("s").l: @unchecked
      val List(iLocal) = useMethod.body.astChildren.collectAll[Local].nameExact("i").l: @unchecked

      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.name shouldBe Operators.logicalAnd
      condition.code shouldBe "((o instanceof Box) && ((($obj0 = ((Box) o).value()) instanceof Pair) && ((($obj1 = ((Pair) $obj0).first()) instanceof String) && (($obj2 = ((Pair) $obj0).second()) instanceof Integer)))) && { s = (String) $obj1; i = (Integer) $obj2; true; }"
      inside(condition.argument.l) { case List(guardAnd: Call, assignmentBlock: Block) =>
        guardAnd.code shouldBe "(o instanceof Box) && ((($obj0 = ((Box) o).value()) instanceof Pair) && ((($obj1 = ((Pair) $obj0).first()) instanceof String) && (($obj2 = ((Pair) $obj0).second()) instanceof Integer)))"
        inside(assignmentBlock.astChildren.l) { case List(sAssignment: Call, iAssignment: Call, _: Literal) =>
          sAssignment.code shouldBe "s = (String) $obj1"
          iAssignment.code shouldBe "i = (Integer) $obj2"
          sAssignment.argument.collectAll[Identifier].head.refsTo.l shouldBe List(sLocal)
          iAssignment.argument.collectAll[Identifier].head.refsTo.l shouldBe List(iLocal)
        }
      }
    }

    "lower mixed record-pattern expressions with selective guarded temporaries" in {
      val cpg = code(
        """package demo;
          |
          |record Foo<T>(T value) {}
          |record Bar<T>(String left, T right) {}
          |
          |public class MixedRecordPatternScope {
          |  public void use(Object o) {
          |    if (o instanceof Foo(Bar(String s, Integer i))) {
          |      sink(s);
          |      sink(i);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/MixedRecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.name shouldBe Operators.logicalAnd
      condition.code shouldBe "((o instanceof Foo) && ((($obj0 = ((Foo) o).value()) instanceof Bar) && (($obj1 = ((Bar) $obj0).right()) instanceof Integer))) && { s = ((Bar) $obj0).left(); i = (Integer) $obj1; true; }"
    }

    "lower deeply mixed record-pattern expressions with reused branch temporaries" in {
      val cpg = code(
        """record A(B a0, C a1) {}
          |record B(String b0) {}
          |record C(D c0, F c1) {}
          |record D(String d0, E d1) {}
          |record E(String e0) {}
          |record F(G f0) {}
          |record G<T>(String g0, T g1) {}
          |
          |class DeepMixedRecordPatternScope {
          |  void use(Object o) {
          |    if (o instanceof A(B(String b0), C(D(String d0, E(String e0)), F(G(String g0, Integer g1))))) {
          |      sink(b0);
          |      sink(d0);
          |      sink(e0);
          |      sink(g0);
          |      sink(g1);
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "DeepMixedRecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(condition: Call) =
        useMethod.controlStructure.controlStructureType(ControlStructureTypes.IF).condition.l: @unchecked
      condition.name shouldBe Operators.logicalAnd
      condition.code shouldBe "((o instanceof A) && (($obj2 = ($obj1 = ($obj0 = ((A) o).a1()).c1().f0()).g1()) instanceof Integer)) && { b0 = ((A) o).a0().b0(); d0 = ($obj3 = $obj0.c0()).d0(); e0 = $obj3.d1().e0(); g0 = $obj1.g0(); g1 = (Integer) $obj2; true; }"
    }

    "lower switch type-pattern labels into guarded case bodies" in {
      val cpg = code(
        """package demo;
          |
          |public class SwitchTypePatternScope {
          |  public void use(Object o) {
          |    switch (o) {
          |      case String s -> sink(s);
          |      default -> {}
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/SwitchTypePatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(switchNode) = useMethod.ast
        .collectAll[ControlStructure]
        .controlStructureType(ControlStructureTypes.SWITCH)
        .l: @unchecked
      inside(switchNode.astChildren.l) { case List(selector: Identifier, switchBlock: Block) =>
        selector.code shouldBe "o"
        inside(switchBlock.astChildren.l) { case List(_: JumpTarget, entryBlock: Block, _: JumpTarget, _: Block) =>
          inside(entryBlock.astChildren.l) { case List(sLocal: Local, caseIf: ControlStructure) =>
            sLocal.name shouldBe "s"
            sLocal.typeFullName shouldBe "java.lang.String"

            val List(condition: Call) = caseIf.condition.l: @unchecked
            condition.code shouldBe "(o instanceof String) && { s = (String) o; true; }"
            val List(sinkArg: Identifier) =
              caseIf.astChildren.isBlock.astChildren
                .collectAll[Call]
                .nameExact("sink")
                .argument
                .collectAll[Identifier]
                .nameExact("s")
                .l: @unchecked
            sinkArg.refsTo.l shouldBe List(sLocal)
          }
        }
      }
    }

    "lower guarded switch type-pattern labels into nested guarded case bodies" in {
      val cpg = code(
        """package demo;
          |
          |public class SwitchGuardedTypePatternScope {
          |  public void use(Object o) {
          |    switch (o) {
          |      case String s when s.isEmpty() -> sink(s);
          |      default -> {}
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/SwitchGuardedTypePatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(sLocal)    = useMethod.ast.collectAll[Local].nameExact("s").l: @unchecked
      sLocal.typeFullName shouldBe "java.lang.String"
      inside(useMethod.ast.collectAll[ControlStructure].controlStructureType(ControlStructureTypes.IF).l) {
        case List(patternIf, guardIf) =>
          val List(patternCondition: Call) = patternIf.condition.l: @unchecked
          patternCondition.code shouldBe "(o instanceof String) && { s = (String) o; true; }"

          val List(guardCondition: Call) = guardIf.condition.l: @unchecked
          guardCondition.name shouldBe "isEmpty"
          guardCondition.code shouldBe "s.isEmpty()"
          val List(receiver: Identifier) = guardCondition.receiver.l: @unchecked
          receiver.refsTo.l shouldBe List(sLocal)

          val List(sinkArg: Identifier) =
            guardIf.ast.collectAll[Call].nameExact("sink").argument.collectAll[Identifier].nameExact("s").l: @unchecked
          sinkArg.refsTo.l shouldBe List(sLocal)
      }
    }

    "lower switch record-pattern labels into guarded case bodies" in {
      val cpg = code(
        """package demo;
          |
          |record Box<T>(T value) {}
          |
          |public class SwitchRecordPatternScope {
          |  public void use(Object o) {
          |    switch (o) {
          |      case Box(String s) -> sink(s);
          |      default -> {}
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/SwitchRecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(switchNode) = useMethod.ast
        .collectAll[ControlStructure]
        .controlStructureType(ControlStructureTypes.SWITCH)
        .l: @unchecked
      inside(switchNode.astChildren.l) { case List(selector: Identifier, switchBlock: Block) =>
        selector.code shouldBe "o"
        inside(switchBlock.astChildren.l) { case List(_: JumpTarget, entryBlock: Block, _: JumpTarget, _: Block) =>
          inside(entryBlock.astChildren.l) { case List(tmpLocal: Local, sLocal: Local, caseIf: ControlStructure) =>
            tmpLocal.name shouldBe "$obj0"
            tmpLocal.typeFullName shouldBe "java.lang.Object"
            sLocal.name shouldBe "s"
            sLocal.typeFullName shouldBe "java.lang.String"

            val List(condition: Call) = caseIf.condition.l: @unchecked
            condition.code shouldBe "((o instanceof Box) && (($obj0 = ((Box) o).value()) instanceof String)) && { s = (String) $obj0; true; }"
            val List(sinkArg: Identifier) =
              caseIf.astChildren.isBlock.astChildren
                .collectAll[Call]
                .nameExact("sink")
                .argument
                .collectAll[Identifier]
                .nameExact("s")
                .l: @unchecked
            sinkArg.refsTo.l shouldBe List(sLocal)
          }
        }
      }
    }

    "lower nested switch record-pattern labels with temporary accessors" in {
      val cpg = code(
        """package demo;
          |
          |record PairBox(Pair value) {}
          |record Pair(String first, Integer second) {}
          |
          |public class SwitchNestedRecordPatternScope {
          |  public void use(Object o) {
          |    switch (o) {
          |      case PairBox(Pair(String s, Integer i)) -> {
          |        sink(s);
          |        sink(i);
          |      }
          |      default -> {}
          |    }
          |  }
          |
          |  static void sink(Object value) {}
          |}
          |""".stripMargin,
        "demo/SwitchNestedRecordPatternScope.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(useMethod) = cpg.method.nameExact("use").l: @unchecked
      val List(switchNode) = useMethod.ast
        .collectAll[ControlStructure]
        .controlStructureType(ControlStructureTypes.SWITCH)
        .l: @unchecked
      inside(switchNode.astChildren.l) { case List(selector: Identifier, switchBlock: Block) =>
        selector.code shouldBe "o"
        inside(switchBlock.astChildren.l) { case List(_: JumpTarget, entryBlock: Block, _: JumpTarget, _: Block) =>
          inside(entryBlock.astChildren.l) {
            case List(sLocal: Local, tmpLocal: Local, iLocal: Local, caseIf: ControlStructure) =>
              sLocal.name shouldBe "s"
              sLocal.typeFullName shouldBe "java.lang.String"
              tmpLocal.name shouldBe "$obj0"
              tmpLocal.typeFullName shouldBe "demo.Pair"
              iLocal.name shouldBe "i"
              iLocal.typeFullName shouldBe "java.lang.Integer"

              val List(condition: Call) = caseIf.condition.l: @unchecked
              condition.code shouldBe "(o instanceof PairBox) && { s = ($obj0 = ((PairBox) o).value()).first(); i = $obj0.second(); true; }"
              inside(caseIf.ast.collectAll[Call].nameExact("sink").l) { case List(sSink: Call, iSink: Call) =>
                sSink.code shouldBe "sink(s)"
                val List(sSinkArg: Identifier) = sSink.argument.l: @unchecked
                sSinkArg.refsTo.l shouldBe List(sLocal)

                iSink.code shouldBe "sink(i)"
                val List(iSinkArg: Identifier) = iSink.argument.l: @unchecked
                iSinkArg.refsTo.l shouldBe List(iLocal)
              }
          }
        }
      }
    }

    "prefer array initializer calls for constant array initializers" in {
      val cpg = code(
        """package demo;
          |
          |class Arrays {
          |  void create() {
          |    int[] separated;
          |    separated = new int[] { 1, 2, 3 };
          |    int[] combined = new int[] { 4, 5 };
          |    int[] shorthand = { 6, 7 };
          |  }
          |}
          |""".stripMargin,
        "demo/Arrays.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(createMethod) = cpg.method.nameExact("create").l: @unchecked
      val initializers       = createMethod.ast.collectAll[Call].nameExact(Operators.arrayInitializer).l
      initializers.map(_.code) should contain allOf (
        "new int[] { 1, 2, 3 }",
        "new int[] { 4, 5 }",
        "{ 6, 7 }"
      )
      initializers.map(_.typeFullName) shouldBe List("int[]", "int[]", "int[]")
      createMethod.ast.collectAll[Call].nameExact(Operators.alloc).code.l should not contain "new int[] { 1, 2, 3 }"

      val List(separatedInitializer: Call) =
        initializers.filter(_.code == "new int[] { 1, 2, 3 }"): @unchecked
      separatedInitializer.argument.code.l shouldBe List("1", "2", "3")

      val List(shorthandInitializer: Call) = initializers.filter(_.code == "{ 6, 7 }"): @unchecked
      shorthandInitializer.argument.code.l shouldBe List("6", "7")
    }

    "create method-body anonymous class type declarations and constructor calls" in {
      val cpg = code(
        """package demo;
          |
          |interface Bar {
          |  void bar();
          |}
          |
          |class Foo {
          |  void foo() {
          |    Bar b = new Bar() {
          |      public void bar() {
          |        sink("BAR");
          |      }
          |    };
          |    b.bar();
          |  }
          |}
          |""".stripMargin,
        "demo/Anonymous.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val anonymousFullName = "demo.Foo.foo.Bar$0"
      cpg.typeDecl.nameExact("Foo").ast.collectAll[TypeDecl].fullNameExact(anonymousFullName).size shouldBe 1

      val List(anonymousDecl) = cpg.typeDecl.fullNameExact(anonymousFullName).l: @unchecked
      anonymousDecl.name shouldBe "Bar$0"
      anonymousDecl.inheritsFromTypeFullName shouldBe List("demo.Bar")

      inside(cpg.all.collectAll[Binding].nameExact("bar").sortBy(_.methodFullName).l) {
        case List(interfaceBinding, anonymousBinding) =>
          interfaceBinding.methodFullName shouldBe "demo.Bar.bar:void()"
          interfaceBinding.bindingTypeDecl.fullName shouldBe "demo.Bar"
          anonymousBinding.methodFullName shouldBe s"$anonymousFullName.bar:void()"
          anonymousBinding.bindingTypeDecl.fullName shouldBe anonymousFullName
      }

      val List(anonymousMethod) = anonymousDecl.method.nameExact("bar").l: @unchecked
      anonymousMethod.fullName shouldBe s"$anonymousFullName.bar:void()"
      anonymousMethod.parameter.name.l shouldBe List("this")

      val List(anonymousCtor) = anonymousDecl.method.nameExact("<init>").l: @unchecked
      anonymousCtor.fullName shouldBe s"$anonymousFullName.<init>:void()"
      anonymousCtor.parameter.name.l shouldBe List("this", "outerClass")
      anonymousCtor.parameter.typeFullName.l shouldBe List(anonymousFullName, "demo.Foo")
      anonymousCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
        "this.outerClass = outerClass"
      )

      val List(initCall) = cpg.method.fullNameExact("demo.Foo.foo:void()").call.nameExact("<init>").l: @unchecked
      initCall.methodFullName shouldBe s"$anonymousFullName.<init>:void()"
      inside(initCall.argument.l) { case List(receiver: Identifier, outerThis: Identifier) =>
        receiver.name shouldBe "b"
        receiver.typeFullName shouldBe "demo.Bar"
        outerThis.name shouldBe "this"
        outerThis.typeFullName shouldBe "demo.Foo"
        outerThis.refsTo.l shouldBe cpg.method.fullNameExact("demo.Foo.foo:void()").parameter.nameExact("this").l
      }
    }

    "create field anonymous class type declarations and constructor calls" in {
      val cpg = code(
        """package demo;
          |
          |interface FieldBar {
          |  void bar();
          |}
          |
          |class InstanceAnon {
          |  FieldBar b = new FieldBar() {
          |    public void bar() {
          |      sink("instance");
          |    }
          |  };
          |}
          |
          |class StaticAnon {
          |  static FieldBar b = new FieldBar() {
          |    public void bar() {
          |      sink("static");
          |    }
          |  };
          |}
          |""".stripMargin,
        "demo/FieldAnonymous.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val instanceAnonymousFullName = "demo.InstanceAnon.b.FieldBar$0"
      val staticAnonymousFullName   = "demo.StaticAnon.b.FieldBar$0"

      cpg.typeDecl
        .nameExact("InstanceAnon")
        .ast
        .collectAll[TypeDecl]
        .fullNameExact(instanceAnonymousFullName)
        .size shouldBe 1
      cpg.typeDecl
        .nameExact("StaticAnon")
        .ast
        .collectAll[TypeDecl]
        .fullNameExact(staticAnonymousFullName)
        .size shouldBe 1
      cpg.typeDecl.fullNameExact(instanceAnonymousFullName).inheritsFromTypeFullName.l shouldBe List("demo.FieldBar")
      cpg.typeDecl.fullNameExact(staticAnonymousFullName).inheritsFromTypeFullName.l shouldBe List("demo.FieldBar")

      val List(instanceInitCall) =
        cpg.typeDecl.nameExact("InstanceAnon").method.nameExact("<init>").call.nameExact("<init>").l: @unchecked
      instanceInitCall.methodFullName shouldBe s"$instanceAnonymousFullName.<init>:void()"
      inside(instanceInitCall.argument.l) { case List(fieldAccess: Call, outerThis: Identifier) =>
        fieldAccess.name shouldBe Operators.fieldAccess
        fieldAccess.typeFullName shouldBe "demo.FieldBar"
        inside(fieldAccess.argument.l) { case List(thisNode: Identifier, bField: FieldIdentifier) =>
          thisNode.name shouldBe "this"
          thisNode.typeFullName shouldBe "demo.InstanceAnon"
          bField.canonicalName shouldBe "b"
        }
        outerThis.name shouldBe "this"
        outerThis.typeFullName shouldBe "demo.InstanceAnon"
        outerThis.refsTo.l shouldBe cpg.typeDecl
          .nameExact("InstanceAnon")
          .method
          .nameExact("<init>")
          .parameter
          .nameExact("this")
          .l
      }

      val List(staticInitCall) =
        cpg.typeDecl.nameExact("StaticAnon").method.nameExact("<clinit>").call.nameExact("<init>").l: @unchecked
      staticInitCall.methodFullName shouldBe s"$staticAnonymousFullName.<init>:void()"
      inside(staticInitCall.argument.l) { case List(fieldAccess: Call) =>
        fieldAccess.name shouldBe Operators.fieldAccess
        fieldAccess.typeFullName shouldBe "demo.FieldBar"
        inside(fieldAccess.argument.l) { case List(typeRef: TypeRef, bField: FieldIdentifier) =>
          typeRef.typeFullName shouldBe "demo.StaticAnon"
          bField.canonicalName shouldBe "b"
        }
      }
    }

    "resolve inherited methods and members in anonymous class bodies" in {
      val cpg = code(
        """package demo;
          |
          |abstract class Base {
          |  int value = 0;
          |  void sink(int input) {}
          |  abstract void run();
          |}
          |
          |class Holder {
          |  static Base b = new Base() {
          |    public void run() {
          |      sink(value);
          |    }
          |  };
          |}
          |""".stripMargin,
        "demo/AnonymousInheritance.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val anonymousFullName = "demo.Holder.b.Base$0"
      val List(sinkCall) =
        cpg.method.fullNameExact(s"$anonymousFullName.run:void()").call.nameExact("sink").l: @unchecked
      sinkCall.methodFullName shouldBe s"$anonymousFullName.sink:void(int)"
      sinkCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
      inside(sinkCall.argument.l) { case List(thisArgument: Identifier, valueAccess: Call) =>
        thisArgument.name shouldBe "this"
        thisArgument.typeFullName shouldBe anonymousFullName

        valueAccess.name shouldBe Operators.fieldAccess
        valueAccess.code shouldBe "this.value"
        valueAccess.typeFullName shouldBe "int"
        inside(valueAccess.argument.l) { case List(valueThis: Identifier, valueField: FieldIdentifier) =>
          valueThis.name shouldBe "this"
          valueThis.typeFullName shouldBe anonymousFullName
          valueField.canonicalName shouldBe "value"
        }
      }
    }

    "create lambda-body anonymous class type declarations under lambda owners" in {
      val cpg = code(
        """package demo;
          |
          |interface Action {
          |  void run();
          |}
          |
          |interface FirstTask {
          |  void doFirst(Action action);
          |}
          |
          |interface SecondTask {
          |  void doSecond(Action action);
          |}
          |
          |interface FirstProvider {
          |  void provide(FirstTask firstTask);
          |}
          |
          |interface SecondProvider {
          |  void provide(SecondTask secondTask);
          |}
          |
          |class LambdaAnon {
          |  static FirstProvider method1() {
          |    return firstTask -> {
          |      firstTask.doFirst(new Action() {
          |        public void run() {}
          |      });
          |    };
          |  }
          |
          |  SecondProvider method2() {
          |    return secondTask -> {
          |      secondTask.doSecond(new Action() {
          |        public void run() {}
          |      });
          |    };
          |  }
          |}
          |""".stripMargin,
        "demo/LambdaAnonymous.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      cpg.typeDecl.nameExact("Action$0").fullName.sorted.l shouldBe List(
        "demo.LambdaAnon.<lambda>0.Action$0",
        "demo.LambdaAnon.<lambda>1.Action$0"
      )
      cpg.typeDecl.fullNameExact("demo.LambdaAnon.<lambda>0.Action$0").inheritsFromTypeFullName.l shouldBe List(
        "demo.Action"
      )
      cpg.typeDecl.fullNameExact("demo.LambdaAnon.<lambda>1.Action$0").inheritsFromTypeFullName.l shouldBe List(
        "demo.Action"
      )
      val List(instanceLambda) =
        cpg.method.fullNameExact("demo.LambdaAnon.<lambda>1:void(demo.SecondTask)").l: @unchecked
      val List(thisCaptureLocal) = instanceLambda.local.nameExact("this").l: @unchecked
      thisCaptureLocal.closureBindingId shouldBe Some("demo/LambdaAnonymous.java:<lambda>1:this")

      val List(instanceLambdaInit) = instanceLambda.call
        .nameExact("<init>")
        .methodFullNameExact("demo.LambdaAnon.<lambda>1.Action$0.<init>:void()")
        .l: @unchecked
      val List(outerThisArg) = instanceLambdaInit.argument.collectAll[Identifier].nameExact("this").l: @unchecked
      outerThisArg.refsTo.l shouldBe List(thisCaptureLocal)

      cpg.local.filter(_._astIn.isEmpty).l shouldBe Nil
    }

    "resolve captured receivers in lambdas inside anonymous classes" in {
      val cpg = code(
        """package demo;
          |
          |import java.util.List;
          |
          |interface Bar {
          |  void remove(Object value);
          |}
          |
          |interface Visitor {
          |  void visit(Visited visited);
          |}
          |
          |interface Visited {
          |  List<Object> getList();
          |}
          |
          |public class Foo {
          |  public static Object test(Bar captured) {
          |    Visitor v = new Visitor() {
          |      public void visit(Visited visited) {
          |        visited.getList().forEach(lambdaParam -> captured.remove(lambdaParam));
          |      }
          |    };
          |    return v;
          |  }
          |}
          |""".stripMargin,
        "demo/AnonymousLambdaCapture.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val anonymousFullName = "demo.Foo.test.Visitor$0"
      cpg.typeDecl.fullNameExact(anonymousFullName).member.nameExact("captured").typeFullName.l shouldBe List(
        "demo.Bar"
      )

      val List(lambdaMethod) = cpg.typeDecl.fullNameExact(anonymousFullName).method.nameExact("<lambda>0").l: @unchecked
      val List(thisLocal)    = lambdaMethod.local.nameExact("this").l: @unchecked
      thisLocal.typeFullName shouldBe anonymousFullName
      thisLocal.closureBindingId shouldBe Some("demo/AnonymousLambdaCapture.java:<lambda>0:this")

      val List(thisClosureBinding) =
        cpg.closureBinding.l.filter(
          _.closureBindingId.contains("demo/AnonymousLambdaCapture.java:<lambda>0:this")
        ): @unchecked
      thisClosureBinding._refOut.l shouldBe cpg.method
        .fullNameExact(s"$anonymousFullName.visit:void(demo.Visited)")
        .parameter
        .nameExact("this")
        .l
      thisClosureBinding._captureIn.l shouldBe cpg.methodRef.methodFullNameExact(lambdaMethod.fullName).l

      val List(removeCall) = lambdaMethod.call.nameExact("remove").l: @unchecked
      removeCall.methodFullName shouldBe "demo.Bar.remove:void(java.lang.Object)"
      removeCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
      inside(removeCall.receiver.l) { case List(capturedReceiver: Call) =>
        capturedReceiver.name shouldBe Operators.fieldAccess
        capturedReceiver.code shouldBe "this.captured"
        capturedReceiver.typeFullName shouldBe "demo.Bar"

        inside(capturedReceiver.argument.l) { case List(thisIdentifier: Identifier, capturedField: FieldIdentifier) =>
          thisIdentifier.name shouldBe "this"
          thisIdentifier.typeFullName shouldBe anonymousFullName
          thisIdentifier.refsTo.l shouldBe List(thisLocal)
          capturedField.canonicalName shouldBe "captured"
        }
      }
    }

    "use anonymous constructed types for assignment expressions" in {
      val cpg = code(
        """package demo;
          |
          |interface ReassignedBar {
          |  void bar();
          |}
          |
          |class ReassignedAnon {
          |  void foo() {
          |    ReassignedBar b;
          |    b = new ReassignedBar() {
          |      public void bar() {
          |        sink("assigned");
          |      }
          |    };
          |  }
          |}
          |""".stripMargin,
        "demo/ReassignedAnonymous.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val anonymousFullName = "demo.ReassignedAnon.foo.ReassignedBar$0"
      cpg.typeDecl.fullNameExact(anonymousFullName).inheritsFromTypeFullName.l shouldBe List("demo.ReassignedBar")

      val List(assignment) = cpg.method
        .fullNameExact("demo.ReassignedAnon.foo:void()")
        .call
        .nameExact(Operators.assignment)
        .codeExact("""b = new ReassignedBar() {
      public void bar() {
        sink("assigned");
      }
    }""")
        .l: @unchecked
      inside(assignment.argument.l) { case List(target: Identifier, alloc: Call) =>
        target.name shouldBe "b"
        target.typeFullName shouldBe "demo.ReassignedBar"
        alloc.name shouldBe Operators.alloc
        alloc.typeFullName shouldBe anonymousFullName
      }

      val List(initCall) =
        cpg.method.fullNameExact("demo.ReassignedAnon.foo:void()").call.nameExact("<init>").l: @unchecked
      initCall.methodFullName shouldBe s"$anonymousFullName.<init>:void()"
      inside(initCall.argument.l) { case List(receiver: Identifier, outerThis: Identifier) =>
        receiver.name shouldBe "b"
        receiver.typeFullName shouldBe "demo.ReassignedBar"
        outerThis.name shouldBe "this"
        outerThis.typeFullName shouldBe "demo.ReassignedAnon"
      }
    }

    "create anonymous constructors for superclass constructor arguments" in {
      val cpg = code(
        """package demo;
          |
          |abstract class ConstructedBase {
          |  ConstructedBase(int seed) {}
          |  abstract int value();
          |}
          |
          |class ConstructorAnon {
          |  void foo(int seed) {
          |    ConstructedBase b = new ConstructedBase(seed) {
          |      public int value() {
          |        return 1;
          |      }
          |    };
          |  }
          |}
          |""".stripMargin,
        "demo/ConstructorAnonymous.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val anonymousFullName   = "demo.ConstructorAnon.foo.ConstructedBase$0"
      val List(anonymousCtor) = cpg.typeDecl.fullNameExact(anonymousFullName).method.nameExact("<init>").l: @unchecked
      anonymousCtor.fullName shouldBe s"$anonymousFullName.<init>:void(int)"
      anonymousCtor.signature shouldBe "void(int)"
      anonymousCtor.parameter.name.l shouldBe List("this", "arg0", "outerClass")
      anonymousCtor.parameter.typeFullName.l shouldBe List(anonymousFullName, "int", "demo.ConstructorAnon")

      val List(superInitCall) =
        anonymousCtor.ast.collectAll[Call].nameExact("<init>").codeExact("super(arg0)").l: @unchecked
      superInitCall.methodFullName shouldBe "demo.ConstructedBase.<init>:void(int)"
      superInitCall.signature shouldBe "void(int)"
      superInitCall.argument.code.l shouldBe List("this", "arg0")
      superInitCall.argument.isIdentifier.nameExact("arg0").refsTo.l shouldBe anonymousCtor.parameter
        .nameExact("arg0")
        .l

      val List(initCall) = cpg.method
        .fullNameExact("demo.ConstructorAnon.foo:void(int)")
        .call
        .nameExact("<init>")
        .methodFullNameExact(s"$anonymousFullName.<init>:void(int)")
        .l: @unchecked
      initCall.signature shouldBe "void(int)"
      initCall.argument.code.l shouldBe List("b", "seed", "this")
      initCall.argument.isIdentifier.nameExact("seed").refsTo.l shouldBe cpg.method
        .fullNameExact("demo.ConstructorAnon.foo:void(int)")
        .parameter
        .nameExact("seed")
        .l
    }

    "create anonymous constructors for local-class methods with superclass args and captures" in {
      val cpg = code(
        """package demo;
          |
          |class Bar {
          |  int barMember;
          |  public Bar(int barParam) {
          |    barMember = barParam;
          |  }
          |}
          |
          |class OuterClass {
          |  void outerMethod(int outerParam) {
          |    class InnerClass {
          |      void innerMethod(int innerParam) {
          |        Bar b = new Bar(innerParam) {
          |          int bar() {
          |            return barMember + innerParam + outerParam;
          |          }
          |        };
          |      }
          |    }
          |  }
          |}
          |""".stripMargin,
        "demo/LocalMethodAnonymous.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val innerFullName     = "demo.OuterClass.outerMethod:void(int).InnerClass"
      val innerMethodName   = s"$innerFullName.innerMethod:void(int)"
      val anonymousFullName = s"$innerFullName.innerMethod.Bar$$0"

      cpg.typeDecl.fullNameExact(innerFullName).member.name.l should contain("outerParam")

      val List(anonymousDecl) = cpg.typeDecl.fullNameExact(anonymousFullName).l: @unchecked
      anonymousDecl.inheritsFromTypeFullName.l shouldBe List("demo.Bar")
      anonymousDecl.member.name.l should contain allOf ("outerClass", "innerParam")
      anonymousDecl.member.nameExact("outerClass").typeFullName.l shouldBe List(innerFullName)
      anonymousDecl.member.nameExact("innerParam").typeFullName.l shouldBe List("int")

      val List(anonymousCtor) = anonymousDecl.method.nameExact("<init>").l: @unchecked
      anonymousCtor.fullName shouldBe s"$anonymousFullName.<init>:void(int)"
      anonymousCtor.parameter.name.l shouldBe List("this", "arg0", "outerClass", "innerParam")
      anonymousCtor.parameter.typeFullName.l shouldBe List(anonymousFullName, "int", innerFullName, "int")

      val List(superInitCall) =
        anonymousCtor.ast.collectAll[Call].nameExact("<init>").codeExact("super(arg0)").l: @unchecked
      superInitCall.methodFullName shouldBe "demo.Bar.<init>:void(int)"
      superInitCall.argument.code.l shouldBe List("this", "arg0")
      anonymousCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
        "this.outerClass = outerClass",
        "this.innerParam = innerParam"
      )

      val List(initCall) = cpg.method
        .fullNameExact(innerMethodName)
        .call
        .nameExact("<init>")
        .methodFullNameExact(s"$anonymousFullName.<init>:void(int)")
        .l: @unchecked
      initCall.argument.code.l shouldBe List("b", "innerParam", "this", "innerParam")

      cpg.method
        .fullNameExact(s"$anonymousFullName.bar:int()")
        .call
        .nameExact(Operators.fieldAccess)
        .code
        .l should contain allOf ("this.barMember", "this.innerParam", "this.outerClass.outerParam")
    }

    "capture visible locals used in anonymous class bodies" in {
      val cpg = code(
        """package demo;
          |
          |interface CapturingAction {
          |  int value();
          |}
          |
          |class CapturingAnon {
          |  void foo(int seed) {
          |    int local = seed + 1;
          |    CapturingAction action = new CapturingAction() {
          |      public int value() {
          |        return local;
          |      }
          |    };
          |  }
          |}
          |""".stripMargin,
        "demo/CapturingAnonymous.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val anonymousFullName = "demo.CapturingAnon.foo.CapturingAction$0"
      cpg.typeDecl.fullNameExact(anonymousFullName).member.name.l should contain allOf ("outerClass", "local")

      val List(anonymousCtor) = cpg.typeDecl.fullNameExact(anonymousFullName).method.nameExact("<init>").l: @unchecked
      anonymousCtor.fullName shouldBe s"$anonymousFullName.<init>:void()"
      anonymousCtor.parameter.name.l shouldBe List("this", "outerClass", "local")
      anonymousCtor.parameter.typeFullName.l shouldBe List(anonymousFullName, "demo.CapturingAnon", "int")
      anonymousCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
        "this.outerClass = outerClass",
        "this.local = local"
      )

      val List(initCall) =
        cpg.method.fullNameExact("demo.CapturingAnon.foo:void(int)").call.nameExact("<init>").l: @unchecked
      initCall.methodFullName shouldBe s"$anonymousFullName.<init>:void()"
      initCall.argument.code.l shouldBe List("action", "this", "local")
      initCall.argument.isIdentifier.nameExact("local").refsTo.l shouldBe cpg.method
        .fullNameExact("demo.CapturingAnon.foo:void(int)")
        .local
        .nameExact("local")
        .l

      val List(returnLocal: Call) = cpg.method
        .fullNameExact(s"$anonymousFullName.value:int()")
        .ast
        .collectAll[Return]
        .astChildren
        .collectAll[Call]
        .nameExact(Operators.fieldAccess)
        .l: @unchecked
      returnLocal.code shouldBe "this.local"
      returnLocal.typeFullName shouldBe "int"
      inside(returnLocal.argument.l) { case List(thisReceiver: Identifier, localField: FieldIdentifier) =>
        thisReceiver.name shouldBe "this"
        thisReceiver.typeFullName shouldBe anonymousFullName
        localField.canonicalName shouldBe "local"
      }
    }

    "resolve lambda receivers through anonymous class capture fields" in {
      val cpg = code(
        """package demo;
          |
          |import java.util.function.Consumer;
          |
          |interface Visitor {
          |  void visit(Visited visited);
          |}
          |
          |interface Visited {
          |  ListLike getList();
          |}
          |
          |interface ListLike {
          |  void forEach(Consumer<String> consumer);
          |}
          |
          |class Bar {
          |  void remove(String value) {}
          |}
          |
          |class Foo {
          |  static Object test(Bar captured) {
          |    Visitor v = new Visitor() {
          |      public void visit(Visited visited) {
          |        visited.getList().forEach(lambdaParam -> captured.remove(lambdaParam));
          |      }
          |    };
          |    return v;
          |  }
          |}
          |""".stripMargin,
        "demo/AnonymousLambdaCapture.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val ownerFullName     = "demo.Foo.test:java.lang.Object(demo.Bar)"
      val anonymousFullName = "demo.Foo.test.Visitor$0"
      val visitFullName     = s"$anonymousFullName.visit:void(demo.Visited)"
      val lambdaFullName    = s"$anonymousFullName.<lambda>0:void(java.lang.String)"

      cpg.typeDecl.fullNameExact(anonymousFullName).member.name.l should contain("captured")
      cpg.typeDecl.fullNameExact(anonymousFullName).member.nameExact("captured").typeFullName.l shouldBe List(
        "demo.Bar"
      )

      val List(anonymousCtor) = cpg.typeDecl.fullNameExact(anonymousFullName).method.nameExact("<init>").l: @unchecked
      anonymousCtor.parameter.name.l shouldBe List("this", "captured")
      anonymousCtor.parameter.typeFullName.l shouldBe List(anonymousFullName, "demo.Bar")
      anonymousCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
        "this.captured = captured"
      )

      val List(initCall) = cpg.method
        .fullNameExact(ownerFullName)
        .call
        .nameExact("<init>")
        .methodFullNameExact(s"$anonymousFullName.<init>:void()")
        .l: @unchecked
      initCall.methodFullName shouldBe s"$anonymousFullName.<init>:void()"
      initCall.argument.code.l shouldBe List("v", "captured")

      val List(lambdaMethod) = cpg.method.fullNameExact(lambdaFullName).l: @unchecked
      lambdaMethod.parameter.name.l shouldBe List("this", "lambdaParam")
      lambdaMethod.parameter.typeFullName.l shouldBe List(anonymousFullName, "java.lang.String")
      lambdaMethod.local.nameExact("this").typeFullName.l shouldBe List(anonymousFullName)
      lambdaMethod.local.nameExact("this").closureBindingId.l shouldBe List(
        "demo/AnonymousLambdaCapture.java:<lambda>0:this"
      )

      val List(thisClosureBinding) = cpg.closureBinding.l: @unchecked
      thisClosureBinding.closureBindingId shouldBe Some("demo/AnonymousLambdaCapture.java:<lambda>0:this")
      thisClosureBinding._refOut.l shouldBe cpg.method.fullNameExact(visitFullName).parameter.nameExact("this").l
      thisClosureBinding._captureIn.l shouldBe cpg.methodRef.methodFullNameExact(lambdaFullName).l

      val List(removeCall) = lambdaMethod.call.nameExact("remove").l: @unchecked
      removeCall.methodFullName shouldBe "demo.Bar.remove:void(java.lang.String)"
      inside(removeCall.receiver.l) { case List(capturedReceiver: Call) =>
        capturedReceiver.name shouldBe Operators.fieldAccess
        capturedReceiver.code shouldBe "this.captured"
        capturedReceiver.typeFullName shouldBe "demo.Bar"

        inside(capturedReceiver.argument.l) { case List(thisIdentifier: Identifier, capturedField: FieldIdentifier) =>
          thisIdentifier.name shouldBe "this"
          thisIdentifier.typeFullName shouldBe anonymousFullName
          thisIdentifier.refsTo.l shouldBe lambdaMethod.local.nameExact("this").l
          capturedField.canonicalName shouldBe "captured"
        }
      }
    }

    "create members and initializer assignments for multi-variable field declarations" in {
      val cpg = code(
        """package demo;
          |
          |class MultiFields {
          |  int first, second = 2;
          |  static String name, alias = "fallback";
          |  String text, values[];
          |}
          |""".stripMargin,
        "demo/MultiFields.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(multiFields) = cpg.typeDecl.nameExact("MultiFields").l: @unchecked
      inside(multiFields.member.l) { case List(first, second, name, alias, text, values) =>
        first.name shouldBe "first"
        first.code shouldBe "int first"
        first.typeFullName shouldBe "int"

        second.name shouldBe "second"
        second.code shouldBe "int second"
        second.typeFullName shouldBe "int"

        name.name shouldBe "name"
        name.code shouldBe "String name"
        name.typeFullName shouldBe "java.lang.String"
        name.modifier.modifierType.l should contain(ModifierTypes.STATIC)

        alias.name shouldBe "alias"
        alias.code shouldBe "String alias"
        alias.typeFullName shouldBe "java.lang.String"
        alias.modifier.modifierType.l should contain(ModifierTypes.STATIC)

        text.name shouldBe "text"
        text.code shouldBe "String text"
        text.typeFullName shouldBe "java.lang.String"

        values.name shouldBe "values"
        values.code shouldBe "String[] values"
        values.typeFullName shouldBe "java.lang.String[]"
      }

      val List(ctor) = multiFields.method.nameExact("<init>").l: @unchecked
      ctor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List("this.second = 2")

      val List(clinit) = multiFields.method.nameExact("<clinit>").l: @unchecked
      clinit.parameter.l shouldBe Nil
      val List(aliasAssign) = clinit.body.astChildren.collectAll[Call].nameExact(Operators.assignment).l: @unchecked
      aliasAssign.code shouldBe "MultiFields.alias = \"fallback\""
      inside(aliasAssign.argument.l) { case List(fieldAccess: Call, value: Literal) =>
        fieldAccess.code shouldBe "MultiFields.alias"
        fieldAccess.typeFullName shouldBe "java.lang.String"
        value.code shouldBe "\"fallback\""
        value.typeFullName shouldBe "java.lang.String"
      }
    }

    "create control-flow ASTs for loops, switch, try, synchronized, jumps, and throw" in {
      val cpg = code(
        """package demo;
          |
          |public class Flow {
          |  public int flow(int[] values) {
          |    int total = 0;
          |    for (int i = 0; i < values.length; i++) {
          |      if (values[i] == 0) {
          |        continue;
          |      }
          |      total += values[i];
          |    }
          |    for (int value : values) {
          |      total += value;
          |    }
          |    outer:
          |    for (int value : values) {
          |      if (value < 0) {
          |        break outer;
          |      }
          |    }
          |    while (total < 10) {
          |      total++;
          |    }
          |    do {
          |      total--;
          |    } while (total > 0);
          |    switch (total) {
          |      case 0:
          |        break;
          |      default:
          |        total = 1;
          |    }
          |    try {
          |      throw new RuntimeException();
          |    } catch (RuntimeException ex) {
          |      total = -1;
          |    } finally {
          |      total = total + 1;
          |    }
          |    synchronized (this) {
          |      total += 2;
          |    }
          |    assert total >= 0;
          |    assert total > 0 : "positive";
          |    return total;
          |  }
          |}
          |""".stripMargin,
        "demo/Flow.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(flowMethod) = cpg.method.nameExact("flow").l: @unchecked
      def flowAst          = flowMethod.ast
      def flowCalls        = flowAst.collectAll[Call]
      val controlTypes     = flowAst.collectAll[ControlStructure].controlStructureType.l

      controlTypes should contain allOf (
        ControlStructureTypes.FOR,
        ControlStructureTypes.WHILE,
        ControlStructureTypes.DO,
        ControlStructureTypes.SWITCH,
        ControlStructureTypes.TRY,
        ControlStructureTypes.CATCH,
        ControlStructureTypes.FINALLY,
        ControlStructureTypes.BREAK,
        ControlStructureTypes.CONTINUE
      )
      flowAst.collectAll[Local].nameExact("value").typeFullName.l should contain("int")
      val List(outerLabel: JumpTarget) = flowAst.collectAll[JumpTarget].nameExact("outer").l: @unchecked
      outerLabel.code shouldBe "outer"
      flowAst
        .collectAll[ControlStructure]
        .controlStructureType(ControlStructureTypes.BREAK)
        .code
        .l should contain allOf (
        "break;",
        "break outer;"
      )
      flowCalls.nameExact(Operators.postIncrement).code.l should contain("i++")
      flowCalls.nameExact(Operators.postDecrement).code.l should contain("total--")
      flowCalls.nameExact(Operators.assignmentPlus).code.l should contain allOf ("total += values[i]", "total += value")
      flowCalls.nameExact("<operator>.throw").code.l should contain("throw new RuntimeException();")
      val List(syncBlock: Block) =
        flowAst.isBlock.where(_.astChildren.isModifier.modifierType("SYNCHRONIZED")).l: @unchecked
      inside(syncBlock.astChildren.l) { case List(modifier: Modifier, lock: Identifier, body: Block) =>
        modifier.modifierType shouldBe "SYNCHRONIZED"
        lock.code shouldBe "this"
        body.astChildren.collectAll[Call].nameExact(Operators.assignmentPlus).code.l shouldBe List("total += 2")
      }
      inside(flowCalls.nameExact("assert").l) { case List(nonNegative, positive) =>
        nonNegative.code shouldBe "assert total >= 0;"
        nonNegative.methodFullName shouldBe "assert"
        nonNegative.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        nonNegative.argument.code.l shouldBe List("total >= 0")

        positive.code shouldBe """assert total > 0 : "positive";"""
        positive.methodFullName shouldBe "assert"
        positive.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        positive.argument.code.l shouldBe List("total > 0")
      }
      flowAst.collectAll[ControlStructure].controlStructureType(ControlStructureTypes.SWITCH).code.l should contain(
        "switch(total)"
      )
    }

    "create try-with-resources ASTs and scope resource locals" in {
      val cpg = code(
        """package demo;
          |
          |import java.io.BufferedReader;
          |import java.io.FileReader;
          |import java.io.IOException;
          |
          |class Resources {
          |  static String read(String path) throws IOException {
          |    try (FileReader fr = new FileReader(path);
          |         BufferedReader br = new BufferedReader(fr)) {
          |      return br.readLine();
          |    }
          |  }
          |}
          |""".stripMargin,
        "demo/Resources.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(readMethod) = cpg.method.nameExact("read").l: @unchecked
      inside(readMethod.body.astChildren.l) {
        case List(
              frLocal: Local,
              frAssign: Call,
              frInit: Call,
              brLocal: Local,
              brAssign: Call,
              brInit: Call,
              tryBlock: ControlStructure
            ) =>
          frLocal.name shouldBe "fr"
          frLocal.code shouldBe "FileReader fr"
          frLocal.typeFullName shouldBe "java.io.FileReader"

          frAssign.name shouldBe Operators.assignment
          val List(frAssignLhs: Identifier, frAssignRhs: Call) = frAssign.argument.l: @unchecked
          frAssignLhs.name shouldBe "fr"
          frAssignLhs.refsTo.l shouldBe List(frLocal)
          frAssignRhs.name shouldBe Operators.alloc
          frAssignRhs.typeFullName shouldBe "java.io.FileReader"

          frInit.name shouldBe io.joern.x2cpg.Defines.ConstructorMethodName
          val List(frInitThis: Identifier, frInitArg: Identifier) = frInit.argument.l: @unchecked
          frInitThis.name shouldBe "fr"
          frInitThis.refsTo.l shouldBe List(frLocal)
          frInitArg.name shouldBe "path"

          brLocal.name shouldBe "br"
          brLocal.code shouldBe "BufferedReader br"
          brLocal.typeFullName shouldBe "java.io.BufferedReader"

          brAssign.name shouldBe Operators.assignment
          val List(brAssignLhs: Identifier, brAssignRhs: Call) = brAssign.argument.l: @unchecked
          brAssignLhs.name shouldBe "br"
          brAssignLhs.refsTo.l shouldBe List(brLocal)
          brAssignRhs.name shouldBe Operators.alloc
          brAssignRhs.typeFullName shouldBe "java.io.BufferedReader"

          brInit.name shouldBe io.joern.x2cpg.Defines.ConstructorMethodName
          val List(brInitThis: Identifier, brInitArg: Identifier) = brInit.argument.l: @unchecked
          brInitThis.name shouldBe "br"
          brInitThis.refsTo.l shouldBe List(brLocal)
          brInitArg.name shouldBe "fr"
          brInitArg.refsTo.l shouldBe List(frLocal)

          tryBlock.controlStructureType shouldBe ControlStructureTypes.TRY
          inside(tryBlock.astChildren.l) { case List(block: Block) =>
            val List(returnStmt: Return) = block.astChildren.l: @unchecked
            returnStmt.code shouldBe "return br.readLine();"
            val List(readLineCall: Call)   = returnStmt.astChildren.collectAll[Call].nameExact("readLine").l: @unchecked
            val List(receiver: Identifier) = readLineCall.argument.isIdentifier.nameExact("br").l: @unchecked
            receiver.refsTo.l shouldBe List(brLocal)
          }
      }
    }

    "create expression ASTs for ternary, casts, instanceof, class literals, constructor invocations, and method refs" in {
      val cpg = code(
        """package demo;
          |
          |import java.util.Base64;
          |import java.util.function.Function;
          |import java.util.function.Supplier;
          |
          |class Widget {
          |  Widget() {}
          |}
          |
          |class BaseExprs {
          |  BaseExprs(int seed) {}
          |}
          |
          |public class Exprs extends BaseExprs {
          |  public Exprs() {
          |    this(1);
          |  }
          |
          |  public Exprs(int seed) {
          |    super(seed);
          |  }
          |
          |  public int pick(Object input, int fallback) {
          |    int y = input instanceof String ? ((String) input).length() : fallback;
          |    int inverted = ~fallback;
          |    int shifted = (fallback >> 1) + (fallback >>> 2);
          |    String cleaned = ((String) input).trim();
          |    boolean blank = cleaned.isEmpty();
          |    String stripped = cleaned.strip();
          |    boolean blankAgain = stripped.isBlank();
          |    String boolText = String.valueOf(blank);
          |    Function<String, String> trim = String::trim;
          |    Function<Integer, Integer> local = this::inc;
          |    Function<Integer, Integer> staticRef = Exprs::doubleIt;
          |    Supplier<Widget> maker = Widget::new;
          |    return y + shifted;
          |  }
          |
          |  public byte[] decode(Base64.Decoder decoder, String src) {
          |    Base64.Decoder localDecoder = Base64.getDecoder();
          |    byte[] direct = decoder.decode(src);
          |    return localDecoder.decode(src);
          |  }
          |
          |  public int inc(int value) {
          |    return value + 1;
          |  }
          |
          |  public static int doubleIt(int value) {
          |    return value * 2;
          |  }
          |
          |  public Class<?> token() {
          |    return String.class;
          |  }
          |
          |  public String describe(int total) {
          |    return switch (total) {
          |      case 0 -> "zero";
          |      case 1, 2 -> "small";
          |      default -> "other";
          |    };
          |  }
          |
          |  public String describeBlock(int total) {
          |    return switch (total) {
          |      case 0 -> {
          |        yield "zero";
          |      }
          |      default -> {
          |        String result = "other";
          |        yield result;
          |      }
          |    };
          |  }
          |}
          |""".stripMargin,
        "demo/Exprs.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      def pickAst   = cpg.method.nameExact("pick").ast
      def pickCalls = pickAst.collectAll[Call]

      pickCalls.nameExact(Operators.conditional).code.l should contain(
        "input instanceof String ? ((String) input).length() : fallback"
      )
      pickCalls.nameExact(Operators.instanceOf).code.l should contain("input instanceof String")
      pickCalls.nameExact(Operators.cast).code.l should contain("(String) input")
      val List(bitwiseComplement: Call) = pickCalls.nameExact(Operators.not).l: @unchecked
      bitwiseComplement.code shouldBe "~fallback"
      bitwiseComplement.typeFullName shouldBe "int"
      bitwiseComplement.argument.isIdentifier.nameExact("fallback").refsTo.l shouldBe cpg.method
        .nameExact("pick")
        .parameter
        .nameExact("fallback")
        .l
      pickCalls.nameExact(Operators.logicalShiftRight).code.l should contain("fallback >> 1")
      pickCalls.nameExact(Operators.arithmeticShiftRight).code.l should contain("fallback >>> 2")
      pickAst.collectAll[TypeRef].typeFullName.l should contain("java.lang.String")

      val List(lengthCall: Call) = pickCalls.nameExact("length").codeExact("((String) input).length()").l: @unchecked
      lengthCall.methodFullName shouldBe "java.lang.String.length:int()"
      lengthCall.signature shouldBe "int()"
      lengthCall.typeFullName shouldBe "int"
      lengthCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

      val List(trimCall: Call) = pickCalls.nameExact("trim").codeExact("((String) input).trim()").l: @unchecked
      trimCall.methodFullName shouldBe "java.lang.String.trim:java.lang.String()"
      trimCall.signature shouldBe "java.lang.String()"
      trimCall.typeFullName shouldBe "java.lang.String"
      trimCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

      val List(isEmptyCall: Call) = pickCalls.nameExact("isEmpty").codeExact("cleaned.isEmpty()").l: @unchecked
      isEmptyCall.methodFullName shouldBe "java.lang.String.isEmpty:boolean()"
      isEmptyCall.signature shouldBe "boolean()"
      isEmptyCall.typeFullName shouldBe "boolean"
      isEmptyCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

      val List(stripCall: Call) = pickCalls.nameExact("strip").codeExact("cleaned.strip()").l: @unchecked
      stripCall.methodFullName shouldBe "java.lang.String.strip:java.lang.String()"
      stripCall.signature shouldBe "java.lang.String()"
      stripCall.typeFullName shouldBe "java.lang.String"
      stripCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

      val List(isBlankCall: Call) = pickCalls.nameExact("isBlank").codeExact("stripped.isBlank()").l: @unchecked
      isBlankCall.methodFullName shouldBe "java.lang.String.isBlank:boolean()"
      isBlankCall.signature shouldBe "boolean()"
      isBlankCall.typeFullName shouldBe "boolean"
      isBlankCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

      val List(valueOfCall: Call) = pickCalls.nameExact("valueOf").codeExact("String.valueOf(blank)").l: @unchecked
      valueOfCall.methodFullName shouldBe "java.lang.String.valueOf:java.lang.String(boolean)"
      valueOfCall.signature shouldBe "java.lang.String(boolean)"
      valueOfCall.typeFullName shouldBe "java.lang.String"
      valueOfCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

      val List(methodRef: MethodRef) = pickAst.collectAll[MethodRef].codeExact("String::trim").l: @unchecked
      methodRef.methodFullName shouldBe "java.lang.String.trim:java.lang.String()"
      methodRef.typeFullName shouldBe "java.lang.String"

      val List(localMethodRef: MethodRef) = pickAst.collectAll[MethodRef].codeExact("this::inc").l: @unchecked
      localMethodRef.methodFullName shouldBe "demo.Exprs.inc:int(int)"
      localMethodRef.typeFullName shouldBe "demo.Exprs"

      val List(staticMethodRef: MethodRef) = pickAst.collectAll[MethodRef].codeExact("Exprs::doubleIt").l: @unchecked
      staticMethodRef.methodFullName shouldBe "demo.Exprs.doubleIt:int(int)"
      staticMethodRef.typeFullName shouldBe "demo.Exprs"

      val List(constructorRef: MethodRef) = pickAst.collectAll[MethodRef].codeExact("Widget::new").l: @unchecked
      constructorRef.methodFullName shouldBe "demo.Widget.<init>:void()"
      constructorRef.typeFullName shouldBe "demo.Widget"

      val List(decodeMethod) =
        cpg.method.fullNameExact("demo.Exprs.decode:byte[](java.util.Base64$Decoder,java.lang.String)").l: @unchecked
      decodeMethod.parameter.nameExact("decoder").typeFullName.l shouldBe List("java.util.Base64$Decoder")
      decodeMethod.local.nameExact("localDecoder").typeFullName.l shouldBe List("java.util.Base64$Decoder")
      val decodeCalls = decodeMethod.ast.collectAll[Call].l
      val List(getDecoderCall: Call) = decodeCalls
        .filter(call => call.name == "getDecoder" && call.code == "Base64.getDecoder()"): @unchecked
      getDecoderCall.methodFullName shouldBe "java.util.Base64.getDecoder:java.util.Base64$Decoder()"
      getDecoderCall.signature shouldBe "java.util.Base64$Decoder()"
      getDecoderCall.typeFullName shouldBe "java.util.Base64$Decoder"
      getDecoderCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
      inside(decodeCalls.filter(_.name == "decode")) { case List(paramDecode: Call, localDecode: Call) =>
        paramDecode.code shouldBe "decoder.decode(src)"
        paramDecode.methodFullName shouldBe "java.util.Base64$Decoder.decode:byte[](java.lang.String)"
        paramDecode.signature shouldBe "byte[](java.lang.String)"
        paramDecode.typeFullName shouldBe "byte[]"
        paramDecode.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        localDecode.code shouldBe "localDecoder.decode(src)"
        localDecode.methodFullName shouldBe "java.util.Base64$Decoder.decode:byte[](java.lang.String)"
        localDecode.signature shouldBe "byte[](java.lang.String)"
        localDecode.typeFullName shouldBe "byte[]"
        localDecode.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
      }

      val List(classLiteral: Call) = cpg.method
        .nameExact("token")
        .ast
        .collectAll[Call]
        .nameExact(Operators.fieldAccess)
        .codeExact("String.class")
        .l: @unchecked
      classLiteral.typeFullName shouldBe "java.lang.Class"
      inside(classLiteral.argument.l) { case List(classTarget: Identifier, classField: FieldIdentifier) =>
        classTarget.name shouldBe "String"
        classTarget.code shouldBe "String"
        classTarget.typeFullName shouldBe "java.lang.String"
        classField.code shouldBe "class"
        classField.canonicalName shouldBe "class"
      }

      val List(initCall) = cpg.method
        .nameExact("<init>")
        .fullNameExact("demo.Exprs.<init>:void()")
        .ast
        .collectAll[Call]
        .nameExact("<init>")
        .l: @unchecked
      initCall.code shouldBe "this(1)"
      initCall.methodFullName shouldBe "demo.Exprs.<init>:void(int)"
      initCall.signature shouldBe "void(int)"

      cpg.typeDecl.nameExact("Exprs").inheritsFromTypeFullName.l should contain("demo.BaseExprs")
      val List(superCall) = cpg.method
        .nameExact("<init>")
        .fullNameExact("demo.Exprs.<init>:void(int)")
        .ast
        .collectAll[Call]
        .nameExact("<init>")
        .codeExact("super(seed)")
        .l: @unchecked
      superCall.methodFullName shouldBe "demo.BaseExprs.<init>:void(int)"
      superCall.signature shouldBe "void(int)"
      superCall.argument.code.l shouldBe List("this", "seed")

      val List(matchNode) = cpg.method
        .nameExact("describe")
        .ast
        .collectAll[ControlStructure]
        .controlStructureType(ControlStructureTypes.MATCH)
        .l: @unchecked
      matchNode.code shouldBe "switch(total)"
      inside(matchNode.astChildren.l) { case List(selector: Identifier, body: Block) =>
        selector.code shouldBe "total"
        inside(body.astChildren.l) {
          case List(
                case0Target: JumpTarget,
                case0Label: Literal,
                zeroResult: Literal,
                case1Target: JumpTarget,
                case1Label: Literal,
                case2Target: JumpTarget,
                case2Label: Literal,
                smallResult: Literal,
                defaultTarget: JumpTarget,
                otherResult: Literal
              ) =>
            case0Target.name shouldBe "case"
            case0Target.code shouldBe "0"
            case0Label.code shouldBe "0"
            zeroResult.code shouldBe "\"zero\""

            case1Target.name shouldBe "case"
            case1Target.code shouldBe "1"
            case1Label.code shouldBe "1"
            case2Target.name shouldBe "case"
            case2Target.code shouldBe "2"
            case2Label.code shouldBe "2"
            smallResult.code shouldBe "\"small\""

            defaultTarget.name shouldBe "default"
            defaultTarget.code shouldBe "default"
            otherResult.code shouldBe "\"other\""
        }
      }

      val List(blockMatchNode) = cpg.method
        .nameExact("describeBlock")
        .ast
        .collectAll[ControlStructure]
        .controlStructureType(ControlStructureTypes.MATCH)
        .l: @unchecked
      blockMatchNode.code shouldBe "switch(total)"
      inside(blockMatchNode.astChildren.l) { case List(selector: Identifier, body: Block) =>
        selector.code shouldBe "total"
        inside(body.astChildren.l) {
          case List(
                case0Target: JumpTarget,
                case0Label: Literal,
                zeroBlock: Block,
                defaultTarget: JumpTarget,
                defaultBlock: Block
              ) =>
            case0Target.name shouldBe "case"
            case0Label.code shouldBe "0"
            inside(zeroBlock.astChildren.collectAll[Return].l) { case List(zeroYield) =>
              zeroYield.code shouldBe """yield "zero";"""
              zeroYield.astChildren.code.l shouldBe List("\"zero\"")
            }

            defaultTarget.name shouldBe "default"
            defaultTarget.code shouldBe "default"
            defaultBlock.astChildren.collectAll[Local].nameExact("result").typeFullName.l shouldBe List(
              "java.lang.String"
            )
            inside(defaultBlock.astChildren.collectAll[Return].l) { case List(resultYield) =>
              resultYield.code shouldBe "yield result;"
              val List(resultIdentifier: Identifier) = resultYield.astChildren.l: @unchecked
              resultIdentifier.code shouldBe "result"
              resultIdentifier.refsTo.l shouldBe defaultBlock.astChildren.collectAll[Local].nameExact("result").l
            }
        }
      }
    }

    "create synthetic methods and method refs for lambda expressions" in {
      val cpg = code(
        """package demo;
          |
          |import java.util.function.Function;
          |
          |public class Lambdas {
          |  public void test(String input, String fallback) {
          |    getFromSupplier(input, lambdaInput -> lambdaInput.length() > 5 ? "Long" : fallback);
          |  }
          |
          |  private void getFromSupplier(String input, Function<String, String> mapper) {}
          |}
          |""".stripMargin,
        "demo/Lambdas.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(lambdaMethod) = cpg.typeDecl.nameExact("Lambdas").method.nameExact("<lambda>0").l: @unchecked
      lambdaMethod.fullName shouldBe "demo.Lambdas.<lambda>0:java.lang.String(java.lang.String)"
      lambdaMethod.parameter.name.l shouldBe List("lambdaInput")
      lambdaMethod.parameter.typeFullName.l shouldBe List("java.lang.String")
      lambdaMethod.methodReturn.typeFullName shouldBe "java.lang.String"
      lambdaMethod.modifier.modifierType.l should contain(ModifierTypes.LAMBDA)

      val List(methodRef: MethodRef) = cpg.method.nameExact("test").ast.collectAll[MethodRef].l: @unchecked
      methodRef.methodFullName shouldBe lambdaMethod.fullName
      methodRef.typeFullName shouldBe lambdaMethod.fullName

      lambdaMethod.ast.collectAll[Local].nameExact("fallback").typeFullName.l shouldBe List("java.lang.String")
      lambdaMethod.ast.collectAll[Return].code.l should contain(
        """return lambdaInput.length() > 5 ? "Long" : fallback;"""
      )
      lambdaMethod.ast.collectAll[Call].nameExact(Operators.conditional).size shouldBe 1

      val List(lambdaTypeDecl) = cpg.typeDecl.fullNameExact(lambdaMethod.fullName).l: @unchecked
      lambdaTypeDecl.name shouldBe "<lambda>0"
      lambdaTypeDecl.inheritsFromTypeFullName should contain("java.util.function.Function")

      cpg.all.collectAll[Binding].nameExact("<lambda>0").l shouldBe Nil
      inside(cpg.all.collectAll[Binding].nameExact("apply").sortBy(_.signature).l) {
        case List(erasedBinding, concreteBinding) =>
          erasedBinding.methodFullName shouldBe lambdaMethod.fullName
          erasedBinding.signature shouldBe "java.lang.Object(java.lang.Object)"
          erasedBinding.bindingTypeDecl.fullName shouldBe lambdaMethod.fullName

          concreteBinding.methodFullName shouldBe lambdaMethod.fullName
          concreteBinding.signature shouldBe "java.lang.String(java.lang.String)"
          concreteBinding.bindingTypeDecl.fullName shouldBe lambdaMethod.fullName
      }
    }

    "create bindings for lambdas implementing custom functional interfaces" in {
      val cpg = code(
        """package demo;
          |
          |public interface Foo<T, R> {
          |  String baz(T input, R moreInput);
          |  default T bar(T input) {
          |    return input;
          |  }
          |}
          |
          |public class TestClass {
          |  public static String foo(Integer x, Integer y, String z) {
          |    return z;
          |  }
          |
          |  public static Foo<Integer, Integer> test(String captured) {
          |    return (input, moreInput) -> foo(input, moreInput, captured);
          |  }
          |}
          |""".stripMargin,
        "demo/TestClass.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val lambdaFullName     = "demo.TestClass.<lambda>0:java.lang.String(java.lang.Integer,java.lang.Integer)"
      val List(lambdaMethod) = cpg.method.fullNameExact(lambdaFullName).l: @unchecked
      lambdaMethod.parameter.name.l shouldBe List("input", "moreInput")
      lambdaMethod.parameter.typeFullName.l shouldBe List("java.lang.Integer", "java.lang.Integer")
      lambdaMethod.methodReturn.typeFullName shouldBe "java.lang.String"

      val List(fooCall) = lambdaMethod.call.nameExact("foo").l: @unchecked
      fooCall.methodFullName shouldBe
        "demo.TestClass.foo:java.lang.String(java.lang.Integer,java.lang.Integer,java.lang.String)"
      fooCall.signature shouldBe "java.lang.String(java.lang.Integer,java.lang.Integer,java.lang.String)"
      fooCall.argument.code.l shouldBe List("input", "moreInput", "captured")

      val List(lambdaTypeDecl) = cpg.typeDecl.fullNameExact(lambdaFullName).l: @unchecked
      lambdaTypeDecl.inheritsFromTypeFullName should contain("demo.Foo")

      cpg.all.collectAll[Binding].nameExact("<lambda>0").l shouldBe Nil
      inside(
        cpg.all
          .collectAll[Binding]
          .nameExact("baz")
          .sortBy(binding => (binding.bindingTypeDecl.fullName, binding.signature))
          .l
      ) { case List(interfaceBinding, erasedBinding, concreteBinding) =>
        interfaceBinding.methodFullName shouldBe "demo.Foo.baz:java.lang.String(java.lang.Object,java.lang.Object)"
        interfaceBinding.signature shouldBe "java.lang.String(java.lang.Object,java.lang.Object)"
        interfaceBinding.bindingTypeDecl.fullName shouldBe "demo.Foo"

        erasedBinding.methodFullName shouldBe lambdaFullName
        erasedBinding.signature shouldBe "java.lang.Object(java.lang.Object,java.lang.Object)"
        erasedBinding.bindingTypeDecl.fullName shouldBe lambdaFullName

        concreteBinding.methodFullName shouldBe lambdaFullName
        concreteBinding.signature shouldBe "java.lang.String(java.lang.Integer,java.lang.Integer)"
        concreteBinding.bindingTypeDecl.fullName shouldBe lambdaFullName
      }
    }

    "create import nodes and annotation ASTs" in {
      val cpg = code(
        """package demo;
          |
          |import some.MarkerAnnotation;
          |import some.NormalAnnotation;
          |import java.util.*;
          |import module java.base;
          |
          |@MarkerAnnotation
          |public class Annotated {
          |  @NormalAnnotation(value = {"a", "b"})
          |  private String field;
          |
          |  @NormalAnnotation("method")
          |  public void run(@MarkerAnnotation String input) {}
          |}
          |""".stripMargin,
        "demo/Annotated.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      cpg.imports.importedAs.l should contain allOf ("MarkerAnnotation", "NormalAnnotation", "*")
      cpg.imports.importedEntity.l should contain allOf (
        "some.MarkerAnnotation",
        "some.NormalAnnotation",
        "java.util",
        "java.base"
      )
      cpg.imports.code.l should contain allOf (
        "import some.MarkerAnnotation",
        "import some.NormalAnnotation",
        "import java.util.*",
        "import module java.base"
      )
      val List(moduleImport) = cpg.imports.importedEntity("java.base").l: @unchecked
      moduleImport.isModuleImport shouldBe Some(true)

      val List(typeAnnotation) = cpg.typeDecl.nameExact("Annotated").annotation.l: @unchecked
      typeAnnotation.code shouldBe "@MarkerAnnotation"
      typeAnnotation.fullName shouldBe "some.MarkerAnnotation"

      val List(fieldAnnotation) = cpg.typeDecl.nameExact("Annotated").member.nameExact("field").annotation.l: @unchecked
      fieldAnnotation.code shouldBe """@NormalAnnotation(value = { "a", "b" })"""
      fieldAnnotation.fullName shouldBe "some.NormalAnnotation"
      fieldAnnotation.parameterAssign.code.l shouldBe List("""value = { "a", "b" }""")
      fieldAnnotation.parameterAssign.parameter.code.l shouldBe List("value")
      fieldAnnotation.parameterAssign.value.isArrayInitializer.code.l shouldBe List("""{ "a", "b" }""")
      fieldAnnotation.parameterAssign.value.astChildren.code.l shouldBe List("a", "b")

      val List(methodAnnotation) = cpg.method.nameExact("run").annotation.l: @unchecked
      methodAnnotation.code shouldBe """@NormalAnnotation("method")"""
      methodAnnotation.parameterAssign.parameter.code.l shouldBe List("value")
      methodAnnotation.parameterAssign.value.code.l shouldBe List("method")

      val List(parameterAnnotation) = cpg.method.nameExact("run").parameter.nameExact("input").annotation.l: @unchecked
      parameterAnnotation.code shouldBe "@MarkerAnnotation"
      parameterAnnotation.fullName shouldBe "some.MarkerAnnotation"
    }

    "resolve common java.base module-imported types" in {
      val cpg = code(
        """package demo;
          |
          |import module java.base;
          |
          |public class ModuleImportedTypes {
          |  void test() {
          |    ArrayList<String> list = new ArrayList<>();
          |    Path path = null;
          |    OptionalInt maybe = OptionalInt.empty();
          |    Stream<String> stream = null;
          |    FileReader reader = new FileReader("x");
          |  }
          |}
          |""".stripMargin,
        "demo/ModuleImportedTypes.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      cpg.method
        .nameExact("test")
        .local
        .nameExact("list")
        .typeFullName
        .l shouldBe List("java.util.ArrayList<java.lang.String>")
      cpg.method.nameExact("test").local.nameExact("path").typeFullName.l shouldBe List("java.nio.file.Path")
      cpg.method.nameExact("test").local.nameExact("maybe").typeFullName.l shouldBe List("java.util.OptionalInt")
      cpg.method
        .nameExact("test")
        .local
        .nameExact("stream")
        .typeFullName
        .l shouldBe List("java.util.stream.Stream<java.lang.String>")
      cpg.method.nameExact("test").local.nameExact("reader").typeFullName.l shouldBe List("java.io.FileReader")

      cpg.call.nameExact(Operators.alloc).codeExact("new ArrayList<>()").typeFullName.l shouldBe List(
        "java.util.ArrayList"
      )
      cpg.call.nameExact(Operators.alloc).codeExact("""new FileReader("x")""").typeFullName.l shouldBe List(
        "java.io.FileReader"
      )
    }

    "create default constructors and lower instance field initializers into constructors" in {
      val cpg = code(
        """package demo;
          |
          |public class Defaults {
          |  private int x = 1;
          |  String name = "seed";
          |
          |  public int read() {
          |    return x;
          |  }
          |}
          |
          |class Explicit {
          |  int y = 2;
          |
          |  Explicit(int value) {
          |    y = value;
          |  }
          |}
          |""".stripMargin,
        "demo/Defaults.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(defaultCtor) = cpg.typeDecl.nameExact("Defaults").method.nameExact("<init>").l: @unchecked
      defaultCtor.fullName shouldBe "demo.Defaults.<init>:void()"
      defaultCtor.signature shouldBe "void()"
      defaultCtor.parameter.name.l shouldBe List("this")
      defaultCtor.parameter.typeFullName.l shouldBe List("demo.Defaults")
      defaultCtor.modifier.modifierType.toSet shouldBe Set(ModifierTypes.CONSTRUCTOR, ModifierTypes.PUBLIC)
      defaultCtor.methodReturn.typeFullName shouldBe "void"

      val defaultAssignments = defaultCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).l
      defaultAssignments.code.l should contain allOf ("this.x = 1", "this.name = \"seed\"")
      val List(xAssignment) = defaultAssignments.codeExact("this.x = 1").l: @unchecked
      inside(xAssignment.argument.l) { case List(fieldAccess: Call, value: Literal) =>
        fieldAccess.name shouldBe Operators.fieldAccess
        fieldAccess.code shouldBe "this.x"
        inside(fieldAccess.argument.l) { case List(thisIdentifier: Identifier, fieldIdentifier: FieldIdentifier) =>
          thisIdentifier.name shouldBe "this"
          thisIdentifier.typeFullName shouldBe "demo.Defaults"
          fieldIdentifier.canonicalName shouldBe "x"
        }
        value.code shouldBe "1"
        value.typeFullName shouldBe "int"
      }

      val List(explicitCtor) = cpg.typeDecl.nameExact("Explicit").method.nameExact("<init>").l: @unchecked
      explicitCtor.fullName shouldBe "demo.Explicit.<init>:void(int)"
      explicitCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l should contain allOf (
        "this.y = 2",
        "y = value"
      )
    }

    "create enum constants and enum body declarations" in {
      val cpg = code(
        """package demo;
          |
          |public enum FuzzyBool {
          |  TRUE,
          |  FALSE,
          |  MAYBE
          |}
          |
          |enum Color {
          |  RED("Red"),
          |  BLUE("Blue");
          |
          |  public final String label;
          |
          |  private Color(String label) {
          |    this.label = label;
          |  }
          |
          |  int code() {
          |    return 1;
          |  }
          |}
          |""".stripMargin,
        "demo/Enums.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(fuzzyBool) = cpg.typeDecl.nameExact("FuzzyBool").l: @unchecked
      fuzzyBool.fullName shouldBe "demo.FuzzyBool"
      fuzzyBool.code shouldBe "public enum FuzzyBool"
      fuzzyBool.inheritsFromTypeFullName should contain("java.lang.Enum")
      fuzzyBool.modifier.modifierType.l shouldBe List(ModifierTypes.PUBLIC)

      inside(fuzzyBool.member.l) { case List(trueMember, falseMember, maybeMember) =>
        trueMember.order shouldBe 1
        trueMember.name shouldBe "TRUE"
        trueMember.code shouldBe "TRUE"
        trueMember.typeFullName shouldBe "demo.FuzzyBool"

        falseMember.order shouldBe 2
        falseMember.name shouldBe "FALSE"
        falseMember.code shouldBe "FALSE"
        falseMember.typeFullName shouldBe "demo.FuzzyBool"

        maybeMember.order shouldBe 3
        maybeMember.name shouldBe "MAYBE"
        maybeMember.code shouldBe "MAYBE"
        maybeMember.typeFullName shouldBe "demo.FuzzyBool"
      }

      val List(color) = cpg.typeDecl.nameExact("Color").l: @unchecked
      color.fullName shouldBe "demo.Color"
      color.inheritsFromTypeFullName should contain("java.lang.Enum")
      inside(color.member.l) { case List(redMember, blueMember, labelMember) =>
        redMember.name shouldBe "RED"
        redMember.code shouldBe """RED("Red")"""
        redMember.typeFullName shouldBe "demo.Color"

        blueMember.name shouldBe "BLUE"
        blueMember.code shouldBe """BLUE("Blue")"""
        blueMember.typeFullName shouldBe "demo.Color"

        labelMember.name shouldBe "label"
        labelMember.code shouldBe "String label"
        labelMember.typeFullName shouldBe "java.lang.String"
        labelMember.modifier.modifierType.toSet shouldBe Set(ModifierTypes.PUBLIC, ModifierTypes.FINAL)
      }

      val List(colorCtor) = color.method.nameExact("<init>").l: @unchecked
      colorCtor.fullName shouldBe "demo.Color.<init>:void(java.lang.String)"
      colorCtor.parameter.name.l shouldBe List("this", "label")
      colorCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
        "this.label = label"
      )

      val List(codeMethod) = color.method.nameExact("code").l: @unchecked
      codeMethod.fullName shouldBe "demo.Color.code:int()"
      codeMethod.body.astChildren.collectAll[Return].code.l shouldBe List("return 1;")
    }

    "create anonymous enum constant type declarations and clinit constructor calls" in {
      val cpg = code(
        """package demo;
          |
          |enum EnumAnon {
          |  ENTRY(42) {
          |    @Override
          |    int getValue() {
          |      return value + 7;
          |    }
          |  };
          |
          |  protected int value;
          |
          |  int getValue() {
          |    return value;
          |  }
          |
          |  EnumAnon(int value) {
          |    this.value = value;
          |  }
          |}
          |""".stripMargin,
        "demo/AnonymousEnum.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val anonymousFullName = "demo.EnumAnon$0"
      cpg.typeDecl.nameExact("EnumAnon").ast.collectAll[TypeDecl].fullNameExact(anonymousFullName).size shouldBe 1
      cpg.typeDecl.fullNameExact(anonymousFullName).inheritsFromTypeFullName.l shouldBe List("demo.EnumAnon")

      val List(entryMember) = cpg.typeDecl.nameExact("EnumAnon").member.nameExact("ENTRY").l: @unchecked
      entryMember.typeFullName shouldBe anonymousFullName

      val List(anonymousCtor) = cpg.typeDecl.fullNameExact(anonymousFullName).method.nameExact("<init>").l: @unchecked
      anonymousCtor.fullName shouldBe s"$anonymousFullName.<init>:void(int)"
      anonymousCtor.parameter.name.l shouldBe List("this", "arg0")
      anonymousCtor.parameter.typeFullName.l shouldBe List(anonymousFullName, "int")
      val List(superCall) =
        anonymousCtor.ast.collectAll[Call].nameExact("<init>").codeExact("super(arg0)").l: @unchecked
      superCall.methodFullName shouldBe "demo.EnumAnon.<init>:void(int)"
      superCall.argument.code.l shouldBe List("this", "arg0")

      val List(initCall) = cpg.typeDecl
        .nameExact("EnumAnon")
        .method
        .nameExact("<clinit>")
        .call
        .nameExact("<init>")
        .methodFullNameExact(s"$anonymousFullName.<init>:void(int)")
        .l: @unchecked
      initCall.signature shouldBe "void(int)"
      initCall.argument.code.l shouldBe List("ENTRY", "42")
      initCall.argument.collectAll[Identifier].nameExact("ENTRY").typeFullName.l shouldBe List(anonymousFullName)

      val List(valueAccess: Call) = cpg.method
        .fullNameExact(s"$anonymousFullName.getValue:int()")
        .ast
        .collectAll[Call]
        .nameExact(Operators.fieldAccess)
        .codeExact("this.value")
        .l: @unchecked
      valueAccess.typeFullName shouldBe "int"
    }

    "create static initializer methods for static field initializers and static blocks" in {
      val cpg = code(
        """package demo;
          |
          |class StaticInit {
          |  static int count = 1;
          |  static StaticInit singleton = new StaticInit();
          |
          |  static {
          |    ping("ready");
          |  }
          |
          |  static void ping(String value) {}
          |}
          |
          |class NoStatic {
          |  int value = 1;
          |}
          |""".stripMargin,
        "demo/StaticInit.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      cpg.typeDecl.nameExact("NoStatic").method.nameExact("<clinit>").l shouldBe Nil

      val List(countMember) = cpg.typeDecl.nameExact("StaticInit").member.nameExact("count").l: @unchecked
      countMember.typeFullName shouldBe "int"
      countMember.modifier.modifierType.l should contain(ModifierTypes.STATIC)

      val List(singletonMember) = cpg.typeDecl.nameExact("StaticInit").member.nameExact("singleton").l: @unchecked
      singletonMember.typeFullName shouldBe "demo.StaticInit"
      singletonMember.modifier.modifierType.l should contain(ModifierTypes.STATIC)

      val List(clinit) = cpg.typeDecl.nameExact("StaticInit").method.nameExact("<clinit>").l: @unchecked
      clinit.fullName shouldBe "demo.StaticInit.<clinit>:void()"
      clinit.signature shouldBe "void()"
      clinit.parameter.l shouldBe Nil
      clinit.modifier.modifierType.l should contain(ModifierTypes.STATIC)
      clinit.methodReturn.typeFullName shouldBe "void"

      inside(clinit.body.astChildren.l) {
        case List(countAssign: Call, singletonAssign: Call, singletonInit: Call, pingCall: Call) =>
          countAssign.name shouldBe Operators.assignment
          countAssign.code shouldBe "StaticInit.count = 1"
          inside(countAssign.argument.l) { case List(fieldAccess: Call, value: Literal) =>
            fieldAccess.code shouldBe "StaticInit.count"
            fieldAccess.typeFullName shouldBe "int"
            inside(fieldAccess.argument.l) { case List(ownerType: TypeRef, fieldIdentifier: FieldIdentifier) =>
              ownerType.code shouldBe "StaticInit"
              ownerType.typeFullName shouldBe "demo.StaticInit"
              fieldIdentifier.canonicalName shouldBe "count"
            }
            value.code shouldBe "1"
            value.typeFullName shouldBe "int"
          }

          singletonAssign.name shouldBe Operators.assignment
          singletonAssign.code shouldBe "StaticInit.singleton = new StaticInit()"
          inside(singletonAssign.argument.l) { case List(fieldAccess: Call, alloc: Call) =>
            fieldAccess.code shouldBe "StaticInit.singleton"
            fieldAccess.typeFullName shouldBe "demo.StaticInit"
            alloc.name shouldBe Operators.alloc
            alloc.code shouldBe "new StaticInit()"
            alloc.typeFullName shouldBe "demo.StaticInit"
          }

          singletonInit.name shouldBe "<init>"
          singletonInit.code shouldBe "new StaticInit()"
          singletonInit.methodFullName shouldBe "demo.StaticInit.<init>:void()"
          singletonInit.signature shouldBe "void()"
          singletonInit.argument.code.l shouldBe List("StaticInit.singleton")

          pingCall.name shouldBe "ping"
          pingCall.code shouldBe """ping("ready")"""
          pingCall.methodFullName shouldBe "demo.StaticInit.ping:void(java.lang.String)"
          pingCall.signature shouldBe "void(java.lang.String)"
      }
    }

    "create record component members, accessors, and canonical constructors" in {
      val cpg = code(
        """package demo;
          |
          |public record Point(String label, int count) {}
          |""".stripMargin,
        "demo/Point.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(point) = cpg.typeDecl.nameExact("Point").l: @unchecked
      point.fullName shouldBe "demo.Point"
      point.inheritsFromTypeFullName should contain("java.lang.Record")

      cpg.typeDecl.nameExact("Point").member.name.l should contain allOf ("label", "count")
      val List(labelMember) = cpg.typeDecl.nameExact("Point").member.nameExact("label").l: @unchecked
      labelMember.code shouldBe "String label"
      labelMember.typeFullName shouldBe "java.lang.String"
      labelMember.modifier.modifierType.toSet shouldBe Set(ModifierTypes.PRIVATE, ModifierTypes.FINAL)

      val List(canonicalCtor) = cpg.typeDecl.nameExact("Point").method.nameExact("<init>").l: @unchecked
      canonicalCtor.fullName shouldBe "demo.Point.<init>:void(java.lang.String,int)"
      canonicalCtor.signature shouldBe "void(java.lang.String,int)"
      canonicalCtor.parameter.name.l shouldBe List("this", "label", "count")
      canonicalCtor.parameter.typeFullName.l shouldBe List("demo.Point", "java.lang.String", "int")
      canonicalCtor.modifier.modifierType.toSet shouldBe Set(ModifierTypes.CONSTRUCTOR, ModifierTypes.PUBLIC)
      canonicalCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l should contain allOf (
        "this.label = label",
        "this.count = count"
      )

      val List(labelAssignment) =
        canonicalCtor.body.astChildren
          .collectAll[Call]
          .nameExact(Operators.assignment)
          .codeExact("this.label = label")
          .l: @unchecked
      inside(labelAssignment.argument.l) { case List(fieldAccess: Call, labelIdentifier: Identifier) =>
        fieldAccess.code shouldBe "this.label"
        fieldAccess.typeFullName shouldBe "java.lang.String"
        labelIdentifier.name shouldBe "label"
        labelIdentifier.refsTo.l shouldBe canonicalCtor.parameter.nameExact("label").l
      }

      val List(labelAccessor) = cpg.typeDecl.nameExact("Point").method.nameExact("label").l: @unchecked
      labelAccessor.fullName shouldBe "demo.Point.label:java.lang.String()"
      labelAccessor.methodReturn.typeFullName shouldBe "java.lang.String"
      labelAccessor.parameter.name.l shouldBe List("this")
      labelAccessor.body.astChildren.collectAll[Return].code.l shouldBe List("return this.label")
      val List(labelAccess) =
        labelAccessor.body.astChildren
          .collectAll[Return]
          .astChildren
          .collectAll[Call]
          .nameExact(Operators.fieldAccess)
          .l: @unchecked
      labelAccess.code shouldBe "this.label"
      labelAccess.typeFullName shouldBe "java.lang.String"
    }

    "create compact record constructors with component assignments before the body" in {
      val cpg = code(
        """package demo;
          |
          |public record Compact(String value) {
          |  public Compact {
          |    System.out.println(value);
          |  }
          |}
          |""".stripMargin,
        "demo/Compact.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(ctor) = cpg.typeDecl.nameExact("Compact").method.nameExact("<init>").l: @unchecked
      ctor.fullName shouldBe "demo.Compact.<init>:void(java.lang.String)"
      ctor.parameter.name.l shouldBe List("this", "value")
      ctor.parameter.typeFullName.l shouldBe List("demo.Compact", "java.lang.String")

      inside(ctor.body.astChildren.l) { case List(valueAssignment: Call, printlnCall: Call) =>
        valueAssignment.name shouldBe Operators.assignment
        valueAssignment.code shouldBe "this.value = value"
        printlnCall.name shouldBe "println"
        printlnCall.code shouldBe "System.out.println(value)"

        val List(printlnValue) = printlnCall.astChildren.collectAll[Identifier].nameExact("value").l: @unchecked
        printlnValue.refsTo.l shouldBe ctor.parameter.nameExact("value").l
      }
    }

    "create explicit and synthesized record constructors without duplicates" in {
      val cpg = code(
        """package demo;
          |
          |public record WithDefault(String value) {
          |  public WithDefault() {
          |    this.value = "value";
          |  }
          |}
          |
          |record WithExplicit(String value) {
          |  public WithExplicit(String value) {
          |    System.out.println(value);
          |    this.value = value;
          |  }
          |}
          |""".stripMargin,
        "demo/Constructors.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      inside(cpg.typeDecl.nameExact("WithDefault").method.nameExact("<init>").sortBy(_.parameter.size).l) {
        case List(nonCanonicalCtor, canonicalCtor) =>
          nonCanonicalCtor.fullName shouldBe "demo.WithDefault.<init>:void()"
          nonCanonicalCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l should contain(
            "this.value = \"value\""
          )

          canonicalCtor.fullName shouldBe "demo.WithDefault.<init>:void(java.lang.String)"
          canonicalCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
            "this.value = value"
          )
      }

      val List(explicitCanonicalCtor) = cpg.typeDecl.nameExact("WithExplicit").method.nameExact("<init>").l: @unchecked
      explicitCanonicalCtor.fullName shouldBe "demo.WithExplicit.<init>:void(java.lang.String)"
      explicitCanonicalCtor.body.astChildren.collectAll[Call].nameExact("println").code.l shouldBe List(
        "System.out.println(value)"
      )
      explicitCanonicalCtor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
        "this.value = value"
      )
    }

    "erase generic record component types in members, accessors, and canonical constructors" in {
      val cpg = code(
        """package demo;
          |
          |public record Box<T>(T value) {}
          |""".stripMargin,
        "demo/Box.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(valueMember) = cpg.typeDecl.nameExact("Box").member.nameExact("value").l: @unchecked
      valueMember.code shouldBe "T value"
      valueMember.typeFullName shouldBe "java.lang.Object"

      val List(ctor) = cpg.typeDecl.nameExact("Box").method.nameExact("<init>").l: @unchecked
      ctor.fullName shouldBe "demo.Box.<init>:void(java.lang.Object)"
      ctor.parameter.name.l shouldBe List("this", "value")
      ctor.parameter.typeFullName.l shouldBe List("demo.Box", "java.lang.Object")
      ctor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).typeFullName.l shouldBe List(
        "java.lang.Object"
      )

      val List(accessor) = cpg.typeDecl.nameExact("Box").method.nameExact("value").l: @unchecked
      accessor.fullName shouldBe "demo.Box.value:java.lang.Object()"
      accessor.methodReturn.typeFullName shouldBe "java.lang.Object"
    }

    "create method-local class capture members and constructor arguments" in {
      val cpg = code(
        """package demo;
          |
          |public class Foo {
          |  void enclosingMethod(int capturedParam) {
          |    int capturedLocal = 1;
          |    class Local {
          |      void usesCaptures() {
          |        sink(capturedParam, capturedLocal);
          |      }
          |    }
          |    Local local = new Local();
          |    local.usesCaptures();
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val ownerFullName   = "demo.Foo.enclosingMethod:void(int)"
      val localFullName   = s"$ownerFullName.Local"
      val List(localDecl) = cpg.typeDecl.fullNameExact(localFullName).l: @unchecked
      localDecl.code shouldBe "class Local"
      localDecl.astParentFullName shouldBe ownerFullName
      localDecl.member.name.l should contain allOf ("outerClass", "capturedParam", "capturedLocal")
      localDecl.member.nameExact("outerClass").typeFullName.l shouldBe List("demo.Foo")
      localDecl.member.nameExact("capturedParam").typeFullName.l shouldBe List("int")
      localDecl.member.nameExact("capturedLocal").typeFullName.l shouldBe List("int")

      val List(ctor) = localDecl.method.nameExact("<init>").l: @unchecked
      ctor.fullName shouldBe s"$localFullName.<init>:void()"
      ctor.parameter.name.l shouldBe List("this", "outerClass", "capturedParam", "capturedLocal")
      ctor.parameter.typeFullName.l shouldBe List(localFullName, "demo.Foo", "int", "int")
      ctor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List(
        "this.outerClass = outerClass",
        "this.capturedParam = capturedParam",
        "this.capturedLocal = capturedLocal"
      )

      cpg.method.fullNameExact(ownerFullName).local.nameExact("local").typeFullName.l shouldBe List(localFullName)
      val List(initCall) =
        cpg.method.fullNameExact(ownerFullName).call.nameExact("<init>").codeExact("new Local()").l: @unchecked
      initCall.methodFullName shouldBe s"$localFullName.<init>:void()"
      initCall.argument.code.l shouldBe List("local", "this", "capturedParam", "capturedLocal")
      initCall.argument.isIdentifier
        .nameExact("local")
        .refsTo
        .l shouldBe cpg.method.fullNameExact(ownerFullName).local.nameExact("local").l
      initCall.argument.isIdentifier.nameExact("capturedParam").refsTo.l shouldBe cpg.method
        .fullNameExact(ownerFullName)
        .parameter
        .nameExact("capturedParam")
        .l
      initCall.argument.isIdentifier.nameExact("capturedLocal").refsTo.l shouldBe cpg.method
        .fullNameExact(ownerFullName)
        .local
        .nameExact("capturedLocal")
        .l

      cpg.method
        .fullNameExact(s"$localFullName.usesCaptures:void()")
        .call
        .nameExact(Operators.fieldAccess)
        .code
        .l should contain allOf ("this.capturedParam", "this.capturedLocal")
      val List(usesCall) = cpg.method.fullNameExact(ownerFullName).call.nameExact("usesCaptures").l: @unchecked
      usesCall.methodFullName shouldBe s"$localFullName.usesCaptures:void()"
    }

    "resolve qualified this through lambda and local class captures" in {
      val cpg = code(
        """package demo;
          |
          |public class Outer {
          |  private String outerValue = "outer";
          |
          |  public void method() {
          |    Runnable r = () -> {
          |      class Inner {
          |        void innerMethod() {
          |          sink(Outer.this.outerValue);
          |        }
          |      }
          |      Inner inner = new Inner();
          |      inner.innerMethod();
          |    };
          |  }
          |}
          |""".stripMargin,
        "demo/Outer.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val List(lambdaMethod) = cpg.typeDecl.nameExact("Outer").method.nameExact("<lambda>0").l: @unchecked
      lambdaMethod.parameter.name.l shouldBe List("this")
      lambdaMethod.parameter.typeFullName.l shouldBe List("demo.Outer")
      lambdaMethod.local.nameExact("this").typeFullName.l shouldBe List("demo.Outer")
      lambdaMethod.local.nameExact("this").closureBindingId.l shouldBe List("demo/Outer.java:<lambda>0:this")

      val List(thisClosureBinding) = cpg.closureBinding.l: @unchecked
      thisClosureBinding.closureBindingId shouldBe Some("demo/Outer.java:<lambda>0:this")
      thisClosureBinding._refOut.l shouldBe cpg.method
        .fullNameExact("demo.Outer.method:void()")
        .parameter
        .nameExact("this")
        .l
      thisClosureBinding._captureIn.l shouldBe cpg.methodRef.methodFullNameExact(lambdaMethod.fullName).l

      val innerFullName   = s"${lambdaMethod.fullName}.Inner"
      val List(innerDecl) = cpg.typeDecl.fullNameExact(innerFullName).l: @unchecked
      innerDecl.member.nameExact("outerClass").typeFullName.l shouldBe List("demo.Outer")

      val List(innerCtor) = innerDecl.method.nameExact("<init>").l: @unchecked
      innerCtor.parameter.name.l shouldBe List("this", "outerClass")
      innerCtor.parameter.typeFullName.l shouldBe List(innerFullName, "demo.Outer")

      val List(initCall) = cpg.method
        .fullNameExact(lambdaMethod.fullName)
        .call
        .nameExact("<init>")
        .codeExact("new Inner()")
        .l: @unchecked
      initCall.methodFullName shouldBe s"$innerFullName.<init>:void()"
      initCall.argument.code.l shouldBe List("inner", "this")

      val List(outerValueAccess) = cpg.method
        .fullNameExact(s"$innerFullName.innerMethod:void()")
        .call
        .nameExact(Operators.fieldAccess)
        .codeExact("Outer.this.outerValue")
        .l: @unchecked
      outerValueAccess.typeFullName shouldBe "java.lang.String"
      inside(outerValueAccess.argument.l) { case List(qualifiedThisAccess: Call, outerValueField: FieldIdentifier) =>
        qualifiedThisAccess.code shouldBe "Outer.this"
        qualifiedThisAccess.typeFullName shouldBe "demo.Outer"
        outerValueField.canonicalName shouldBe "outerValue"

        inside(qualifiedThisAccess.argument.l) {
          case List(thisIdentifier: Identifier, outerClassField: FieldIdentifier) =>
            thisIdentifier.name shouldBe "this"
            thisIdentifier.typeFullName shouldBe innerFullName
            outerClassField.canonicalName shouldBe "outerClass"
        }
      }
    }

    "resolve nested local class captures through outerClass chains" in {
      val cpg = code(
        """package demo;
          |
          |class Foo {
          |  void foo(int fooParam) {
          |    class Bar {
          |      void bar() {
          |        class Baz {
          |          void baz() {
          |            sink(fooParam);
          |          }
          |        }
          |      }
          |    }
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val barFullName = "demo.Foo.foo:void(int).Bar"
      val bazFullName = s"$barFullName.bar:void().Baz"

      val List(barCtor) = cpg.typeDecl.fullNameExact(barFullName).method.nameExact("<init>").l: @unchecked
      barCtor.parameter.name.l shouldBe List("this", "outerClass", "fooParam")
      barCtor.parameter.typeFullName.l shouldBe List(barFullName, "demo.Foo", "int")

      val List(bazCtor) = cpg.typeDecl.fullNameExact(bazFullName).method.nameExact("<init>").l: @unchecked
      bazCtor.parameter.name.l shouldBe List("this", "outerClass")
      bazCtor.parameter.typeFullName.l shouldBe List(bazFullName, barFullName)

      val List(fooParamAccess) = cpg.method
        .fullNameExact(s"$bazFullName.baz:void()")
        .call
        .nameExact(Operators.fieldAccess)
        .codeExact("this.outerClass.fooParam")
        .l: @unchecked
      fooParamAccess.typeFullName shouldBe "int"
      inside(fooParamAccess.argument.l) { case List(outerClassAccess: Call, fooParamField: FieldIdentifier) =>
        outerClassAccess.code shouldBe "this.outerClass"
        outerClassAccess.typeFullName shouldBe barFullName
        fooParamField.canonicalName shouldBe "fooParam"

        inside(outerClassAccess.argument.l) { case List(thisIdentifier: Identifier, outerClassField: FieldIdentifier) =>
          thisIdentifier.name shouldBe "this"
          thisIdentifier.typeFullName shouldBe bazFullName
          outerClassField.canonicalName shouldBe "outerClass"
        }
      }
    }

    "resolve lambdas inside local classes through promoted capture fields" in {
      val cpg = code(
        """package demo;
          |
          |import java.util.function.Supplier;
          |
          |class Test {
          |  static void sinksSupplier(Supplier<String> supplier) {
          |    sink(supplier.get());
          |  }
          |
          |  void test(String value) {
          |    class LocalClass {
          |      public void localClassMethod() {
          |        sinksSupplier(() -> value);
          |      }
          |    }
          |
          |    new LocalClass().localClassMethod();
          |  }
          |}
          |""".stripMargin,
        "demo/Test.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val ownerFullName       = "demo.Test.test:void(java.lang.String)"
      val localFullName       = s"$ownerFullName.LocalClass"
      val localMethodFullName = s"$localFullName.localClassMethod:void()"
      val lambdaFullName      = s"$localFullName.<lambda>0:java.lang.String()"

      cpg.typeDecl.fullNameExact(localFullName).member.name.l should contain allOf ("outerClass", "value")
      cpg.typeDecl.fullNameExact(localFullName).member.nameExact("value").typeFullName.l shouldBe List(
        "java.lang.String"
      )

      val List(lambdaMethod) = cpg.method.fullNameExact(lambdaFullName).l: @unchecked
      lambdaMethod.parameter.name.l shouldBe List("this")
      lambdaMethod.parameter.typeFullName.l shouldBe List(localFullName)
      lambdaMethod.local.name.l shouldBe List("this")
      lambdaMethod.local.nameExact("this").typeFullName.l shouldBe List(localFullName)

      val List(thisClosureBinding) = cpg.closureBinding.l: @unchecked
      thisClosureBinding.closureBindingId shouldBe Some("demo/Test.java:<lambda>0:this")
      thisClosureBinding._refOut.l shouldBe cpg.method.fullNameExact(localMethodFullName).parameter.nameExact("this").l
      thisClosureBinding._captureIn.l shouldBe cpg.methodRef.methodFullNameExact(lambdaFullName).l
      lambdaMethod.local.nameExact("this").closureBindingId.l shouldBe List("demo/Test.java:<lambda>0:this")

      val List(valueAccess) =
        lambdaMethod.ast.isReturn.astChildren.isCall.nameExact(Operators.fieldAccess).l: @unchecked
      valueAccess.code shouldBe "this.value"
      valueAccess.typeFullName shouldBe "java.lang.String"
      inside(valueAccess.argument.l) { case List(thisIdentifier: Identifier, valueField: FieldIdentifier) =>
        thisIdentifier.name shouldBe "this"
        thisIdentifier.typeFullName shouldBe localFullName
        thisIdentifier.refsTo.l shouldBe lambdaMethod.local.nameExact("this").l
        valueField.canonicalName shouldBe "value"
      }
    }

    "resolve nested local class method calls through outerClass chains" in {
      val cpg = code(
        """package demo;
          |
          |class Foo {
          |  void outerCall() {}
          |  void foo() {
          |    class Bar {
          |      void bar() {
          |        class Baz {
          |          void baz() {
          |            outerCall();
          |          }
          |        }
          |      }
          |    }
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val barFullName = "demo.Foo.foo:void().Bar"
      val bazFullName = s"$barFullName.bar:void().Baz"

      val List(outerCall) =
        cpg.method.fullNameExact(s"$bazFullName.baz:void()").call.nameExact("outerCall").l: @unchecked
      outerCall.methodFullName shouldBe "demo.Foo.outerCall:void()"
      outerCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

      inside(outerCall.receiver.l) { case List(fooReceiver: Call) =>
        fooReceiver.code shouldBe "this.outerClass.outerClass"
        fooReceiver.typeFullName shouldBe "demo.Foo"

        inside(fooReceiver.argument.l) { case List(barReceiver: Call, fooOuterClassField: FieldIdentifier) =>
          barReceiver.code shouldBe "this.outerClass"
          barReceiver.typeFullName shouldBe barFullName
          fooOuterClassField.canonicalName shouldBe "outerClass"

          inside(barReceiver.argument.l) { case List(thisIdentifier: Identifier, barOuterClassField: FieldIdentifier) =>
            thisIdentifier.name shouldBe "this"
            thisIdentifier.typeFullName shouldBe bazFullName
            barOuterClassField.canonicalName shouldBe "outerClass"
          }
        }
      }
    }

    "respect static boundaries in nested local class captures" in {
      val cpg = code(
        """package demo;
          |
          |public class Test {
          |  int testMember = 1;
          |
          |  void test(int testParam) {
          |    int testLocal = 2;
          |
          |    class Foo {
          |      int fooMember = 4;
          |
          |      static void foo(int fooParam) {
          |        int fooLocal = 8;
          |
          |        class Bar {
          |          int barMember = 16;
          |
          |          void bar(int barParam) {
          |            int barLocal = 32;
          |
          |            class Baz {
          |              void baz() {
          |                sink(fooParam, fooLocal, barMember, barParam, barLocal);
          |              }
          |            }
          |          }
          |        }
          |      }
          |
          |      void fooCaptures() {
          |        sink(testMember, testParam, testLocal);
          |      }
          |    }
          |  }
          |}
          |""".stripMargin,
        "demo/Test.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val fooFullName = "demo.Test.test:void(int).Foo"
      val barFullName = s"$fooFullName.foo:void(int).Bar"
      val bazFullName = s"$barFullName.bar:void(int).Baz"

      cpg.typeDecl
        .fullNameExact(fooFullName)
        .member
        .name
        .l should contain allOf ("outerClass", "testParam", "testLocal")
      cpg.typeDecl.fullNameExact(barFullName).member.name.l should contain allOf ("fooParam", "fooLocal", "barMember")
      cpg.typeDecl.fullNameExact(barFullName).member.nameExact("outerClass").l shouldBe Nil
      cpg.typeDecl.fullNameExact(bazFullName).member.name.l should contain allOf ("outerClass", "barParam", "barLocal")
      cpg.typeDecl.fullNameExact(bazFullName).member.name.l should not contain "fooParam"
      cpg.typeDecl.fullNameExact(bazFullName).member.name.l should not contain "fooLocal"

      val List(fooCtor) = cpg.typeDecl.fullNameExact(fooFullName).method.nameExact("<init>").l: @unchecked
      fooCtor.parameter.name.l shouldBe List("this", "outerClass", "testParam", "testLocal")
      fooCtor.parameter.typeFullName.l shouldBe List(fooFullName, "demo.Test", "int", "int")

      val List(barCtor) = cpg.typeDecl.fullNameExact(barFullName).method.nameExact("<init>").l: @unchecked
      barCtor.parameter.name.l shouldBe List("this", "fooParam", "fooLocal")
      barCtor.parameter.typeFullName.l shouldBe List(barFullName, "int", "int")

      val List(bazCtor) = cpg.typeDecl.fullNameExact(bazFullName).method.nameExact("<init>").l: @unchecked
      bazCtor.parameter.name.l shouldBe List("this", "outerClass", "barParam", "barLocal")
      bazCtor.parameter.typeFullName.l shouldBe List(bazFullName, barFullName, "int", "int")

      val List(sinkCall) = cpg.method.fullNameExact(s"$bazFullName.baz:void()").call.nameExact("sink").l: @unchecked
      sinkCall.argument.code.l shouldBe List(
        "this.outerClass.fooParam",
        "this.outerClass.fooLocal",
        "this.outerClass.barMember",
        "this.barParam",
        "this.barLocal"
      )
    }

    "create method-local record declarations with canonical constructors and bindings" in {
      val cpg = code(
        """package demo;
          |
          |public class Foo {
          |  void enclosingMethod() {
          |    record LocalRecord(String value) {}
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val ownerFullName = "demo.Foo.enclosingMethod:void()"
      val localFullName = s"$ownerFullName.LocalRecord"

      val List(localDecl) = cpg.typeDecl.fullNameExact(localFullName).l: @unchecked
      localDecl.code shouldBe "record LocalRecord"
      localDecl.astParentFullName shouldBe ownerFullName
      localDecl.inheritsFromTypeFullName should contain("java.lang.Record")

      val List(valueMember) = localDecl.member.nameExact("value").l: @unchecked
      valueMember.typeFullName shouldBe "java.lang.String"
      valueMember.modifier.modifierType.toSet shouldBe Set(ModifierTypes.PRIVATE, ModifierTypes.FINAL)

      val List(accessor) = localDecl.method.nameExact("value").l: @unchecked
      accessor.fullName shouldBe s"$localFullName.value:java.lang.String()"
      accessor.body.astChildren.collectAll[Return].code.l shouldBe List("return this.value")

      val List(ctor) = localDecl.method.nameExact("<init>").signature("void\\(java.lang.String\\)").l: @unchecked
      ctor.fullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
      ctor.parameter.name.l shouldBe List("this", "value")
      ctor.parameter.typeFullName.l shouldBe List(localFullName, "java.lang.String")
      ctor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l shouldBe List("this.value = value")

      val initBindings = cpg.all.collectAll[Binding].nameExact("<init>").l.filter(_.methodFullName == ctor.fullName)
      inside(initBindings) { case List(initBinding) =>
        initBinding.signature shouldBe "void(java.lang.String)"
        initBinding.bindingTypeDecl.fullName shouldBe localFullName
      }
    }

    "create method-local compact record constructors with component assignments before the body" in {
      val cpg = code(
        """package demo;
          |
          |public class Foo {
          |  void enclosingMethod() {
          |    record LocalRecord(String value) {
          |      public LocalRecord {
          |        System.out.println(value);
          |      }
          |    }
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val localFullName = "demo.Foo.enclosingMethod:void().LocalRecord"
      val List(ctor)    = cpg.typeDecl.fullNameExact(localFullName).method.nameExact("<init>").l: @unchecked
      inside(ctor.body.astChildren.l) { case List(valueAssignment: Call, printlnCall: Call) =>
        valueAssignment.name shouldBe Operators.assignment
        valueAssignment.code shouldBe "this.value = value"
        printlnCall.name shouldBe "println"
        printlnCall.code shouldBe "System.out.println(value)"
      }
    }

    "create method-local record capture members and constructor parameters for visible locals" in {
      val cpg = code(
        """package demo;
          |
          |public class Foo {
          |  int capturedMember;
          |  static int staticMember;
          |
          |  void enclosingMethod(int capturedParam) {
          |    int capturedLocal = 1;
          |    record LocalRecord(String value) {
          |      LocalRecord() {
          |        this("delegated");
          |      }
          |      void usesCaptures() {
          |        sink(capturedParam, capturedLocal, capturedMember, staticMember, value);
          |      }
          |    }
          |    LocalRecord created = new LocalRecord("seed");
          |    created = new LocalRecord("next");
          |    new LocalRecord("call").usesCaptures();
          |    new LocalRecord("access").value();
          |    LocalRecord[] slots = new LocalRecord[1];
          |    slots[0] = new LocalRecord("array");
          |  }
          |}
          |""".stripMargin,
        "demo/Foo.java"
      ).withConfig(Config(parserBackend = JavaParserBackend.Oxidized, skipTypeInfPass = true))

      val localFullName   = "demo.Foo.enclosingMethod:void(int).LocalRecord"
      val List(localDecl) = cpg.typeDecl.fullNameExact(localFullName).l: @unchecked

      localDecl.member.nameExact("capturedParam").typeFullName.l shouldBe List("int")
      localDecl.member.nameExact("capturedLocal").typeFullName.l shouldBe List("int")
      localDecl.member.nameExact("capturedMember").l shouldBe Nil
      localDecl.member.nameExact("staticMember").l shouldBe Nil
      localDecl.member.nameExact("outerClass").l shouldBe Nil

      val List(ctor) = localDecl.method.nameExact("<init>").signature("void\\(java.lang.String\\)").l: @unchecked
      ctor.fullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
      ctor.parameter.name.l shouldBe List("this", "value", "capturedParam", "capturedLocal")
      ctor.parameter.typeFullName.l shouldBe List(localFullName, "java.lang.String", "int", "int")
      ctor.body.astChildren.collectAll[Call].nameExact(Operators.assignment).code.l should contain allOf (
        "this.value = value",
        "this.capturedParam = capturedParam",
        "this.capturedLocal = capturedLocal"
      )

      val List(delegatingCtor) = localDecl.method.nameExact("<init>").signature("void\\(\\)").l: @unchecked
      delegatingCtor.fullName shouldBe s"$localFullName.<init>:void()"
      delegatingCtor.parameter.name.l shouldBe List("this", "capturedParam", "capturedLocal")
      val List(thisCall) = delegatingCtor.body.astChildren
        .collectAll[Call]
        .nameExact("<init>")
        .codeExact("""this("delegated")""")
        .l: @unchecked
      thisCall.methodFullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
      thisCall.signature shouldBe "void(java.lang.String)"
      thisCall.argument.code.l shouldBe List("this", "\"delegated\"", "capturedParam", "capturedLocal")
      thisCall.argument.argumentIndex.l shouldBe List(0, 1, 2, 3)
      thisCall.argument.isIdentifier.nameExact("capturedParam").refsTo.l shouldBe delegatingCtor.parameter
        .nameExact("capturedParam")
        .l
      thisCall.argument.isIdentifier.nameExact("capturedLocal").refsTo.l shouldBe delegatingCtor.parameter
        .nameExact("capturedLocal")
        .l

      cpg.method.nameExact("enclosingMethod").local.nameExact("created").typeFullName.l shouldBe List(localFullName)
      val List(createdAssign) = cpg.method
        .nameExact("enclosingMethod")
        .body
        .astChildren
        .collectAll[Call]
        .nameExact(Operators.assignment)
        .l
        .filter(_.argument.code.l.contains("""new LocalRecord("seed")""")): @unchecked
      inside(createdAssign.argument.l) { case List(createdTarget: Identifier, alloc: Call) =>
        createdTarget.name shouldBe "created"
        createdTarget.refsTo.l shouldBe cpg.method.nameExact("enclosingMethod").local.nameExact("created").l
        alloc.name shouldBe Operators.alloc
        alloc.typeFullName shouldBe localFullName
        alloc.argument.l shouldBe Nil
      }

      val List(initCall) = cpg.method
        .nameExact("enclosingMethod")
        .ast
        .collectAll[Call]
        .nameExact("<init>")
        .codeExact("new LocalRecord(\"seed\")")
        .l: @unchecked
      initCall.methodFullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
      initCall.signature shouldBe "void(java.lang.String)"
      initCall.argument.code.l shouldBe List("created", "\"seed\"", "capturedParam", "capturedLocal")
      initCall.argument.argumentIndex.l shouldBe List(0, 1, 2, 3)
      initCall.argument.isIdentifier.nameExact("created").refsTo.l shouldBe cpg.method
        .nameExact("enclosingMethod")
        .local
        .nameExact("created")
        .l
      initCall.argument.isIdentifier.nameExact("capturedParam").refsTo.l shouldBe cpg.method
        .nameExact("enclosingMethod")
        .parameter
        .nameExact("capturedParam")
        .l
      initCall.argument.isIdentifier.nameExact("capturedLocal").refsTo.l shouldBe cpg.method
        .nameExact("enclosingMethod")
        .local
        .nameExact("capturedLocal")
        .l

      val List(reassignment) = cpg.method
        .nameExact("enclosingMethod")
        .body
        .astChildren
        .collectAll[Call]
        .nameExact(Operators.assignment)
        .codeExact("""created = new LocalRecord("next")""")
        .l: @unchecked
      inside(reassignment.argument.l) { case List(reassignedTarget: Identifier, alloc: Call) =>
        reassignedTarget.name shouldBe "created"
        reassignedTarget.refsTo.l shouldBe cpg.method.nameExact("enclosingMethod").local.nameExact("created").l
        alloc.name shouldBe Operators.alloc
        alloc.code shouldBe """new LocalRecord("next")"""
        alloc.typeFullName shouldBe localFullName
        alloc.argument.l shouldBe Nil
      }

      val List(reassignmentInitCall) = cpg.method
        .nameExact("enclosingMethod")
        .ast
        .collectAll[Call]
        .nameExact("<init>")
        .codeExact("new LocalRecord(\"next\")")
        .l: @unchecked
      reassignmentInitCall.methodFullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
      reassignmentInitCall.signature shouldBe "void(java.lang.String)"
      reassignmentInitCall.argument.code.l shouldBe List("created", "\"next\"", "capturedParam", "capturedLocal")
      reassignmentInitCall.argument.argumentIndex.l shouldBe List(0, 1, 2, 3)
      reassignmentInitCall.argument.isIdentifier.nameExact("created").refsTo.l shouldBe cpg.method
        .nameExact("enclosingMethod")
        .local
        .nameExact("created")
        .l
      reassignmentInitCall.argument.isIdentifier.nameExact("capturedLocal").refsTo.l shouldBe cpg.method
        .nameExact("enclosingMethod")
        .local
        .nameExact("capturedLocal")
        .l

      val List(receiverCall) = cpg.method
        .nameExact("enclosingMethod")
        .ast
        .collectAll[Call]
        .nameExact("usesCaptures")
        .codeExact("""new LocalRecord("call").usesCaptures()""")
        .l: @unchecked
      receiverCall.methodFullName shouldBe s"$localFullName.usesCaptures:void()"
      receiverCall.signature shouldBe "void()"
      receiverCall.typeFullName shouldBe "void"
      inside(receiverCall.receiver.l) { case List(constructorReceiverBlock: Block) =>
        constructorReceiverBlock.typeFullName shouldBe localFullName
        inside(constructorReceiverBlock.astChildren.l) {
          case List(tempLocal: Local, tempAssignment: Call, callInitCall: Call, returnedTemp: Identifier) =>
            tempLocal.name shouldBe "$obj0"
            tempLocal.typeFullName shouldBe localFullName

            inside(tempAssignment.argument.l) { case List(tempTarget: Identifier, alloc: Call) =>
              tempTarget.name shouldBe "$obj0"
              tempTarget.refsTo.l shouldBe List(tempLocal)
              alloc.name shouldBe Operators.alloc
              alloc.code shouldBe """new LocalRecord("call")"""
              alloc.typeFullName shouldBe localFullName
            }

            callInitCall.name shouldBe "<init>"
            callInitCall.methodFullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
            callInitCall.argument.code.l shouldBe List("$obj0", "\"call\"", "capturedParam", "capturedLocal")
            callInitCall.argument.isIdentifier.nameExact("capturedLocal").refsTo.l shouldBe cpg.method
              .nameExact("enclosingMethod")
              .local
              .nameExact("capturedLocal")
              .l

            returnedTemp.name shouldBe "$obj0"
            returnedTemp.refsTo.l shouldBe List(tempLocal)
        }
      }

      val List(accessorCall) = cpg.method
        .nameExact("enclosingMethod")
        .ast
        .collectAll[Call]
        .nameExact("value")
        .codeExact("""new LocalRecord("access").value()""")
        .l: @unchecked
      accessorCall.methodFullName shouldBe s"$localFullName.value:java.lang.String()"
      accessorCall.signature shouldBe "java.lang.String()"
      accessorCall.typeFullName shouldBe "java.lang.String"
      inside(accessorCall.receiver.l) { case List(accessorReceiverBlock: Block) =>
        accessorReceiverBlock.typeFullName shouldBe localFullName
        inside(accessorReceiverBlock.astChildren.l) {
          case List(tempLocal: Local, tempAssignment: Call, accessorInitCall: Call, returnedTemp: Identifier) =>
            tempLocal.name shouldBe "$obj1"
            tempLocal.typeFullName shouldBe localFullName

            inside(tempAssignment.argument.l) { case List(tempTarget: Identifier, alloc: Call) =>
              tempTarget.name shouldBe "$obj1"
              tempTarget.refsTo.l shouldBe List(tempLocal)
              alloc.name shouldBe Operators.alloc
              alloc.code shouldBe """new LocalRecord("access")"""
              alloc.typeFullName shouldBe localFullName
            }

            accessorInitCall.name shouldBe "<init>"
            accessorInitCall.methodFullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
            accessorInitCall.argument.code.l shouldBe List("$obj1", "\"access\"", "capturedParam", "capturedLocal")
            accessorInitCall.argument.isIdentifier.nameExact("capturedLocal").refsTo.l shouldBe cpg.method
              .nameExact("enclosingMethod")
              .local
              .nameExact("capturedLocal")
              .l

            returnedTemp.name shouldBe "$obj1"
            returnedTemp.refsTo.l shouldBe List(tempLocal)
        }
      }

      val List(complexAssignment) = cpg.method
        .nameExact("enclosingMethod")
        .body
        .astChildren
        .collectAll[Call]
        .nameExact(Operators.assignment)
        .codeExact("""slots[0] = new LocalRecord("array")""")
        .l: @unchecked
      inside(complexAssignment.argument.l) { case List(indexAccess: Call, constructorBlock: Block) =>
        indexAccess.name shouldBe Operators.indexAccess
        indexAccess.code shouldBe "slots[0]"
        constructorBlock.typeFullName shouldBe localFullName

        inside(constructorBlock.astChildren.l) {
          case List(tempLocal: Local, tempAssignment: Call, arrayInitCall: Call, returnedTemp: Identifier) =>
            tempLocal.name shouldBe "$obj2"
            tempLocal.typeFullName shouldBe localFullName

            inside(tempAssignment.argument.l) { case List(tempTarget: Identifier, alloc: Call) =>
              tempTarget.name shouldBe "$obj2"
              tempTarget.refsTo.l shouldBe List(tempLocal)
              alloc.name shouldBe Operators.alloc
              alloc.code shouldBe """new LocalRecord("array")"""
              alloc.typeFullName shouldBe localFullName
              alloc.argument.l shouldBe Nil
            }

            arrayInitCall.name shouldBe "<init>"
            arrayInitCall.methodFullName shouldBe s"$localFullName.<init>:void(java.lang.String)"
            arrayInitCall.signature shouldBe "void(java.lang.String)"
            arrayInitCall.argument.code.l shouldBe List("$obj2", "\"array\"", "capturedParam", "capturedLocal")
            arrayInitCall.argument.argumentIndex.l shouldBe List(0, 1, 2, 3)
            arrayInitCall.argument.isIdentifier.nameExact("capturedLocal").refsTo.l shouldBe cpg.method
              .nameExact("enclosingMethod")
              .local
              .nameExact("capturedLocal")
              .l

            returnedTemp.name shouldBe "$obj2"
            returnedTemp.refsTo.l shouldBe List(tempLocal)
        }
      }
    }
  }
}
