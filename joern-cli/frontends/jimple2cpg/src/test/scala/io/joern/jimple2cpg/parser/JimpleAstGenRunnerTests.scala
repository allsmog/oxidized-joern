package io.joern.jimple2cpg.parser

import io.joern.jimple2cpg.Config
import io.joern.jimple2cpg.parser.JimpleAstGenRunner.*
import io.joern.jimple2cpg.testfixtures.JimpleCodeToCpgFixture
import io.shiftleft.codepropertygraph.generated.{DispatchTypes, Operators, PropertyNames}
import io.shiftleft.codepropertygraph.generated.nodes.{
  NewBlock,
  NewCall,
  NewControlStructure,
  NewIdentifier,
  NewJumpTarget,
  NewLiteral,
  NewLocal,
  NewReturn,
  NewTypeRef,
  NewUnknown
}
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.FileUtil.*
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

import java.nio.file.{Files, Path}
import java.util.jar.{JarEntry, JarOutputStream}

class JimpleAstGenRunnerTests extends AnyWordSpec with Matchers {

  "JimpleAstGenRunner" should {

    "extract compiled class files into package layout" in {
      FileUtil.usingTemporaryDirectory("jimpleastgenTestInput") { inputDir =>
        val javaFile = inputDir / "Foo.java"
        writeFile(
          javaFile,
          """package demo;
            |public class Foo {
            |  public int add(int x) { return x + 1; }
            |}
            |""".stripMargin
        )
        JimpleCodeToCpgFixture.compileJava(inputDir, List(javaFile.toFile))

        FileUtil.usingTemporaryDirectory("jimpleastgenTestOut") { outputDir =>
          val config = Config().withInputPath(inputDir.toString).withOutputPath(outputDir.toString)
          val result = new JimpleAstGenRunner(config).execute(outputDir)

          result.classFiles.flatMap(_.fullyQualifiedClassName) should contain("demo.Foo")
          Files.isRegularFile(outputDir / "demo" / "Foo.class") shouldBe true
          Files.isRegularFile(outputDir / "manifest.json") shouldBe true
        }
      }
    }

    "extract compiled class files from jars" in {
      FileUtil.usingTemporaryDirectory("jimpleastgenJarInput") { inputDir =>
        val javaFile = inputDir / "Foo.java"
        writeFile(
          javaFile,
          """package archive.demo;
            |public class Foo {
            |  public String id(String value) { return value; }
            |}
            |""".stripMargin
        )
        JimpleCodeToCpgFixture.compileJava(inputDir, List(javaFile.toFile))
        val jarFile = inputDir / "sample.jar"
        writeJar(jarFile, inputDir / "archive" / "demo" / "Foo.class", "archive/demo/Foo.class")

        FileUtil.usingTemporaryDirectory("jimpleastgenJarOut") { outputDir =>
          val config = Config().withInputPath(jarFile.toString).withOutputPath(outputDir.toString)
          val result = new JimpleAstGenRunner(config).execute(outputDir)

          result.classFiles.flatMap(_.fullyQualifiedClassName) should contain("archive.demo.Foo")
          Files.isRegularFile(outputDir / "archive" / "demo" / "Foo.class") shouldBe true
        }
      }
    }

    "decode declaration metadata from the manifest" in {
      FileUtil.usingTemporaryDirectory("jimpleastgenMetadataInput") { inputDir =>
        val javaFile = inputDir / "Foo.java"
        writeFile(
          javaFile,
          """package metadata.demo;
            |public class Foo implements java.io.Serializable {
            |  public static final int MAGIC = 7;
            |  public static final String TITLE = "demo";
            |  private String name;
            |  private int count;
            |
            |  public Foo(String name) {
            |    this.name = name;
            |  }
            |
            |  public Foo copy() {
            |    return new Foo(name);
            |  }
            |
            |  public void noop() {
            |    return;
            |  }
            |
            |  public int[] numbers() {
            |    return new int[] {1, 2};
            |  }
            |
            |  public String[] names(int count) {
            |    return new String[count];
            |  }
            |
            |  public int[][] matrix(int rows, int cols) {
            |    return new int[rows][cols];
            |  }
            |
            |  public boolean isString(Object value) {
            |    return value instanceof String;
            |  }
            |
            |  public String asString(Object value) {
            |    return (String) value;
            |  }
            |
            |  public Class<?> classLiteral() {
            |    return Foo.class;
            |  }
            |
            |  public Class<?> primitiveClassLiteral() {
            |    return int.class;
            |  }
            |
            |  public long widen(int x) {
            |    return (long) x;
            |  }
            |
            |  public boolean less(double left, double right) {
            |    return left < right;
            |  }
            |
            |  public int bitOps(int x, int y) {
            |    return ((x << 1) ^ (y & 3)) | (x >> 2) | (y >>> 1);
            |  }
            |
            |  public int tableSwitch(int value) {
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
            |  public int lookupSwitch(int value) {
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
            |  public int postLocal(int x) {
            |    return x++;
            |  }
            |
            |  public int fieldPost(Foo other) {
            |    return other.count++;
            |  }
            |
            |  public int arrayPost(int[] values, int i) {
            |    return values[i]++;
            |  }
            |
            |  public Runnable lambda(String x) {
            |    return () -> System.out.println(x);
            |  }
            |
            |  public String sync(String text) {
            |    synchronized (this) {
            |      text = text + name;
            |    }
            |    return text;
            |  }
            |
            |  public static void mayThrow() throws Exception {
            |    throw new Exception();
            |  }
            |
            |  public String catchValue(String text) {
            |    try {
            |      mayThrow();
            |    } catch (Exception e) {
            |      return text;
            |    }
            |    return "SAFE";
            |  }
            |
            |  public String withFinally(String text) {
            |    try {
            |      text = text + "T";
            |    } finally {
            |      text = text + "F";
            |    }
            |    return text;
            |  }
            |
            |  public String label(int[] values, java.util.List<String> labels) {
            |    return name + values.length + labels.size();
            |  }
            |
            |  public int choose(int x) {
            |    if (x > 0) {
            |      return x;
            |    }
            |    return -x;
            |  }
            |
            |  public int adjust(int x) {
            |    if (x > 0) {
            |      x = 1;
            |    } else {
            |      x = 2;
            |    }
            |    return x;
            |  }
            |}
            |""".stripMargin
        )
        JimpleCodeToCpgFixture.compileJava(inputDir, List(javaFile.toFile))

        FileUtil.usingTemporaryDirectory("jimpleastgenMetadataOut") { outputDir =>
          val config = Config().withInputPath(inputDir.toString).withOutputPath(outputDir.toString)
          val result = new JimpleAstGenRunner(config).execute(outputDir)
          val foo    = result.classInfo.find(_.fullyQualifiedName == "metadata.demo.Foo").get

          foo.internalName shouldBe "metadata/demo/Foo"
          foo.superFullyQualifiedName shouldBe Some("java.lang.Object")
          foo.interfaces.map(_.fullyQualifiedName) should contain("java.io.Serializable")
          foo.accessFlagsText should contain("public")
          foo.sourceFile shouldBe Some("Foo.java")

          val magic = foo.fields.find(_.name == "MAGIC").get
          magic.descriptor shouldBe "I"
          magic.typeName shouldBe Some("int")
          magic.accessFlagsText should contain allOf ("public", "static", "final")
          magic.constantValue shouldBe Some("7")

          val title = foo.fields.find(_.name == "TITLE").get
          title.descriptor shouldBe "Ljava/lang/String;"
          title.typeName shouldBe Some("java.lang.String")
          title.accessFlagsText should contain allOf ("public", "static", "final")
          title.constantValue shouldBe Some("\"demo\"")

          val name = foo.fields.find(_.name == "name").get
          name.descriptor shouldBe "Ljava/lang/String;"
          name.typeName shouldBe Some("java.lang.String")
          name.accessFlagsText should contain("private")
          name.constantValue shouldBe None

          val label = foo.methods.find(_.name == "label").get
          label.descriptor shouldBe "([ILjava/util/List;)Ljava/lang/String;"
          label.parameterTypes shouldBe List("int[]", "java.util.List")
          label.returnType shouldBe Some("java.lang.String")
          label.accessFlagsText should contain("public")
          val labelCode = label.code.get
          labelCode.maxLocals should be >= 3
          labelCode.bytecodeLength should be > 0L
          labelCode.instructions.map(_.mnemonic) should contain allOf ("getfield", "arraylength", "areturn")
          val getFieldRef = labelCode.instructions
            .find(_.mnemonic == "getfield")
            .flatMap(_.operands.find(_.kind == "constant_pool"))
            .flatMap(_.resolved)
            .get
          getFieldRef.tag shouldBe "Fieldref"
          getFieldRef.classReference.map(_.fullyQualifiedName) shouldBe Some("metadata.demo.Foo")
          getFieldRef.name shouldBe Some("name")
          getFieldRef.descriptor shouldBe Some("Ljava/lang/String;")
          getFieldRef.fieldType shouldBe Some("java.lang.String")
          labelCode.bodyIr.map(_.operation) should contain allOf ("field_load", "array_length", "call", "return")
          labelCode.bodyIr.find(_.operation == "field_load").map(_.code) should contain("this.name")
          val lengthIr = labelCode.bodyIr.find(_.operation == "array_length").get
          lengthIr.code shouldBe "values.length"
          lengthIr.target shouldBe Some("int")
          val sizeCall =
            labelCode.bodyIr.find(entry => entry.operation == "call" && entry.target.contains("labels.size")).get
          sizeCall.methodFullName shouldBe Some("java.util.List.size:int()")
          sizeCall.signature shouldBe Some("int()")
          sizeCall.dispatchType shouldBe Some("DYNAMIC_DISPATCH")
          sizeCall.receiver shouldBe Some("labels")

          val copy = foo.methods.find(_.name == "copy").get
          copy.returnType shouldBe Some("metadata.demo.Foo")
          val copyCode   = copy.code.get
          val allocation = copyCode.bodyIr.find(_.operation == "alloc").get
          allocation.code shouldBe "new metadata.demo.Foo"
          allocation.result shouldBe Some("$stack1")
          allocation.target shouldBe Some("metadata.demo.Foo")
          allocation.arguments shouldBe empty
          val constructorCall =
            copyCode.bodyIr
              .find(entry => entry.operation == "call" && entry.methodFullName.exists(_.contains(".<init>:")))
              .get
          constructorCall.methodFullName shouldBe Some("metadata.demo.Foo.<init>:void(java.lang.String)")
          constructorCall.signature shouldBe Some("void(java.lang.String)")
          constructorCall.dispatchType shouldBe Some(DispatchTypes.STATIC_DISPATCH)
          constructorCall.receiver shouldBe Some("$stack1")
          val copyBodyAst = JimpleBodyIrAstBuilder.methodBodyAst(copy)
          val copyCalls   = copyBodyAst.nodes.collect { case call: NewCall => call }
          copyBodyAst.nodes.collect { case local: NewLocal => local.name -> local.typeFullName }.toSet should contain(
            "$stack1" -> "metadata.demo.Foo"
          )
          val copyAllocAssign = copyCalls
            .find(call => call.name == Operators.assignment && call.code == "$stack1 = new metadata.demo.Foo")
            .get
          copyAllocAssign.typeFullName shouldBe "metadata.demo.Foo"
          val copyAlloc = copyCalls
            .find(call => call.name == Operators.alloc && call.code == "new metadata.demo.Foo")
            .get
          copyAlloc.typeFullName shouldBe "metadata.demo.Foo"
          copyCalls.exists(call =>
            call.name == Operators.fieldAccess && call.code.startsWith("new metadata.demo.Foo")
          ) shouldBe false
          val copyInit = copyCalls.find(_.name == "<init>").get
          copyInit.methodFullName shouldBe "metadata.demo.Foo.<init>:void(java.lang.String)"
          copyInit.signature shouldBe "void(java.lang.String)"
          copyInit.typeFullName shouldBe "void"
          copyInit.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          copyInit.code shouldBe "$stack1.Foo(this.name)"
          copyBodyAst.nodes.collect { case ret: NewReturn => ret.code } should contain("return $stack1;")
          copyBodyAst.nodes.collect { case unknown: NewUnknown => unknown.code } shouldBe empty

          val noop = foo.methods.find(_.name == "noop").get
          noop.returnType shouldBe Some("void")
          noop.code.get.bodyIr.find(_.operation == "return").map(_.code) should contain("return")
          JimpleBodyIrAstBuilder
            .methodBodyAst(noop)
            .nodes
            .collect { case ret: NewReturn => ret.code } should contain("return;")

          val numbers = foo.methods.find(_.name == "numbers").get
          numbers.returnType shouldBe Some("int[]")
          val numbersCode       = numbers.code.get
          val numbersAllocation = numbersCode.bodyIr.find(_.operation == "alloc_array").get
          numbersAllocation.code shouldBe "new int[2]"
          numbersAllocation.result shouldBe Some("$stack1")
          numbersAllocation.target shouldBe Some("int[]")
          numbersAllocation.arguments shouldBe List("2")
          val numbersStore = numbersCode.bodyIr.find(_.operation == "array_store").get
          numbersStore.code shouldBe "$stack1[0] = 1"
          val numbersBodyAst = JimpleBodyIrAstBuilder.methodBodyAst(numbers)
          val numbersCalls   = numbersBodyAst.nodes.collect { case call: NewCall => call }
          numbersBodyAst.nodes.collect { case local: NewLocal =>
            local.name -> local.typeFullName
          }.toSet should contain("$stack1" -> "int[]")
          val numbersAllocAssign = numbersCalls
            .find(call => call.name == Operators.assignment && call.code == "$stack1 = new int[2]")
            .get
          numbersAllocAssign.typeFullName shouldBe "int[]"
          val numbersAlloc = numbersCalls.find(call => call.name == Operators.alloc && call.code == "new int[2]").get
          numbersAlloc.typeFullName shouldBe "int[]"
          numbersBodyAst.nodes.collect { case literal: NewLiteral => literal.code } should contain("2")

          val names = foo.methods.find(_.name == "names").get
          names.returnType shouldBe Some("java.lang.String[]")
          val namesCode       = names.code.get
          val namesAllocation = namesCode.bodyIr.find(_.operation == "alloc_array").get
          namesAllocation.code shouldBe "new java.lang.String[count]"
          namesAllocation.result shouldBe Some("new java.lang.String[count]")
          namesAllocation.target shouldBe Some("java.lang.String[]")
          namesAllocation.arguments shouldBe List("count")
          val namesBodyAst = JimpleBodyIrAstBuilder.methodBodyAst(names)
          val namesAlloc = namesBodyAst.nodes
            .collect { case call: NewCall => call }
            .find(call => call.name == Operators.alloc && call.code == "new java.lang.String[count]")
            .get
          namesAlloc.typeFullName shouldBe "java.lang.String[]"

          val matrix = foo.methods.find(_.name == "matrix").get
          matrix.returnType shouldBe Some("int[][]")
          val matrixCode       = matrix.code.get
          val matrixAllocation = matrixCode.bodyIr.find(_.operation == "alloc_array").get
          matrixAllocation.code shouldBe "new int[rows][cols]"
          matrixAllocation.result shouldBe Some("new int[rows][cols]")
          matrixAllocation.target shouldBe Some("int[][]")
          matrixAllocation.arguments shouldBe List("rows", "cols")
          val matrixBodyAst = JimpleBodyIrAstBuilder.methodBodyAst(matrix)
          val matrixAlloc = matrixBodyAst.nodes
            .collect { case call: NewCall => call }
            .find(call => call.name == Operators.alloc && call.code == "new int[rows][cols]")
            .get
          matrixAlloc.typeFullName shouldBe "int[][]"

          val isString     = foo.methods.find(_.name == "isString").get
          val isStringCode = isString.code.get
          val typeCheck    = isStringCode.bodyIr.find(_.operation == "type_check").get
          typeCheck.code shouldBe "instanceof(value, java.lang.String)"
          typeCheck.target shouldBe Some("java.lang.String")
          typeCheck.arguments shouldBe List("value")
          val isStringCalls =
            JimpleBodyIrAstBuilder.methodBodyAst(isString).nodes.collect { case call: NewCall => call }
          val instanceOf = isStringCalls.find(_.name == Operators.instanceOf).get
          instanceOf.typeFullName shouldBe "boolean"
          val instanceOfTypeRefs =
            JimpleBodyIrAstBuilder.methodBodyAst(isString).nodes.collect { case typeRef: NewTypeRef => typeRef }
          instanceOfTypeRefs.map(typeRef => typeRef.code -> typeRef.typeFullName) should contain(
            "String" -> "java.lang.String"
          )

          val asString = foo.methods.find(_.name == "asString").get
          val cast     = asString.code.get.bodyIr.find(_.operation == "cast").get
          cast.code shouldBe "checkcast(value, java.lang.String)"
          cast.target shouldBe Some("java.lang.String")
          cast.arguments shouldBe List("value")
          val asStringBodyAst = JimpleBodyIrAstBuilder.methodBodyAst(asString)
          val castCall = asStringBodyAst.nodes.collect { case call: NewCall => call }.find(_.name == Operators.cast).get
          castCall.typeFullName shouldBe "java.lang.String"
          asStringBodyAst.nodes.collect { case typeRef: NewTypeRef =>
            typeRef.code -> typeRef.typeFullName
          } should contain("String" -> "java.lang.String")

          val classLiteral  = foo.methods.find(_.name == "classLiteral").get
          val classConstant = classLiteral.code.get.bodyIr.find(_.operation == "constant").get
          classConstant.code shouldBe "metadata.demo.Foo.class"
          classConstant.target shouldBe Some("java.lang.Class")
          JimpleBodyIrAstBuilder
            .methodBodyAst(classLiteral)
            .nodes
            .collect { case literal: NewLiteral => literal.code -> literal.typeFullName } should contain(
            "metadata.demo.Foo.class" -> "java.lang.Class"
          )

          val primitiveClassLiteral  = foo.methods.find(_.name == "primitiveClassLiteral").get
          val primitiveClassConstant = primitiveClassLiteral.code.get.bodyIr.find(_.operation == "constant").get
          primitiveClassConstant.code shouldBe "int.class"
          primitiveClassConstant.target shouldBe Some("java.lang.Class")
          JimpleBodyIrAstBuilder
            .methodBodyAst(primitiveClassLiteral)
            .nodes
            .collect { case literal: NewLiteral => literal.code -> literal.typeFullName } should contain(
            "int.class" -> "java.lang.Class"
          )

          val widen         = foo.methods.find(_.name == "widen").get
          val primitiveCast = widen.code.get.bodyIr.find(_.operation == "cast").get
          primitiveCast.code shouldBe "(long) x"
          primitiveCast.target shouldBe Some("long")
          primitiveCast.arguments shouldBe List("x")
          val widenBodyAst = JimpleBodyIrAstBuilder.methodBodyAst(widen, excludedLocalNames = Set("x"))
          val widenCast = widenBodyAst.nodes.collect { case call: NewCall => call }.find(_.name == Operators.cast).get
          widenCast.typeFullName shouldBe "long"
          widenBodyAst.nodes.collect { case typeRef: NewTypeRef =>
            typeRef.code -> typeRef.typeFullName
          } should contain("long" -> "long")

          val less    = foo.methods.find(_.name == "less").get
          val compare = less.code.get.bodyIr.find(_.operation == "compare").get
          compare.arguments shouldBe List("left", "right")
          val compareCall =
            JimpleBodyIrAstBuilder
              .methodBodyAst(less)
              .nodes
              .collect { case call: NewCall => call }
              .find(_.name == Operators.compare)
              .get
          compareCall.typeFullName shouldBe "int"

          val bitOps     = foo.methods.find(_.name == "bitOps").get
          val bitOpsCode = bitOps.code.get
          bitOpsCode.bodyIr.map(_.operation) should contain allOf ("binary", "return")
          bitOpsCode.bodyIr.map(_.code) should contain allOf (
            "(x << 1)",
            "(y & 3)",
            "((x << 1) ^ (y & 3))",
            "(x >> 2)",
            "(((x << 1) ^ (y & 3)) | (x >> 2))",
            "(y >>> 1)"
          )
          val bitOpsCalls =
            JimpleBodyIrAstBuilder.methodBodyAst(bitOps, excludedLocalNames = Set("x", "y")).nodes.collect {
              case call: NewCall => call
            }
          bitOpsCalls.map(_.name) should contain allOf (
            Operators.shiftLeft,
            Operators.and,
            Operators.xor,
            Operators.logicalShiftRight,
            Operators.arithmeticShiftRight,
            Operators.or
          )

          val tableSwitchCode = foo.methods.find(_.name == "tableSwitch").flatMap(_.code).get
          tableSwitchCode.instructions.map(_.mnemonic) should contain("tableswitch")
          tableSwitchCode.bodyIr.map(_.operation) should contain("switch")
          val tableSwitchAst = JimpleBodyIrAstBuilder.methodBodyAst(tableSwitchCode, excludedLocalNames = Set("value"))
          tableSwitchAst.nodes.collect { case control: NewControlStructure => control.code } should contain(
            "tableswitch(value)"
          )
          tableSwitchAst.nodes.collect { case jumpTarget: NewJumpTarget =>
            jumpTarget.name
          }.toSet should contain allOf (
            "default",
            "case 0",
            "case 1",
            "case 2"
          )

          val lookupSwitchCode = foo.methods.find(_.name == "lookupSwitch").flatMap(_.code).get
          lookupSwitchCode.instructions.map(_.mnemonic) should contain("lookupswitch")
          lookupSwitchCode.bodyIr.map(_.operation) should contain("switch")
          val lookupSwitchAst =
            JimpleBodyIrAstBuilder.methodBodyAst(lookupSwitchCode, excludedLocalNames = Set("value"))
          lookupSwitchAst.nodes.collect { case control: NewControlStructure => control.code } should contain(
            "lookupswitch(value)"
          )
          lookupSwitchAst.nodes.collect { case jumpTarget: NewJumpTarget =>
            jumpTarget.name
          }.toSet should contain allOf (
            "default",
            "case 7",
            "case 1000"
          )

          val postLocal     = foo.methods.find(_.name == "postLocal").get
          val postLocalCode = postLocal.code.get
          postLocalCode.bodyIr.map(_.operation) should contain allOf ("binary", "assignment", "return")
          postLocalCode.bodyIr.map(_.code) should contain allOf (
            "$stack1 = x",
            "(x + 1)",
            "x = (x + 1)",
            "return $stack1"
          )
          JimpleBodyIrAstBuilder
            .methodBodyAst(postLocal)
            .nodes
            .collect { case local: NewLocal => local.name -> local.typeFullName } should contain("$stack1" -> "int")

          val fieldPostCode = foo.methods.find(_.name == "fieldPost").flatMap(_.code).get
          fieldPostCode.bodyIr.map(_.operation) should not contain "unsupported"
          fieldPostCode.bodyIr.map(_.code) should contain allOf (
            "$stack1 = other.count",
            "($stack1 + 1)",
            "other.count = ($stack1 + 1)",
            "return $stack1"
          )

          val arrayPostCode = foo.methods.find(_.name == "arrayPost").flatMap(_.code).get
          arrayPostCode.bodyIr.map(_.operation) should not contain "unsupported"
          arrayPostCode.bodyIr.find(_.operation == "array_load").flatMap(_.target) should contain("int")
          arrayPostCode.bodyIr.find(_.code == "$stack1 = values[i]").flatMap(_.target) should contain("int")
          arrayPostCode.bodyIr.map(_.code) should contain allOf (
            "$stack1 = values[i]",
            "($stack1 + 1)",
            "values[i] = ($stack1 + 1)",
            "return $stack1"
          )
          JimpleBodyIrAstBuilder
            .methodBodyAst(arrayPostCode, excludedLocalNames = Set("values", "i"))
            .nodes
            .collect { case local: NewLocal => local.name -> local.typeFullName } should contain("$stack1" -> "int")

          val lambdaCode = foo.methods.find(_.name == "lambda").flatMap(_.code).get
          lambdaCode.instructions.map(_.mnemonic) should contain("invokedynamic")
          val lambdaDynamic =
            lambdaCode.bodyIr.find(entry => entry.operation == "call" && entry.target.contains("run")).get
          lambdaDynamic.arguments shouldBe List("x")
          lambdaDynamic.bootstrapArguments.exists(_.contains("lambda$lambda$0")) shouldBe true
          lambdaDynamic.signature shouldBe Some("java.lang.Runnable(java.lang.String)")
          lambdaDynamic.dispatchType shouldBe Some(DispatchTypes.DYNAMIC_DISPATCH)

          val mayThrow = foo.methods.find(_.name == "mayThrow").get
          mayThrow.exceptions.map(_.fullyQualifiedName) shouldBe List("java.lang.Exception")
          val mayThrowCode    = mayThrow.code.get
          val explicitThrowIr = mayThrowCode.bodyIr.find(_.operation == "throw").get
          explicitThrowIr.code shouldBe "athrow($stack1)"
          explicitThrowIr.arguments shouldBe List("$stack1")
          val mayThrowAst = JimpleBodyIrAstBuilder.methodBodyAst(mayThrow)
          val explicitThrowCall = mayThrowAst.nodes
            .collect { case call: NewCall => call }
            .find(_.name == "<operator>.throw")
            .get
          explicitThrowCall.code shouldBe "throw new java.lang.Exception()"
          mayThrowAst.nodes.collect {
            case identifier: NewIdentifier if identifier.code == "$stack1" => identifier.typeFullName
          } should contain("java.lang.Exception")

          val sync       = foo.methods.find(_.name == "sync").get
          val monitorOps = sync.code.get.bodyIr.filter(_.operation.contains("monitor"))
          monitorOps.map(_.operation) shouldBe List("monitorenter", "monitorexit", "monitorexit")
          monitorOps.head.code shouldBe "monitorenter(this)"
          monitorOps.head.arguments shouldBe List("this")
          monitorOps(1).code shouldBe "monitorexit(l2)"
          monitorOps(1).arguments shouldBe List("l2")
          monitorOps(2).code shouldBe "monitorexit(l2)"
          monitorOps(2).arguments shouldBe List("l2")
          val syncAst = JimpleBodyIrAstBuilder.methodBodyAst(sync)
          val monitorUnknowns = syncAst.nodes
            .collect { case unknown: NewUnknown => unknown.code }
            .filter(_.contains("monitor"))
          monitorUnknowns shouldBe List("entermonitor this", "exitmonitor l2", "exitmonitor l2")
          val monitorArgCodes = syncAst.argEdges.collect {
            case edge if edge.src.isInstanceOf[NewUnknown] =>
              edge.src.properties(PropertyNames.Code).toString -> edge.dst.properties(PropertyNames.Code).toString
          }
          monitorArgCodes should contain allOf (
            "entermonitor this" -> "this",
            "exitmonitor l2"    -> "l2"
          )

          val catchValue       = foo.methods.find(_.name == "catchValue").get
          val catchCode        = catchValue.code.get
          val exceptionHandler = catchCode.exceptionTable.head
          exceptionHandler.catchType.map(_.fullyQualifiedName) shouldBe Some("java.lang.Exception")
          exceptionHandler.startPc shouldBe 0
          exceptionHandler.endPc shouldBe 3
          exceptionHandler.handlerPc shouldBe 6
          catchCode.bodyIr.find(_.offset == exceptionHandler.handlerPc).map(_.code) should contain(
            "e = @caughtexception"
          )
          val catchBodyAstWithCfg =
            JimpleBodyIrAstBuilder.methodBodyAstWithCfg(catchCode, excludedLocalNames = Set.empty)
          val catchCfgCodes = catchBodyAstWithCfg.cfgEdges.map { case (source, destination) =>
            source.properties(PropertyNames.Code).toString -> destination.properties(PropertyNames.Code).toString
          }
          catchCfgCodes should contain("metadata.demo.Foo.mayThrow()" -> "e = @caughtexception")
          val catchAssignment = catchBodyAstWithCfg.ast.nodes
            .collect { case call: NewCall => call }
            .find(call => call.name == Operators.assignment && call.code == "e = @caughtexception")
            .get
          catchAssignment.typeFullName shouldBe "java.lang.Exception"

          val withFinally    = foo.methods.find(_.name == "withFinally").get
          val finallyCode    = withFinally.code.get
          val finallyHandler = finallyCode.exceptionTable.find(_.catchType.isEmpty).get
          val finallyCaughtException = finallyCode.bodyIr
            .find(entry => entry.offset == finallyHandler.handlerPc && entry.operation == "assignment")
            .get
          finallyCaughtException.code shouldBe "l2 = @caughtexception"
          finallyCaughtException.target shouldBe Some("java.lang.Throwable")
          finallyCaughtException.arguments shouldBe List("@caughtexception")
          val withFinallyAst = JimpleBodyIrAstBuilder.methodBodyAst(withFinally)
          withFinallyAst.nodes.collect { case local: NewLocal => local.name -> local.typeFullName } should contain(
            "l2" -> "java.lang.Throwable"
          )
          val finallyAssignment = withFinallyAst.nodes
            .collect { case call: NewCall => call }
            .find(call => call.name == Operators.assignment && call.code == "l2 = @caughtexception")
            .get
          finallyAssignment.typeFullName shouldBe "java.lang.Throwable"
          withFinallyAst.nodes.collect {
            case identifier: NewIdentifier if identifier.code == "@caughtexception" => identifier.typeFullName
          } should contain("java.lang.Throwable")
          withFinallyAst.nodes
            .collect { case call: NewCall => call }
            .find(_.name == "<operator>.throw")
            .map(_.code) should contain("throw new java.lang.Throwable()")
          labelCode.bodyIr.last.operation shouldBe "return"
          val bodyAstWithCfg = JimpleBodyIrAstBuilder.methodBodyAstWithCfg(label, excludedLocalNames = Set.empty)
          val bodyAst        = bodyAstWithCfg.ast
          bodyAst.nodes.collect {
            case call: NewCall if call.name == Operators.lengthOf => call.typeFullName
          } should contain("int")
          bodyAst.nodes.collect { case _: NewBlock => "block" } should not be empty
          bodyAst.nodes.collect { case local: NewLocal => local.name }.toSet should contain allOf ("values", "labels")
          bodyAst.nodes.collect { case call: NewCall => call.name }.toSet should contain allOf (
            "<operator>.fieldAccess",
            "<operator>.lengthOf"
          )
          bodyAst.nodes.collect { case ret: NewReturn => ret.code }.exists(_.startsWith("return")) shouldBe true
          bodyAstWithCfg.cfgEdges should not be empty
          labelCode.lineNumbers.map(_.lineNumber) should not be empty
          labelCode.localVariables.map(_.name) should contain allOf ("this", "values", "labels")
          val labelsLocal = labelCode.localVariables.find(_.name == "labels").get
          labelsLocal.descriptor shouldBe "Ljava/util/List;"
          labelsLocal.typeName shouldBe Some("java.util.List")
          labelsLocal.signature shouldBe Some("Ljava/util/List<Ljava/lang/String;>;")

          val chooseCode = foo.methods.find(_.name == "choose").flatMap(_.code).get
          val branch     = chooseCode.bodyIr.find(_.operation == "branch").get
          branch.targets should not be empty
          JimpleBodyIrAstBuilder
            .methodBodyAstWithCfg(chooseCode, excludedLocalNames = Set("x"))
            .cfgEdges
            .size should be > 1

          val adjustCode = foo.methods.find(_.name == "adjust").flatMap(_.code).get
          val gotoBranch = adjustCode.bodyIr
            .find(entry => entry.operation == "branch" && entry.code.startsWith("goto"))
            .get
          val expectedGotoLine = adjustCode.lineNumbers
            .sortBy(_.startPc)
            .takeWhile(_.startPc <= gotoBranch.targets.head)
            .last
            .lineNumber
          val adjustBodyAstWithCfg =
            JimpleBodyIrAstBuilder.methodBodyAstWithCfg(adjustCode, excludedLocalNames = Set("x"))
          adjustBodyAstWithCfg.ast.nodes.collect { case unknown: NewUnknown => unknown.code } should contain(
            s"goto $expectedGotoLine"
          )
          val adjustCfgCodes = adjustBodyAstWithCfg.cfgEdges.map { case (source, destination) =>
            source.properties(PropertyNames.Code).toString -> destination.properties(PropertyNames.Code).toString
          }
          adjustCfgCodes should contain(s"goto $expectedGotoLine" -> "return x;")
        }
      }
    }

    "build CFG edges for legacy subroutine IR" in {
      val codeInfo = JimpleMethodCodeInfo(
        maxStack = 1,
        maxLocals = 2,
        bytecodeLength = 8,
        instructions = Nil,
        bodyIr = List(
          JimpleMethodBodyIrInfo(
            offset = 0,
            operation = "jsr",
            code = "jsr 5",
            result = None,
            target = None,
            methodFullName = None,
            signature = None,
            dispatchType = None,
            receiver = None,
            targets = List(5),
            arguments = List("@retaddr3"),
            bootstrapArguments = Nil
          ),
          JimpleMethodBodyIrInfo(
            offset = 3,
            operation = "return",
            code = "return 1",
            result = None,
            target = None,
            methodFullName = None,
            signature = None,
            dispatchType = None,
            receiver = None,
            targets = Nil,
            arguments = List("1"),
            bootstrapArguments = Nil
          ),
          JimpleMethodBodyIrInfo(
            offset = 5,
            operation = "assignment",
            code = "l1 = @retaddr3",
            result = Some("l1"),
            target = None,
            methodFullName = None,
            signature = None,
            dispatchType = None,
            receiver = None,
            targets = Nil,
            arguments = List("@retaddr3"),
            bootstrapArguments = Nil
          ),
          JimpleMethodBodyIrInfo(
            offset = 6,
            operation = "ret",
            code = "ret l1",
            result = None,
            target = None,
            methodFullName = None,
            signature = None,
            dispatchType = None,
            receiver = None,
            targets = List(3),
            arguments = List("l1"),
            bootstrapArguments = Nil
          )
        ),
        exceptionTable = Nil,
        lineNumbers = List(
          JimpleLineNumberInfo(startPc = 0, lineNumber = 10),
          JimpleLineNumberInfo(startPc = 3, lineNumber = 11),
          JimpleLineNumberInfo(startPc = 5, lineNumber = 12),
          JimpleLineNumberInfo(startPc = 6, lineNumber = 13)
        ),
        localVariables = Nil
      )

      val bodyAstWithCfg = JimpleBodyIrAstBuilder.methodBodyAstWithCfg(codeInfo, excludedLocalNames = Set.empty)
      val unknownCodes   = bodyAstWithCfg.ast.nodes.collect { case unknown: NewUnknown => unknown.code }
      unknownCodes should contain allOf ("jsr 5", "ret l1")
      val unknownChildCodes = bodyAstWithCfg.ast.edges.collect {
        case edge if edge.src.isInstanceOf[NewUnknown] =>
          edge.src.properties(PropertyNames.Code).toString -> edge.dst.properties(PropertyNames.Code).toString
      }
      unknownChildCodes should contain allOf ("jsr 5" -> "@retaddr3", "ret l1" -> "l1")
      val cfgCodes = bodyAstWithCfg.cfgEdges.map { case (source, destination) =>
        source.properties(PropertyNames.Code).toString -> destination.properties(PropertyNames.Code).toString
      }
      cfgCodes should contain allOf (
        "jsr 5"          -> "l1 = @retaddr3",
        "l1 = @retaddr3" -> "ret l1",
        "ret l1"         -> "return 1;"
      )
      cfgCodes should not contain ("jsr 5" -> "return 1;")
    }
  }

  private def writeFile(path: Path, content: String): Unit = {
    path.createWithParentsIfNotExists(createParents = true)
    Files.writeString(path, content)
  }

  private def writeJar(jarFile: Path, classFile: Path, entryName: String): Unit = {
    val out = new JarOutputStream(Files.newOutputStream(jarFile))
    try {
      out.putNextEntry(new JarEntry(entryName))
      out.write(Files.readAllBytes(classFile))
      out.closeEntry()
    } finally {
      out.close()
    }
  }
}
