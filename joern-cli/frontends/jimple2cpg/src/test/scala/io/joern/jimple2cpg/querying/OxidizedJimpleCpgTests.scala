package io.joern.jimple2cpg.querying

import io.joern.jimple2cpg.{Config, JimpleParserBackend}
import io.joern.jimple2cpg.testfixtures.JimpleCode2CpgFixture
import io.shiftleft.codepropertygraph.generated.Operators
import io.shiftleft.codepropertygraph.generated.nodes.{
  Call,
  ControlStructure,
  FieldIdentifier,
  Identifier,
  JumpTarget,
  Literal,
  TypeRef,
  Unknown
}
import io.shiftleft.semanticcpg.language.*

class OxidizedJimpleCpgTests extends JimpleCode2CpgFixture {

  "oxidized class extraction backend" should {

    "feed compiled classes into the existing Jimple CPG pipeline" in {
      val cpg = code("""
          |package demo;
          |
          |class Foo {
          |  static int MAX_VALUE = 7;
          |  static final int FLAG = 1;
          |  static final String LABEL = "OK";
          |  int value;
          |
          |  Foo() {
          |    this.value = 1;
          |  }
          |
          |  int add(int x) {
          |    return x + 1;
          |  }
          |
          |  void noop() {
          |    return;
          |  }
          |
          |  static int triple(int x) {
          |    return x * 3;
          |  }
          |
          |  int callStatic(int x) {
          |    return Foo.triple(x);
          |  }
          |
          |  int callInstance(int x) {
          |    return this.add(x);
          |  }
          |
          |  Foo make() {
          |    return new Foo();
          |  }
          |
          |  int choose(int x) {
          |    if (x > 0) {
          |      return x;
          |    }
          |    return -x;
          |  }
          |
          |  int adjust(int x) {
          |    if (x > 0) {
          |      x = 1;
          |    } else {
          |      x = 2;
          |    }
          |    return x;
          |  }
          |
          |  String read(String[] vals, int i) {
          |    return vals[i];
          |  }
          |
          |  int lengthOf(int[] values) {
          |    return values.length;
          |  }
          |
          |  void write(String[] vals) {
          |    vals[1] = "MALICIOUS";
          |  }
          |
          |  int[] buildArray() {
          |    int[] values = new int[2];
          |    values[0] = 1;
          |    return values;
          |  }
          |
          |  int[] literalArray() {
          |    return new int[] {1, 2};
          |  }
          |
          |  String[] names(int count) {
          |    return new String[count];
          |  }
          |
          |  int[][] matrix(int rows, int cols) {
          |    return new int[rows][cols];
          |  }
          |
          |  boolean isString(Object value) {
          |    return value instanceof String;
          |  }
          |
          |  String asString(Object value) {
          |    return (String) value;
          |  }
          |
          |  Class<?> classLiteral() {
          |    return Foo.class;
          |  }
          |
          |  Class<?> primitiveClassLiteral() {
          |    return int.class;
          |  }
          |
          |  long widen(int x) {
          |    return (long) x;
          |  }
          |
          |  boolean less(double left, double right) {
          |    return left < right;
          |  }
          |
          |  int bitOps(int x, int y) {
          |    return ((x << 1) ^ (y & 3)) | (x >> 2) | (y >>> 1);
          |  }
          |
          |  int tableSwitch(int value) {
          |    switch (value) {
          |      case 0:
          |        return 10;
          |      case 1:
          |        return 11;
          |      case 2:
          |        return 12;
          |      default:
          |        return -1;
          |    }
          |  }
          |
          |  int lookupSwitch(int value) {
          |    switch (value) {
          |      case 7:
          |        return 70;
          |      case 1000:
          |        return 100;
          |      default:
          |        return -1;
          |    }
          |  }
          |
          |  int postLocal(int x) {
          |    return x++;
          |  }
          |
          |  int fieldPost(Foo other) {
          |    return other.value++;
          |  }
          |
          |  int arrayPost(int[] values, int i) {
          |    return values[i]++;
          |  }
          |
          |  Runnable lambda(String x) {
          |    return () -> System.out.println(x);
          |  }
          |
          |  String sync(String text) {
          |    synchronized (this) {
          |      text = text + "A";
          |    }
          |    return text;
          |  }
          |
          |  static void mayThrow() throws Exception {
          |    throw new Exception();
          |  }
          |
          |  String catchValue(String text) {
          |    try {
          |      Foo.mayThrow();
          |    } catch (Exception e) {
          |      return text;
          |    }
          |    return "SAFE";
          |  }
          |
          |  String withFinally(String text) {
          |    try {
          |      text = text + "T";
          |    } finally {
          |      text = text + "F";
          |    }
          |    return text;
          |  }
          |
          |  int readStatic() {
          |    return Foo.MAX_VALUE;
          |  }
          |
          |  int readField() {
          |    return this.value;
          |  }
          |
          |  void writeField(int value) {
          |    this.value = value;
          |  }
          |}
          |""".stripMargin).withConfig(Config(parserBackend = JimpleParserBackend.Oxidized))

      cpg.typeDecl.fullNameExact("demo.Foo").name.l shouldBe List("Foo")
      cpg.typeDecl.fullNameExact("demo.Foo").member.nameExact("FLAG").code.l shouldBe List("int FLAG = 1")
      cpg.typeDecl.fullNameExact("demo.Foo").member.nameExact("LABEL").code.l shouldBe List(
        "java.lang.String LABEL = \"OK\""
      )
      cpg.method.nameExact("add").fullName.l shouldBe List("demo.Foo.add:int(int)")
      cpg.method.nameExact("add").parameter.name.l should contain("x")
      cpg.method.nameExact("add").ast.isCall.nameExact(Operators.addition).code.l should contain("(x + 1)")
      cpg.method.nameExact("add").ast.isReturn.code.l should contain("return (x + 1);")
      cpg.method.nameExact("noop").ast.isReturn.code.l should contain("return;")
      cpg.method.nameExact("add").ast.collectAll[Unknown].l shouldBe empty
      val staticCall = cpg.method.nameExact("callStatic").ast.isCall.nameExact("triple").head
      staticCall.methodFullName shouldBe "demo.Foo.triple:int(int)"
      staticCall.signature shouldBe "int(int)"
      staticCall.typeFullName shouldBe "int"
      staticCall.dispatchType shouldBe "STATIC_DISPATCH"
      staticCall.argument(1).code shouldBe "x"

      val instanceCall = cpg.method.nameExact("callInstance").ast.isCall.nameExact("add").head
      instanceCall.methodFullName shouldBe "demo.Foo.add:int(int)"
      instanceCall.signature shouldBe "int(int)"
      instanceCall.typeFullName shouldBe "int"
      instanceCall.dispatchType shouldBe "DYNAMIC_DISPATCH"
      instanceCall.receiver.code.l shouldBe List("this")
      instanceCall.argument(1).code shouldBe "x"

      cpg.method.nameExact("make").local.nameExact("$stack1").typeFullName.l shouldBe List("demo.Foo")
      val makeAllocationAssignment = cpg.method
        .nameExact("make")
        .ast
        .isCall
        .nameExact(Operators.assignment)
        .codeExact("$stack1 = new demo.Foo")
        .head
      makeAllocationAssignment.typeFullName shouldBe "demo.Foo"
      val makeAllocation =
        cpg.method.nameExact("make").ast.isCall.nameExact(Operators.alloc).codeExact("new demo.Foo").head
      makeAllocation.methodFullName shouldBe Operators.alloc
      makeAllocation.dispatchType shouldBe "STATIC_DISPATCH"
      makeAllocation.typeFullName shouldBe "demo.Foo"
      makeAllocationAssignment.argument(2).asInstanceOf[Call].name shouldBe Operators.alloc
      val constructorCall = cpg.method.nameExact("make").ast.isCall.nameExact("<init>").head
      constructorCall.methodFullName shouldBe "demo.Foo.<init>:void()"
      constructorCall.signature shouldBe "void()"
      constructorCall.typeFullName shouldBe "void"
      constructorCall.dispatchType shouldBe "STATIC_DISPATCH"
      constructorCall.code shouldBe "$stack1.Foo()"
      constructorCall.receiver.code.l shouldBe List("$stack1")
      constructorCall.receiver.l.collect { case identifier: Identifier => identifier.typeFullName } shouldBe List(
        "demo.Foo"
      )
      cpg.method.nameExact("make").ast.isReturn.code.l should contain("return $stack1;")
      cpg.method
        .nameExact("make")
        .ast
        .isCall
        .nameExact(Operators.fieldAccess)
        .code("new demo.Foo.*")
        .l shouldBe empty

      cpg.method.nameExact("choose").ast.isCall.nameExact(Operators.lessEqualsThan).cfgOut.size should be > 1
      val adjustGoto = cpg.method
        .nameExact("adjust")
        .ast
        .collectAll[Unknown]
        .filter(_.code.startsWith("goto "))
        .head
      val adjustGotoTargetLine = adjustGoto.cfgOut.lineNumber.headOption.get
      adjustGoto.code shouldBe s"goto $adjustGotoTargetLine"
      cpg.method
        .nameExact("adjust")
        .ast
        .collectAll[ControlStructure]
        .filter(_.code.startsWith("goto "))
        .l shouldBe empty

      val readIndexAccess =
        cpg.method.nameExact("read").ast.isCall.nameExact(Operators.indexAccess).codeExact("vals[i]").head
      readIndexAccess.lineNumber should not be empty
      readIndexAccess.argument(1).code shouldBe "vals"
      readIndexAccess.argument(2).code shouldBe "i"

      val lengthOfCall = cpg.method.nameExact("lengthOf").ast.isCall.nameExact(Operators.lengthOf).head
      lengthOfCall.code shouldBe "values.length"
      lengthOfCall.typeFullName shouldBe "int"
      lengthOfCall.argument(1).code shouldBe "values"
      cpg.method.nameExact("lengthOf").ast.isReturn.code.l should contain("return values.length;")

      val writeAssignment =
        cpg.method
          .nameExact("write")
          .ast
          .isCall
          .nameExact(Operators.assignment)
          .codeExact("vals[1] = \"MALICIOUS\"")
          .head
      val writeIndexAccess = writeAssignment.argument(1).asInstanceOf[Call]
      writeAssignment.lineNumber should not be empty
      writeIndexAccess.name shouldBe Operators.indexAccess
      writeIndexAccess.code shouldBe "vals[1]"
      writeIndexAccess.lineNumber shouldBe writeAssignment.lineNumber
      writeIndexAccess.argument(1).code shouldBe "vals"
      writeIndexAccess.argument(2).code shouldBe "1"
      writeAssignment.argument(2).code shouldBe "\"MALICIOUS\""

      val buildArrayAssignment = cpg.method
        .nameExact("buildArray")
        .ast
        .isCall
        .nameExact(Operators.assignment)
        .codeExact("values = new int[2]")
        .head
      buildArrayAssignment.typeFullName shouldBe "int[]"
      val buildArrayAlloc = buildArrayAssignment.argument(2).asInstanceOf[Call]
      buildArrayAlloc.name shouldBe Operators.alloc
      buildArrayAlloc.methodFullName shouldBe Operators.alloc
      buildArrayAlloc.typeFullName shouldBe "int[]"
      buildArrayAlloc.argument(1).code shouldBe "2"

      val literalArrayMethod = cpg.method.nameExact("literalArray")
      literalArrayMethod.local.nameExact("$stack1").typeFullName.l shouldBe List("int[]")

      val namesAlloc = cpg.method
        .nameExact("names")
        .ast
        .isCall
        .nameExact(Operators.alloc)
        .codeExact("new java.lang.String[count]")
        .head
      namesAlloc.typeFullName shouldBe "java.lang.String[]"
      namesAlloc.argument(1).code shouldBe "count"

      val matrixAlloc =
        cpg.method.nameExact("matrix").ast.isCall.nameExact(Operators.alloc).codeExact("new int[rows][cols]").head
      matrixAlloc.typeFullName shouldBe "int[][]"
      matrixAlloc.argument(1).code shouldBe "rows"
      matrixAlloc.argument(2).code shouldBe "cols"

      val instanceOfCall =
        cpg.method.nameExact("isString").ast.isCall.nameExact(Operators.instanceOf).head
      instanceOfCall.typeFullName shouldBe "boolean"
      instanceOfCall.argument(1).code shouldBe "value"
      val instanceOfType = instanceOfCall.argument(2).asInstanceOf[TypeRef]
      instanceOfType.code shouldBe "String"
      instanceOfType.typeFullName shouldBe "java.lang.String"

      val castCall = cpg.method.nameExact("asString").ast.isCall.nameExact(Operators.cast).head
      castCall.typeFullName shouldBe "java.lang.String"
      val castType = castCall.argument(1).asInstanceOf[TypeRef]
      castType.code shouldBe "String"
      castType.typeFullName shouldBe "java.lang.String"
      castCall.argument(2).code shouldBe "value"

      val objectClassLiteral = cpg.method.nameExact("classLiteral").ast.collectAll[Literal].head
      objectClassLiteral.code shouldBe "demo.Foo.class"
      objectClassLiteral.typeFullName shouldBe "java.lang.Class"
      cpg.method.nameExact("classLiteral").ast.isReturn.code.l should contain("return demo.Foo.class;")

      val primitiveClassLiteral = cpg.method.nameExact("primitiveClassLiteral").ast.collectAll[Literal].head
      primitiveClassLiteral.code shouldBe "int.class"
      primitiveClassLiteral.typeFullName shouldBe "java.lang.Class"
      cpg.method.nameExact("primitiveClassLiteral").ast.isReturn.code.l should contain("return int.class;")

      val widenCast = cpg.method.nameExact("widen").ast.isCall.nameExact(Operators.cast).head
      widenCast.code shouldBe "(long) x"
      widenCast.typeFullName shouldBe "long"
      val widenCastType = widenCast.argument(1).asInstanceOf[TypeRef]
      widenCastType.code shouldBe "long"
      widenCastType.typeFullName shouldBe "long"
      widenCast.argument(2).code shouldBe "x"

      val compareCall = cpg.method.nameExact("less").ast.isCall.nameExact(Operators.compare).head
      compareCall.typeFullName shouldBe "int"
      compareCall.argument(1).code shouldBe "left"
      compareCall.argument(2).code shouldBe "right"

      cpg.method.nameExact("bitOps").ast.isCall.nameExact(Operators.shiftLeft).code.l should contain("(x << 1)")
      cpg.method.nameExact("bitOps").ast.isCall.nameExact(Operators.and).code.l should contain("(y & 3)")
      cpg.method.nameExact("bitOps").ast.isCall.nameExact(Operators.xor).code.l should contain("((x << 1) ^ (y & 3))")
      cpg.method.nameExact("bitOps").ast.isCall.nameExact(Operators.logicalShiftRight).code.l should contain("(x >> 2)")
      cpg.method.nameExact("bitOps").ast.isCall.nameExact(Operators.arithmeticShiftRight).code.l should contain(
        "(y >>> 1)"
      )
      cpg.method.nameExact("bitOps").ast.isCall.nameExact(Operators.or).code.l should contain allOf (
        "(((x << 1) ^ (y & 3)) | (x >> 2))",
        "((((x << 1) ^ (y & 3)) | (x >> 2)) | (y >>> 1))"
      )

      cpg.method.nameExact("tableSwitch").switchBlock.code.l should contain("tableswitch(value)")
      cpg.method.nameExact("tableSwitch").switchBlock.condition.code.l should contain("value")
      cpg.method
        .nameExact("tableSwitch")
        .switchBlock
        .astChildren
        .collectAll[JumpTarget]
        .name
        .toSet should contain allOf ("default", "case 0", "case 1", "case 2")

      cpg.method.nameExact("lookupSwitch").switchBlock.code.l should contain("lookupswitch(value)")
      cpg.method.nameExact("lookupSwitch").switchBlock.condition.code.l should contain("value")
      cpg.method
        .nameExact("lookupSwitch")
        .switchBlock
        .astChildren
        .collectAll[JumpTarget]
        .name
        .toSet should contain allOf ("default", "case 7", "case 1000")

      cpg.method.nameExact("postLocal").local.nameExact("$stack1").typeFullName.l shouldBe List("int")
      cpg.method
        .nameExact("postLocal")
        .ast
        .isCall
        .nameExact(Operators.addition)
        .code
        .l should contain("(x + 1)")
      cpg.method.nameExact("postLocal").ast.isCall.nameExact(Operators.assignment).code.l should contain allOf (
        "$stack1 = x",
        "x = (x + 1)"
      )
      cpg.method.nameExact("postLocal").ast.isReturn.code.l should contain("return $stack1;")
      cpg.method.nameExact("postLocal").ast.collectAll[Unknown].l shouldBe empty

      cpg.method.nameExact("fieldPost").local.nameExact("$stack1").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("fieldPost").ast.isCall.nameExact(Operators.assignment).code.l should contain allOf (
        "$stack1 = other.value",
        "other.value = ($stack1 + 1)"
      )
      cpg.method
        .nameExact("fieldPost")
        .ast
        .isCall
        .nameExact(Operators.addition)
        .code
        .l should contain("($stack1 + 1)")
      cpg.method.nameExact("fieldPost").ast.isReturn.code.l should contain("return $stack1;")
      cpg.method.nameExact("fieldPost").ast.collectAll[Unknown].l shouldBe empty

      cpg.method.nameExact("arrayPost").local.nameExact("$stack1").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("arrayPost").ast.isCall.nameExact(Operators.assignment).code.l should contain allOf (
        "$stack1 = values[i]",
        "values[i] = ($stack1 + 1)"
      )
      cpg.method
        .nameExact("arrayPost")
        .ast
        .isCall
        .nameExact(Operators.indexAccess)
        .codeExact("values[i]")
        .typeFullName
        .l
        .toSet should contain("int")
      cpg.method
        .nameExact("arrayPost")
        .ast
        .isCall
        .nameExact(Operators.addition)
        .code
        .l should contain("($stack1 + 1)")
      cpg.method.nameExact("arrayPost").ast.isReturn.code.l should contain("return $stack1;")
      cpg.method.nameExact("arrayPost").ast.collectAll[Unknown].l shouldBe empty

      val lambdaDynamic = cpg.method.nameExact("lambda").ast.isCall.nameExact("run").head
      lambdaDynamic.dispatchType shouldBe "DYNAMIC_DISPATCH"
      lambdaDynamic.signature shouldBe "java.lang.Runnable(java.lang.String)"
      lambdaDynamic.typeFullName shouldBe "java.lang.Runnable"
      lambdaDynamic.code shouldBe "run(x)"
      lambdaDynamic.argument.code.l should contain("x")
      lambdaDynamic.argument.code.l.exists(_.contains("lambda$lambda$0")) shouldBe true

      val explicitThrow = cpg.method.nameExact("mayThrow").ast.isCall.nameExact("<operator>.throw").head
      explicitThrow.code shouldBe "throw new java.lang.Exception()"
      explicitThrow.argument(1).code shouldBe "$stack1"
      explicitThrow.argument(1).asInstanceOf[Identifier].typeFullName shouldBe "java.lang.Exception"

      val List(enterMonitor, exitMonitor, exceptionalExitMonitor) =
        cpg.method.nameExact("sync").ast.collectAll[Unknown].filter(_.code.contains("monitor")).l: @unchecked
      enterMonitor.code shouldBe "entermonitor this"
      enterMonitor.astChildren.collectAll[Identifier].code.l shouldBe List("this")
      enterMonitor._argumentOut.collectAll[Identifier].argumentIndex(1).code.l shouldBe List("this")
      exitMonitor.code shouldBe "exitmonitor l2"
      exitMonitor.astChildren.collectAll[Identifier].code.l shouldBe List("l2")
      exitMonitor._argumentOut.collectAll[Identifier].argumentIndex(1).code.l shouldBe List("l2")
      exceptionalExitMonitor.code shouldBe "exitmonitor l2"
      exceptionalExitMonitor.astChildren.collectAll[Identifier].code.l shouldBe List("l2")
      exceptionalExitMonitor._argumentOut.collectAll[Identifier].argumentIndex(1).code.l shouldBe List("l2")

      val mayThrowCall = cpg.method.nameExact("catchValue").ast.isCall.nameExact("mayThrow").head
      mayThrowCall.cfgOut.code.l should contain("e = @caughtexception")
      val catchAssignment = cpg.method
        .nameExact("catchValue")
        .ast
        .isCall
        .nameExact(Operators.assignment)
        .codeExact("e = @caughtexception")
        .head
      catchAssignment.argument(1).code shouldBe "e"
      catchAssignment.argument(2).code shouldBe "@caughtexception"

      cpg.method.nameExact("withFinally").local.nameExact("l2").typeFullName.l shouldBe List("java.lang.Throwable")
      val finallyHandlerAssignment = cpg.method
        .nameExact("withFinally")
        .ast
        .isCall
        .nameExact(Operators.assignment)
        .codeExact("l2 = @caughtexception")
        .head
      finallyHandlerAssignment.typeFullName shouldBe "java.lang.Throwable"
      finallyHandlerAssignment.argument(1).code shouldBe "l2"
      finallyHandlerAssignment.argument(1).asInstanceOf[Identifier].typeFullName shouldBe "java.lang.Throwable"
      finallyHandlerAssignment.argument(2).code shouldBe "@caughtexception"
      finallyHandlerAssignment.argument(2).asInstanceOf[Identifier].typeFullName shouldBe "java.lang.Throwable"
      val finallyThrow = cpg.method.nameExact("withFinally").ast.isCall.nameExact("<operator>.throw").head
      finallyThrow.code shouldBe "throw new java.lang.Throwable()"
      finallyThrow.argument(1).code shouldBe "l2"
      finallyThrow.argument(1).asInstanceOf[Identifier].typeFullName shouldBe "java.lang.Throwable"

      val readStaticFieldAccess = cpg.method
        .nameExact("readStatic")
        .ast
        .isCall
        .nameExact(Operators.fieldAccess)
        .l
        .find(_.argument(2).code == "MAX_VALUE")
        .get
      readStaticFieldAccess.argument(1).code.endsWith("Foo") shouldBe true
      readStaticFieldAccess.argument(2).asInstanceOf[FieldIdentifier].canonicalName shouldBe "MAX_VALUE"

      val readFieldAccess =
        cpg.method.nameExact("readField").ast.isCall.nameExact(Operators.fieldAccess).codeExact("this.value").head
      readFieldAccess.argument(1).code shouldBe "this"
      readFieldAccess.argument(2).asInstanceOf[FieldIdentifier].canonicalName shouldBe "value"

      val writeFieldAssignment =
        cpg.method
          .nameExact("writeField")
          .ast
          .isCall
          .nameExact(Operators.assignment)
          .codeExact("this.value = value")
          .head
      val writeFieldAccess = writeFieldAssignment.argument(1).asInstanceOf[Call]
      writeFieldAccess.name shouldBe Operators.fieldAccess
      writeFieldAccess.argument(1).code shouldBe "this"
      writeFieldAccess.argument(2).asInstanceOf[FieldIdentifier].canonicalName shouldBe "value"
      writeFieldAssignment.argument(2).code shouldBe "value"
    }
  }
}
