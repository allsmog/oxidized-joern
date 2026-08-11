package io.joern.kotlin2cpg.querying

import io.joern.kotlin2cpg.{Config, Constants, Kotlin2Cpg, KotlinParserBackend}
import io.joern.kotlin2cpg.types.TypeConstants
import io.joern.x2cpg.Defines
import io.shiftleft.codepropertygraph.generated.{
  ControlStructureTypes,
  DispatchTypes,
  EvaluationStrategies,
  ModifierTypes,
  Operators
}
import io.shiftleft.codepropertygraph.generated.edges.Capture
import io.shiftleft.codepropertygraph.generated.nodes.{Annotation, AnnotationLiteral, JumpLabel, JumpTarget, Unknown}
import io.shiftleft.semanticcpg.language.*
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.FileUtil.*
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

import java.nio.file.{Files, Path}

class OxidizedKotlinCpgTests extends AnyWordSpec with Matchers {

  "oxidized Kotlin backend" should {

    "run native astgen and materialize parsed source files" in {
      withOxidizedCpg("""package demo
          |class Sample {
          |  fun value(): Int = 1
          |}
          |""".stripMargin) { cpg =>
        cpg.metaData.language.l shouldBe List("KOTLIN")
        cpg.file.name.l should contain("demo/Sample.kt")
        cpg.file.nameExact("demo/Sample.kt").content.l.head should include("class Sample")
      }
    }

    "lower packages, imports, classes, members, methods, parameters, and returns" in {
      withOxidizedCpg("""package demo
          |
          |import kotlin.math.max
          |
          |class Foo(val x: Int) {
          |  val y: String = "hi"
          |
          |  fun add(a: Int, b: Int): Int {
          |    return a + b
          |  }
          |}
          |
          |fun top(name: String): String {
          |  return name
          |}
          |""".stripMargin) { cpg =>
        cpg.namespaceBlock.nameExact("demo").filename.l shouldBe List("demo/Sample.kt")
        cpg.imports.importedEntity.l should contain("kotlin.math.max")
        cpg.imports.importedAs.l should contain("max")

        cpg.typeDecl.nameExact("Foo").fullName.l shouldBe List("demo.Foo")
        cpg.typeDecl.nameExact("Foo").inheritsFromTypeFullName.l shouldBe List("java.lang.Object")
        cpg.member.nameExact("x").typeFullName.l shouldBe List("int")
        cpg.member.nameExact("y").typeFullName.l shouldBe List("java.lang.String")

        val List(ctor) = cpg.method.fullNameExact("demo.Foo.<init>:void(int)").l: @unchecked
        ctor.parameter.name.l shouldBe List("this", "x")
        ctor.methodReturn.typeFullName shouldBe "void"
        ctor.block.astChildren.isCall.nameExact(Operators.assignment).code.l shouldBe List(
          "this.x = x",
          "this.y = \"hi\""
        )
        ctor.ast.isCall
          .codeExact("this.x = x")
          .argument
          .isIdentifier
          .nameExact("x")
          .refsTo
          .l shouldBe
          ctor.parameter.nameExact("x").l
        ctor.ast.isCall
          .codeExact("this.x = x")
          .argument
          .isCall
          .nameExact(Operators.fieldAccess)
          .argument
          .isIdentifier
          .nameExact("this")
          .refsTo
          .l shouldBe
          ctor.parameter.nameExact("this").l

        val List(add) = cpg.method.fullNameExact("demo.Foo.add:int(int,int)").l: @unchecked
        add.parameter.name.l shouldBe List("this", "a", "b")
        add.parameter.nameExact("a").typeFullName.l shouldBe List("int")
        add.parameter.nameExact("b").typeFullName.l shouldBe List("int")
        add.methodReturn.typeFullName shouldBe "int"
        add.ast.isReturn.code.l shouldBe List("return a + b")
        add.ast.isReturn.astChildren.isCall.name.l shouldBe List(Operators.addition)

        val List(top) = cpg.method.fullNameExact("demo.top:java.lang.String(java.lang.String)").l: @unchecked
        top.parameter.name.l shouldBe List("name")
        top.parameter.nameExact("name").typeFullName.l shouldBe List("java.lang.String")
        top.methodReturn.typeFullName shouldBe "java.lang.String"
        top.ast.isReturn.code.l shouldBe List("return name")
      }
    }

    "lower class property initializers in primary constructors" in {
      withOxidizedCpg("""package demo
          |
          |fun addB(a: String): String {
          |  return a + "b"
          |}
          |
          |class MyClass(val x: String) {
          |  var m: String = addB(x)
          |  fun printM() = println(this.m)
          |}
          |""".stripMargin) { cpg =>
        val List(ctor) = cpg.method.fullNameExact("demo.MyClass.<init>:void(java.lang.String)").l: @unchecked
        ctor.block.astChildren.isCall.nameExact(Operators.assignment).code.l shouldBe List(
          "this.x = x",
          "this.m = addB(x)"
        )

        val List(memberInit) = ctor.ast.isCall.codeExact("this.m = addB(x)").l: @unchecked
        val List(lhs)        = memberInit.argument.isCall.nameExact(Operators.fieldAccess).l: @unchecked
        lhs.code shouldBe "this.m"
        lhs.argument.isIdentifier.nameExact("this").refsTo.l shouldBe ctor.parameter.nameExact("this").l
        lhs.argument.isFieldIdentifier.canonicalName.l shouldBe List("m")

        val List(rhs) = memberInit.argument.isCall.nameExact("addB").l: @unchecked
        rhs.code shouldBe "addB(x)"
        rhs.methodFullName shouldBe "demo.addB:java.lang.String(java.lang.String)"
        rhs.argument.isIdentifier.nameExact("x").refsTo.l shouldBe ctor.parameter.nameExact("x").l
      }
    }

    "lower local constructor initializers with allocation and init calls" in {
      withOxidizedCpg("""package demo
          |
          |class AClass(val x: String) {
          |  fun printX() {
          |    println(x)
          |  }
          |}
          |
          |fun make(msg: String) {
          |  val a = AClass(msg)
          |  a.printX()
          |}
          |""".stripMargin) { cpg =>
        val List(make)   = cpg.method.fullNameExact("demo.make:void(java.lang.String)").l: @unchecked
        val List(aLocal) = make.ast.isLocal.nameExact("a").l: @unchecked
        aLocal.typeFullName shouldBe "demo.AClass"

        val List(allocAssignment) =
          make.block.astChildren.isCall.nameExact(Operators.assignment).codeExact("a = <alloc>").l
        val List(assignmentLhs) = allocAssignment.argument.isIdentifier.nameExact("a").l: @unchecked
        assignmentLhs.refsTo.l shouldBe List(aLocal)
        allocAssignment.argument.isCall.nameExact(Operators.alloc).typeFullName.l shouldBe List("demo.AClass")

        val List(initCall) = make.block.astChildren.isCall.nameExact("<init>").codeExact("AClass(msg)").l: @unchecked
        initCall.methodFullName shouldBe "demo.AClass.<init>:void(java.lang.String)"
        initCall.signature shouldBe "void(java.lang.String)"
        initCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        initCall.typeFullName shouldBe "void"
        initCall.argument.isIdentifier.nameExact("a").refsTo.l shouldBe List(aLocal)
        initCall.argument.isIdentifier.nameExact("msg").refsTo.l shouldBe make.parameter.nameExact("msg").l

        val List(printCall) = make.block.astChildren.isCall.nameExact("printX").l: @unchecked
        printCall.methodFullName shouldBe "demo.AClass.printX:void()"
        printCall.argument.isIdentifier.nameExact("a").refsTo.l shouldBe List(aLocal)
      }
    }

    "lower constructor calls used as expression arguments and receivers" in {
      withOxidizedCpg("""package demo
          |
          |class AClass(val x: String) {
          |  fun appendX(to: String): String {
          |    return to + x
          |  }
          |}
          |
          |fun make(msg: String) {
          |  val values = listOf(AClass(msg))
          |  val out = AClass(msg).appendX("!")
          |  println(values)
          |  println(out)
          |}
          |""".stripMargin) { cpg =>
        val List(make) = cpg.method.fullNameExact("demo.make:void(java.lang.String)").l: @unchecked

        val List(listCall) = make.ast.isCall.nameExact("listOf").codeExact("listOf(AClass(msg))").l: @unchecked
        listCall.methodFullName shouldBe "kotlin.collections.listOf:java.util.List(java.lang.Object)"
        val List(argumentBlock) = listCall.argument.isBlock.l: @unchecked
        argumentBlock.typeFullName shouldBe "demo.AClass"
        val List(firstTmp)        = argumentBlock.astChildren.isLocal.nameExact("tmp_1").l: @unchecked
        val List(firstAssignment) = argumentBlock.astChildren.isCall.nameExact(Operators.assignment).l: @unchecked
        firstAssignment.code shouldBe "tmp_1 = <alloc>"
        firstAssignment.argument.isCall.nameExact(Operators.alloc).typeFullName.l shouldBe List("demo.AClass")
        val List(firstInit) = argumentBlock.astChildren.isCall.nameExact("<init>").l: @unchecked
        firstInit.methodFullName shouldBe "demo.AClass.<init>:void(java.lang.String)"
        firstInit.signature shouldBe "void(java.lang.String)"
        firstInit.argument.isIdentifier.nameExact("tmp_1").refsTo.l shouldBe List(firstTmp)
        firstInit.argument.isIdentifier.nameExact("msg").refsTo.l shouldBe make.parameter.nameExact("msg").l
        argumentBlock.astChildren.isIdentifier.nameExact("tmp_1").refsTo.l shouldBe List(firstTmp)

        val List(appendCall) =
          make.ast.isCall.nameExact("appendX").codeExact("""AClass(msg).appendX("!")""").l: @unchecked
        appendCall.methodFullName shouldBe "demo.AClass.appendX:java.lang.String(java.lang.String)"
        appendCall.signature shouldBe "java.lang.String(java.lang.String)"
        appendCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

        val List(receiverBlock) = appendCall.argument.isBlock.l: @unchecked
        receiverBlock.typeFullName shouldBe "demo.AClass"
        val List(secondTmp) = receiverBlock.astChildren.isLocal.nameExact("tmp_2").l: @unchecked
        receiverBlock.astChildren.isCall.nameExact(Operators.assignment).code.l shouldBe List("tmp_2 = <alloc>")
        val List(secondInit) = receiverBlock.astChildren.isCall.nameExact("<init>").l: @unchecked
        secondInit.methodFullName shouldBe "demo.AClass.<init>:void(java.lang.String)"
        secondInit.argument.isIdentifier.nameExact("tmp_2").refsTo.l shouldBe List(secondTmp)
        secondInit.argument.isIdentifier.nameExact("msg").refsTo.l shouldBe make.parameter.nameExact("msg").l
        receiverBlock.astChildren.isIdentifier.nameExact("tmp_2").refsTo.l shouldBe List(secondTmp)
        appendCall.argument.isLiteral.codeExact("\"!\"").size shouldBe 1
      }
    }

    "lower imported and fully-qualified Java constructors" in {
      withOxidizedCpg("""package demo
          |
          |import java.io.File
          |
          |fun make(path: String) {
          |  val f = File(path)
          |  val err = java.lang.Error("err")
          |  val files = listOf(File(path))
          |  val errors = listOf(java.lang.Error("nested"))
          |  f.writeText(path)
          |  File(path).writeText("Hello")
          |  println(f)
          |  println(err)
          |  println(files)
          |  println(errors)
          |}
          |""".stripMargin) { cpg =>
        val List(make) = cpg.method.fullNameExact("demo.make:void(java.lang.String)").l: @unchecked

        val List(fLocal) = make.ast.isLocal.nameExact("f").l: @unchecked
        fLocal.typeFullName shouldBe "java.io.File"
        val List(fileInit) =
          make.block.astChildren.isCall
            .nameExact("<init>")
            .methodFullNameExact("java.io.File.<init>:void(java.lang.String)")
            .l: @unchecked
        fileInit.signature shouldBe "void(java.lang.String)"
        fileInit.argument.isIdentifier.nameExact("f").refsTo.l shouldBe List(fLocal)
        fileInit.argument.isIdentifier.nameExact("path").refsTo.l shouldBe make.parameter.nameExact("path").l

        val List(errLocal) = make.ast.isLocal.nameExact("err").l: @unchecked
        errLocal.typeFullName shouldBe "java.lang.Error"
        val List(errorInit) =
          make.block.astChildren.isCall
            .nameExact("<init>")
            .methodFullNameExact("java.lang.Error.<init>:void(java.lang.String)")
            .l: @unchecked
        errorInit.signature shouldBe "void(java.lang.String)"
        errorInit.argument.isIdentifier.nameExact("err").refsTo.l shouldBe List(errLocal)
        errorInit.argument.isLiteral.codeExact("\"err\"").size shouldBe 1

        val List(fileListCall)      = make.ast.isCall.nameExact("listOf").codeExact("listOf(File(path))").l: @unchecked
        val List(fileArgumentBlock) = fileListCall.argument.isBlock.l: @unchecked
        fileArgumentBlock.typeFullName shouldBe "java.io.File"
        val List(fileTmp) = fileArgumentBlock.astChildren.isLocal.nameExact("tmp_1").l: @unchecked
        fileArgumentBlock.astChildren.isCall.nameExact("<init>").methodFullName.l shouldBe List(
          "java.io.File.<init>:void(java.lang.String)"
        )
        fileArgumentBlock.astChildren.isCall
          .nameExact("<init>")
          .argument
          .isIdentifier
          .nameExact("tmp_1")
          .refsTo
          .l shouldBe List(fileTmp)
        fileArgumentBlock.astChildren.isIdentifier.nameExact("tmp_1").refsTo.l shouldBe List(fileTmp)

        val List(errorListCall) =
          make.ast.isCall.nameExact("listOf").codeExact("""listOf(java.lang.Error("nested"))""").l: @unchecked
        val List(errorArgumentBlock) = errorListCall.argument.isBlock.l: @unchecked
        errorArgumentBlock.typeFullName shouldBe "java.lang.Error"
        val List(errorTmp) = errorArgumentBlock.astChildren.isLocal.nameExact("tmp_2").l: @unchecked
        errorArgumentBlock.astChildren.isCall.nameExact("<init>").methodFullName.l shouldBe List(
          "java.lang.Error.<init>:void(java.lang.String)"
        )
        errorArgumentBlock.astChildren.isCall
          .nameExact("<init>")
          .argument
          .isIdentifier
          .nameExact("tmp_2")
          .refsTo
          .l shouldBe List(errorTmp)
        errorArgumentBlock.astChildren.isIdentifier.nameExact("tmp_2").refsTo.l shouldBe List(errorTmp)

        val writeTextCalls = make.ast.isCall.nameExact("writeText").l
        writeTextCalls.map(_.methodFullName).distinct shouldBe List(
          "kotlin.io.writeText:void(java.io.File,java.lang.String,java.nio.charset.Charset)"
        )
        writeTextCalls.map(_.signature).distinct shouldBe List(
          "void(java.io.File,java.lang.String,java.nio.charset.Charset)"
        )
        writeTextCalls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.STATIC_DISPATCH)
        writeTextCalls.map(_.typeFullName).distinct shouldBe List("void")

        val List(localWriteTextCall) = writeTextCalls.filter(_.code == "f.writeText(path)"): @unchecked
        localWriteTextCall.argument.isIdentifier.nameExact("f").refsTo.l shouldBe List(fLocal)
        localWriteTextCall.argument.isIdentifier.nameExact("path").refsTo.l shouldBe make.parameter.nameExact("path").l

        val List(inlineWriteTextCall) = writeTextCalls.filter(_.code == """File(path).writeText("Hello")"""): @unchecked
        val List(inlineReceiverBlock) = inlineWriteTextCall.argument.isBlock.l: @unchecked
        inlineReceiverBlock.typeFullName shouldBe "java.io.File"
        val List(inlineFileTmp) = inlineReceiverBlock.astChildren.isLocal.nameExact("tmp_3").l: @unchecked
        inlineReceiverBlock.astChildren.isCall.nameExact("<init>").methodFullName.l shouldBe List(
          "java.io.File.<init>:void(java.lang.String)"
        )
        inlineReceiverBlock.astChildren.isIdentifier.nameExact("tmp_3").refsTo.l shouldBe List(inlineFileTmp)
        inlineWriteTextCall.argument.isLiteral.codeExact("\"Hello\"").size shouldBe 1
      }
    }

    "preserve unresolved imported aliases for unknown external calls" in {
      withOxidizedCpg("""package demo
          |
          |import no.such.CaseClass as MyCaseClass
          |
          |fun main() {
          |  val res = MyCaseClass.PROP
          |  println(res)
          |
          |  val otherRes = MyCaseClass("AN_ARGUMENT")
          |  println(otherRes.aFn())
          |}
          |""".stripMargin) { cpg =>
        val List(main) = cpg.method.fullNameExact("demo.main:void()").l: @unchecked

        val List(propCall) =
          main.ast.isCall.nameExact(Operators.fieldAccess).codeExact("MyCaseClass.PROP").l: @unchecked
        propCall.methodFullName shouldBe Operators.fieldAccess
        propCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        propCall.signature shouldBe ""
        propCall.argument.isIdentifier.nameExact("MyCaseClass").typeFullName.l shouldBe List("no.such.CaseClass")
        propCall.argument.isFieldIdentifier.canonicalName.l shouldBe List("PROP")

        val List(aliasCall) = main.ast.isCall.nameExact("MyCaseClass").codeExact("""MyCaseClass("AN_ARGUMENT")""").l
        aliasCall.methodFullName shouldBe s"no.such.CaseClass:${Defines.UnresolvedSignature}(1)"
        aliasCall.signature shouldBe s"${Defines.UnresolvedSignature}(1)"
        aliasCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        aliasCall.argument.isLiteral.codeExact("\"AN_ARGUMENT\"").size shouldBe 1
        main.ast.isLocal.nameExact("otherRes").typeFullName.l shouldBe List("ANY")
      }
    }

    "resolve default JVM and Kotlin qualified calls" in {
      withOxidizedCpg("""package demo
          |
          |import java.util.UUID
          |import kotlin.random.Random
          |
          |fun qualified(execStr: String): String {
          |  val process = Runtime.getRuntime().exec(execStr)
          |  val randomBound = Random.nextInt(0, 100)
          |  val randomUntil = Random.nextInt(100)
          |  val uuid = UUID.randomUUID()
          |  val out = StringBuilder().append("one").append("-two").toString()
          |  val rng = Random(1)
          |  val coin = rng.nextBoolean()
          |  println(process)
          |  println(randomBound)
          |  println(randomUntil)
          |  println(uuid)
          |  println(coin)
          |  return out
          |}
          |""".stripMargin) { cpg =>
        val List(qualified) =
          cpg.method.fullNameExact("demo.qualified:java.lang.String(java.lang.String)").l: @unchecked

        val List(getRuntimeCall) = qualified.ast.isCall.nameExact("getRuntime").l: @unchecked
        getRuntimeCall.methodFullName shouldBe "java.lang.Runtime.getRuntime:java.lang.Runtime()"
        getRuntimeCall.signature shouldBe "java.lang.Runtime()"
        getRuntimeCall.typeFullName shouldBe "java.lang.Runtime"
        getRuntimeCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(execCall) = qualified.ast.isCall.nameExact("exec").l: @unchecked
        execCall.methodFullName shouldBe "java.lang.Runtime.exec:java.lang.Process(java.lang.String)"
        execCall.signature shouldBe "java.lang.Process(java.lang.String)"
        execCall.typeFullName shouldBe "java.lang.Process"
        execCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        execCall.argument.isIdentifier.nameExact("execStr").refsTo.l shouldBe qualified.parameter.nameExact("execStr").l

        val randomCalls = qualified.ast.isCall.nameExact("nextInt").l
        randomCalls.map(_.methodFullName).toSet shouldBe Set(
          "kotlin.random.Random.nextInt:int(int)",
          "kotlin.random.Random.nextInt:int(int,int)"
        )
        randomCalls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.STATIC_DISPATCH)
        qualified.ast.isLocal.nameExact("randomBound").typeFullName.l shouldBe List("int")
        qualified.ast.isLocal.nameExact("randomUntil").typeFullName.l shouldBe List("int")

        val List(uuidCall) = qualified.ast.isCall.nameExact("randomUUID").l: @unchecked
        uuidCall.methodFullName shouldBe "java.util.UUID.randomUUID:java.util.UUID()"
        uuidCall.signature shouldBe "java.util.UUID()"
        uuidCall.typeFullName shouldBe "java.util.UUID"
        uuidCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        qualified.ast.isLocal.nameExact("uuid").typeFullName.l shouldBe List("java.util.UUID")

        val appendCalls = qualified.ast.isCall.nameExact("append").l.sortBy(_.code)
        appendCalls.map(_.methodFullName).distinct shouldBe List(
          "java.lang.StringBuilder.append:java.lang.StringBuilder(java.lang.String)"
        )
        appendCalls.map(_.typeFullName).distinct shouldBe List("java.lang.StringBuilder")

        val List(toStringCall) =
          qualified.ast.isCall
            .nameExact("toString")
            .codeExact("""StringBuilder().append("one").append("-two").toString()""")
            .l: @unchecked
        toStringCall.methodFullName shouldBe "java.lang.StringBuilder.toString:java.lang.String()"
        toStringCall.typeFullName shouldBe "java.lang.String"
        qualified.ast.isLocal.nameExact("out").typeFullName.l shouldBe List("java.lang.String")

        val List(rngLocal) = qualified.ast.isLocal.nameExact("rng").l: @unchecked
        rngLocal.typeFullName shouldBe "kotlin.random.Random"
        val List(nextBooleanCall) = qualified.ast.isCall.nameExact("nextBoolean").l: @unchecked
        nextBooleanCall.methodFullName shouldBe "kotlin.random.Random.nextBoolean:boolean()"
        nextBooleanCall.signature shouldBe "boolean()"
        nextBooleanCall.typeFullName shouldBe "boolean"
        nextBooleanCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        nextBooleanCall.argument.isIdentifier.nameExact("rng").refsTo.l shouldBe List(rngLocal)
        qualified.ast.isLocal.nameExact("coin").typeFullName.l shouldBe List("boolean")
      }
    }

    "resolve HashMap constructors member calls and indexed values" in {
      withOxidizedCpg("""package demo
          |
          |fun getHashMap(): HashMap<String, String> {
          |  val aMap = HashMap<String, String>()
          |  aMap["user"] = "foo"
          |  return aMap
          |}
          |
          |fun hashMapUser() {
          |  val contains = getHashMap().containsKey("user")
          |  val local = HashMap<String, String>()
          |  local["user"] = "foo"
          |  val value = local["user"]
          |  println(contains)
          |  println(value)
          |}
          |""".stripMargin) { cpg =>
        val List(getHashMap) = cpg.method.fullNameExact("demo.getHashMap:java.util.HashMap()").l: @unchecked
        getHashMap.methodReturn.typeFullName shouldBe "java.util.HashMap"
        getHashMap.ast.isLocal.nameExact("aMap").typeFullName.l shouldBe List("java.util.HashMap")
        getHashMap.ast.isCall.nameExact(Operators.assignment).codeExact("""aMap["user"] = "foo"""").size shouldBe 1

        val List(hashMapUser)  = cpg.method.fullNameExact("demo.hashMapUser:void()").l: @unchecked
        val List(containsCall) = hashMapUser.ast.isCall.nameExact("containsKey").l: @unchecked
        containsCall.methodFullName shouldBe "java.util.HashMap.containsKey:boolean(java.lang.Object)"
        containsCall.signature shouldBe "boolean(java.lang.Object)"
        containsCall.typeFullName shouldBe "boolean"
        containsCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        containsCall.argument.isCall.nameExact("getHashMap").methodFullName.l shouldBe List(
          "demo.getHashMap:java.util.HashMap()"
        )
        containsCall.argument.isLiteral.codeExact("\"user\"").size shouldBe 1
        hashMapUser.ast.isLocal.nameExact("contains").typeFullName.l shouldBe List("boolean")

        val List(localMap) = hashMapUser.ast.isLocal.nameExact("local").l: @unchecked
        localMap.typeFullName shouldBe "java.util.HashMap"
        val List(valueLocal) = hashMapUser.ast.isLocal.nameExact("value").l: @unchecked
        valueLocal.typeFullName shouldBe "java.lang.String"
        val indexCalls = hashMapUser.ast.isCall.nameExact(Operators.indexAccess).codeExact("""local["user"]""").l
        indexCalls.size shouldBe 2
        indexCalls.map(_.typeFullName).distinct shouldBe List("java.lang.String")
        indexCalls.flatMap(_.argument.isIdentifier.nameExact("local").refsTo.l).distinct shouldBe List(localMap)
      }
    }

    "resolve arrayOf element types for indexed receivers" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayReceiver() {
          |  val arr = arrayOf(1, 2, 3)
          |  val z = arr[0].toString()
          |  println(z)
          |}
          |""".stripMargin) { cpg =>
        val List(arrayReceiver) = cpg.method.fullNameExact("demo.arrayReceiver:void()").l: @unchecked
        val List(arrLocal)      = arrayReceiver.ast.isLocal.nameExact("arr").l: @unchecked
        arrLocal.typeFullName shouldBe "int[]"

        val List(indexCall) =
          arrayReceiver.ast.isCall.nameExact(Operators.indexAccess).codeExact("arr[0]").l: @unchecked
        indexCall.typeFullName shouldBe "int"
        indexCall.argument.isIdentifier.nameExact("arr").refsTo.l shouldBe List(arrLocal)

        val List(toStringCall) =
          arrayReceiver.ast.isCall.nameExact("toString").codeExact("arr[0].toString()").l: @unchecked
        toStringCall.methodFullName shouldBe "kotlin.Int.toString:java.lang.String()"
        toStringCall.signature shouldBe "java.lang.String()"
        toStringCall.typeFullName shouldBe "java.lang.String"
        toStringCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        toStringCall.argument.isCall.nameExact(Operators.indexAccess).codeExact("arr[0]").typeFullName.l shouldBe List(
          "int"
        )
        arrayReceiver.ast.isLocal.nameExact("z").typeFullName.l shouldBe List("java.lang.String")
      }
    }

    "resolve primitive and nullable array factory calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayFactories(size: Int) {
          |  val strings = emptyArray<String>()
          |  val nullStrings = arrayOfNulls<String>(size)
          |  val ints = intArrayOf(1, 2)
          |  val longs = longArrayOf(1L, 2L)
          |  val chars = charArrayOf('a', 'b')
          |  val booleans = booleanArrayOf(true, false)
          |  val doubles = doubleArrayOf(1.0, 2.0)
          |  val firstString = strings[0]
          |  val firstNullString = nullStrings[0]
          |  val firstInt = ints[0]
          |  val firstLong = longs[0]
          |  val firstChar = chars[0]
          |  val firstBoolean = booleans[0]
          |  val firstDouble = doubles[0]
          |  println(firstString)
          |  println(firstNullString)
          |  println(firstInt)
          |  println(firstLong)
          |  println(firstChar)
          |  println(firstBoolean)
          |  println(firstDouble)
          |}
          |""".stripMargin) { cpg =>
        val List(arrayFactories) = cpg.method.fullNameExact("demo.arrayFactories:void(int)").l: @unchecked

        Map(
          "emptyArray<String>()" -> ("emptyArray", "kotlin.emptyArray", "java.lang.Object[]()", "java.lang.String[]"),
          "arrayOfNulls<String>(size)" -> ("arrayOfNulls", "kotlin.arrayOfNulls", "java.lang.Object[](int)", "java.lang.String[]"),
          "intArrayOf(1, 2)"      -> ("intArrayOf", "kotlin.intArrayOf", "int[](int[])", "int[]"),
          "longArrayOf(1L, 2L)"   -> ("longArrayOf", "kotlin.longArrayOf", "long[](long[])", "long[]"),
          "charArrayOf('a', 'b')" -> ("charArrayOf", "kotlin.charArrayOf", "char[](char[])", "char[]"),
          "booleanArrayOf(true, false)" -> ("booleanArrayOf", "kotlin.booleanArrayOf", "boolean[](boolean[])", "boolean[]"),
          "doubleArrayOf(1.0, 2.0)" -> ("doubleArrayOf", "kotlin.doubleArrayOf", "double[](double[])", "double[]")
        ).foreach { case (code, (name, fullNameBase, signature, typeFullName)) =>
          val List(call) = arrayFactories.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = arrayFactories.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "strings"         -> "java.lang.String[]",
          "nullStrings"     -> "java.lang.String[]",
          "ints"            -> "int[]",
          "longs"           -> "long[]",
          "chars"           -> "char[]",
          "booleans"        -> "boolean[]",
          "doubles"         -> "double[]",
          "firstString"     -> "java.lang.String",
          "firstNullString" -> "java.lang.String",
          "firstInt"        -> "int",
          "firstLong"       -> "long",
          "firstChar"       -> "char",
          "firstBoolean"    -> "boolean",
          "firstDouble"     -> "double"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }

        Map(
          "strings[0]"     -> "java.lang.String",
          "nullStrings[0]" -> "java.lang.String",
          "ints[0]"        -> "int",
          "longs[0]"       -> "long",
          "chars[0]"       -> "char",
          "booleans[0]"    -> "boolean",
          "doubles[0]"     -> "double"
        ).foreach { case (code, typeFullName) =>
          val List(indexCall) =
            arrayFactories.ast.isCall.nameExact(Operators.indexAccess).codeExact(code).l: @unchecked
          indexCall.typeFullName shouldBe typeFullName
        }
      }
    }

    "resolve array constructor calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayConstructors(size: Int) {
          |  val strings = Array<String>(size) { it.toString() }
          |  val ints = IntArray(size)
          |  val initializedInts = IntArray(size) { it }
          |  val longs = LongArray(size) { idx -> idx.toLong() }
          |  val chars = CharArray(size)
          |  val booleans = BooleanArray(size)
          |  val bytes = ByteArray(size)
          |  val shorts = ShortArray(size) { idx -> idx.toShort() }
          |  val floats = FloatArray(size)
          |  val doubles = DoubleArray(size) { idx -> idx.toDouble() }
          |  val firstString = strings[0]
          |  val firstInt = ints[0]
          |  val firstInitializedInt = initializedInts[0]
          |  val firstLong = longs[0]
          |  val firstChar = chars[0]
          |  val firstBoolean = booleans[0]
          |  val firstByte = bytes[0]
          |  val firstShort = shorts[0]
          |  val firstFloat = floats[0]
          |  val firstDouble = doubles[0]
          |  println(firstString)
          |  println(firstInt)
          |  println(firstInitializedInt)
          |  println(firstLong)
          |  println(firstChar)
          |  println(firstBoolean)
          |  println(firstByte)
          |  println(firstShort)
          |  println(firstFloat)
          |  println(firstDouble)
          |}
          |""".stripMargin) { cpg =>
        val List(arrayConstructors) = cpg.method.fullNameExact("demo.arrayConstructors:void(int)").l: @unchecked

        Map(
          "Array<String>(size) { it.toString() }" -> ("kotlin.Array.<init>", "void(int,kotlin.jvm.functions.Function1)"),
          "IntArray(size)"        -> ("kotlin.IntArray.<init>", "void(int)"),
          "IntArray(size) { it }" -> ("kotlin.IntArray.<init>", "void(int,kotlin.jvm.functions.Function1)"),
          "LongArray(size) { idx -> idx.toLong() }" -> (
            "kotlin.LongArray.<init>",
            "void(int,kotlin.jvm.functions.Function1)"
          ),
          "CharArray(size)"    -> ("kotlin.CharArray.<init>", "void(int)"),
          "BooleanArray(size)" -> ("kotlin.BooleanArray.<init>", "void(int)"),
          "ByteArray(size)"    -> ("kotlin.ByteArray.<init>", "void(int)"),
          "ShortArray(size) { idx -> idx.toShort() }" -> (
            "kotlin.ShortArray.<init>",
            "void(int,kotlin.jvm.functions.Function1)"
          ),
          "FloatArray(size)" -> ("kotlin.FloatArray.<init>", "void(int)"),
          "DoubleArray(size) { idx -> idx.toDouble() }" -> (
            "kotlin.DoubleArray.<init>",
            "void(int,kotlin.jvm.functions.Function1)"
          )
        ).foreach { case (code, (fullNameBase, signature)) =>
          val List(initCall) = arrayConstructors.ast.isCall.nameExact("<init>").codeExact(code).l: @unchecked
          initCall.methodFullName shouldBe s"$fullNameBase:$signature"
          initCall.signature shouldBe signature
          initCall.typeFullName shouldBe "void"
          initCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = arrayConstructors.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "strings"             -> "java.lang.String[]",
          "ints"                -> "int[]",
          "initializedInts"     -> "int[]",
          "longs"               -> "long[]",
          "chars"               -> "char[]",
          "booleans"            -> "boolean[]",
          "bytes"               -> "byte[]",
          "shorts"              -> "short[]",
          "floats"              -> "float[]",
          "doubles"             -> "double[]",
          "firstString"         -> "java.lang.String",
          "firstInt"            -> "int",
          "firstInitializedInt" -> "int",
          "firstLong"           -> "long",
          "firstChar"           -> "char",
          "firstBoolean"        -> "boolean",
          "firstByte"           -> "byte",
          "firstShort"          -> "short",
          "firstFloat"          -> "float",
          "firstDouble"         -> "double"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }

        Map(
          "strings[0]"         -> "java.lang.String",
          "ints[0]"            -> "int",
          "initializedInts[0]" -> "int",
          "longs[0]"           -> "long",
          "chars[0]"           -> "char",
          "booleans[0]"        -> "boolean",
          "bytes[0]"           -> "byte",
          "shorts[0]"          -> "short",
          "floats[0]"          -> "float",
          "doubles[0]"         -> "double"
        ).foreach { case (code, typeFullName) =>
          val List(indexCall) =
            arrayConstructors.ast.isCall.nameExact(Operators.indexAccess).codeExact(code).l: @unchecked
          indexCall.typeFullName shouldBe typeFullName
        }

        val List(stringLambda, intLambda, longLambda, shortLambda, doubleLambda) =
          cpg.method.fullName("demo.arrayConstructors.<lambda>.*").l.sortBy(_.fullName): @unchecked
        List(
          (stringLambda, "java.lang.String(int)", "it"),
          (intLambda, "int(int)", "it"),
          (longLambda, "long(int)", "idx"),
          (shortLambda, "short(int)", "idx"),
          (doubleLambda, "double(int)", "idx")
        ).foreach { case (lambda, signature, parameterName) =>
          lambda.signature shouldBe signature
          lambda.parameter.name.l shouldBe List(parameterName)
          lambda.parameter.typeFullName.l shouldBe List("int")
          lambda.ast.isIdentifier.nameExact(parameterName).typeFullName.l shouldBe List("int")
          lambda.ast.isIdentifier.nameExact(parameterName).refsTo.l shouldBe lambda.parameter.nameExact(parameterName).l
        }
      }
    }

    "lower method locals and reference identifier uses" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun add(a: Int, b: Int): Int {
          |    val total: Int = a + b
          |    return total
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(add)        = cpg.method.fullNameExact("demo.Foo.add:int(int,int)").l: @unchecked
        val List(totalLocal) = add.ast.isLocal.nameExact("total").l: @unchecked

        totalLocal.typeFullName shouldBe "int"
        add.ast.isCall.nameExact(Operators.assignment).code.l shouldBe List("val total: Int = a + b")
        add.ast.isCall.nameExact(Operators.addition).argument.isIdentifier.nameExact("a").refsTo.l shouldBe
          add.parameter.nameExact("a").l
        add.ast.isCall.nameExact(Operators.addition).argument.isIdentifier.nameExact("b").refsTo.l shouldBe
          add.parameter.nameExact("b").l
        add.ast.isReturn.astChildren.isIdentifier.nameExact("total").refsTo.l shouldBe List(totalLocal)
      }
    }

    "lower primitive literals" in {
      withOxidizedCpg("""package demo
          |
          |val topTiny: Byte = 126
          |fun literalChar() = 'A'
          |fun literalLong() = 9999L
          |fun literalHex() = 0xB4DF00D
          |fun literalBits() = 0b010101
          |
          |class Foo {
          |  fun charLocal(): Char {
          |    val marker = 'A'
          |    return marker
          |  }
          |
          |  fun numericLocals(): Long {
          |    val tiny: Byte = 127
          |    val shorty: Short = 12
          |    val widened: Long = 1
          |    val count = 9999L
          |    val mask = 0xB4DF00D
          |    val bits = 0b010101
          |    return count
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(charLocal)   = cpg.method.nameExact("charLocal").l: @unchecked
        val List(markerLocal) = charLocal.ast.isLocal.nameExact("marker").l: @unchecked

        charLocal.methodReturn.typeFullName shouldBe "char"
        markerLocal.typeFullName shouldBe "char"
        charLocal.ast.isLiteral.codeExact("'A'").typeFullName.l shouldBe List("char")
        charLocal.ast.collectAll[Unknown].codeExact("'A'").l shouldBe empty

        cpg.local.nameExact("topTiny").typeFullName.l shouldBe List("byte")
        cpg.literal.codeExact("126").typeFullName.l shouldBe List("byte")
        cpg.method.fullNameExact("demo.literalChar:char()").methodReturn.typeFullName.l shouldBe List("char")
        cpg.method.fullNameExact("demo.literalLong:long()").methodReturn.typeFullName.l shouldBe List("long")
        cpg.method.fullNameExact("demo.literalHex:int()").methodReturn.typeFullName.l shouldBe List("int")
        cpg.method.fullNameExact("demo.literalBits:int()").methodReturn.typeFullName.l shouldBe List("int")

        val List(numericLocals) = cpg.method.nameExact("numericLocals").l: @unchecked
        numericLocals.methodReturn.typeFullName shouldBe "long"
        numericLocals.ast.isLocal.nameExact("tiny").typeFullName.l shouldBe List("byte")
        numericLocals.ast.isLocal.nameExact("shorty").typeFullName.l shouldBe List("short")
        numericLocals.ast.isLocal.nameExact("widened").typeFullName.l shouldBe List("long")
        numericLocals.ast.isLocal.nameExact("count").typeFullName.l shouldBe List("long")
        numericLocals.ast.isLocal.nameExact("mask").typeFullName.l shouldBe List("int")
        numericLocals.ast.isLocal.nameExact("bits").typeFullName.l shouldBe List("int")
        numericLocals.ast.isLiteral.codeExact("127").typeFullName.l shouldBe List("byte")
        numericLocals.ast.isLiteral.codeExact("12").typeFullName.l shouldBe List("short")
        numericLocals.ast.isLiteral.codeExact("1").typeFullName.l shouldBe List("long")
        numericLocals.ast.isLiteral.codeExact("9999L").typeFullName.l shouldBe List("long")
        numericLocals.ast.isLiteral.codeExact("0xB4DF00D").typeFullName.l shouldBe List("int")
        numericLocals.ast.isLiteral.codeExact("0b010101").typeFullName.l shouldBe List("int")
        numericLocals.ast.collectAll[Unknown].codeExact("9999L").l shouldBe empty
      }
    }

    "lower direct calls and member access expressions" in {
      withOxidizedCpg("""package demo
          |
          |import kotlin.math.max
          |
          |class Foo {
          |  fun compute(label: String, a: Int, b: Int): Int {
          |    val chosen: Int = max(a, b)
          |    println(label.length)
          |    return chosen
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(compute) =
          cpg.method.fullNameExact("demo.Foo.compute:int(java.lang.String,int,int)").l: @unchecked

        val List(maxCall) = compute.ast.isCall.nameExact("max").l: @unchecked
        maxCall.methodFullName shouldBe "kotlin.math.max:int(int,int)"
        maxCall.signature shouldBe "int(int,int)"
        maxCall.typeFullName shouldBe "int"
        maxCall.argument.isIdentifier.nameExact("a").refsTo.l shouldBe compute.parameter.nameExact("a").l
        maxCall.argument.isIdentifier.nameExact("b").refsTo.l shouldBe compute.parameter.nameExact("b").l

        val List(printlnCall) = compute.ast.isCall.nameExact("println").l: @unchecked
        printlnCall.methodFullName shouldBe "kotlin.io.println:void(int)"
        printlnCall.signature shouldBe "void(int)"
        printlnCall.typeFullName shouldBe "void"
        val List(fieldAccess) = printlnCall.argument.isCall.nameExact(Operators.fieldAccess).l: @unchecked
        fieldAccess.code shouldBe "label.length"
        fieldAccess.argument.isIdentifier.nameExact("label").refsTo.l shouldBe compute.parameter.nameExact("label").l
        fieldAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List("length")
      }
    }

    "resolve builtin print calls without overriding user-defined functions" in {
      withOxidizedCpg("""package demo
          |
          |fun println(value: String) {
          |  print(value)
          |}
          |
          |fun caller(value: String, count: Int) {
          |  println(value)
          |  print(count)
          |}
          |""".stripMargin) { cpg =>
        val List(userPrintlnCall) = cpg.call.nameExact("println").codeExact("println(value)").l: @unchecked
        userPrintlnCall.methodFullName shouldBe "demo.println:void(java.lang.String)"
        userPrintlnCall.signature shouldBe "void(java.lang.String)"
        userPrintlnCall.argument.isIdentifier.nameExact("value").refsTo.l shouldBe
          cpg.method.nameExact("caller").parameter.nameExact("value").l

        val List(printStringCall) = cpg.call.nameExact("print").codeExact("print(value)").l: @unchecked
        printStringCall.methodFullName shouldBe "kotlin.io.print:void(java.lang.Object)"
        printStringCall.signature shouldBe "void(java.lang.Object)"

        val List(printIntCall) = cpg.call.nameExact("print").codeExact("print(count)").l: @unchecked
        printIntCall.methodFullName shouldBe "kotlin.io.print:void(int)"
        printIntCall.signature shouldBe "void(int)"
        printIntCall.argument.isIdentifier.nameExact("count").refsTo.l shouldBe
          cpg.method.nameExact("caller").parameter.nameExact("count").l
      }
    }

    "resolve default-imported collection factories and infix to calls" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun build() {
          |    val emptyListValue = emptyList<String>()
          |    val emptyMapValue = emptyMap<String, Int>()
          |    val emptyMapViaMapOf = mapOf<String, Int>()
          |    val emptyMutableMap = mutableMapOf<String, Int>()
          |    val singleMap = mapOf("only" to 1)
          |    val numbersMap = mapOf("key1" to 1, "key2" to 2)
          |    val mutableMapSingle = mutableMapOf("one" to 1)
          |    val mutableMapMany = mutableMapOf("one" to 1, "two" to 2)
          |    val listNotNullSingle = listOfNotNull("one")
          |    val listNotNullMany = listOfNotNull("one", null)
          |    val arrayListEmpty = arrayListOf<String>()
          |    val arrayListSingle = arrayListOf("one")
          |    val arrayListMany = arrayListOf("one", "two")
          |    val singleList = listOf(1)
          |    val values = listOf(1, 2)
          |    val empty = mutableListOf<Int>()
          |    val mutableListSingle = mutableListOf("one")
          |    val mutableListMany = mutableListOf("one", "two")
          |    println(numbersMap)
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(build) = cpg.method.fullNameExact("demo.Foo.build:void()").l: @unchecked

        val List(emptyList) = build.ast.isCall.nameExact("emptyList").codeExact("emptyList<String>()").l: @unchecked
        emptyList.methodFullName shouldBe "kotlin.collections.emptyList:java.util.List()"
        emptyList.signature shouldBe "java.util.List()"
        emptyList.typeFullName shouldBe "java.util.List"

        val List(emptyMap) = build.ast.isCall.nameExact("emptyMap").codeExact("emptyMap<String, Int>()").l: @unchecked
        emptyMap.methodFullName shouldBe "kotlin.collections.emptyMap:java.util.Map()"
        emptyMap.signature shouldBe "java.util.Map()"
        emptyMap.typeFullName shouldBe "java.util.Map"

        val List(emptyMapViaMapOf) =
          build.ast.isCall.nameExact("mapOf").codeExact("mapOf<String, Int>()").l: @unchecked
        emptyMapViaMapOf.methodFullName shouldBe "kotlin.collections.mapOf:java.util.Map()"
        emptyMapViaMapOf.signature shouldBe "java.util.Map()"
        emptyMapViaMapOf.typeFullName shouldBe "java.util.Map"

        val List(emptyMutableMap) =
          build.ast.isCall.nameExact("mutableMapOf").codeExact("mutableMapOf<String, Int>()").l: @unchecked
        emptyMutableMap.methodFullName shouldBe "kotlin.collections.mutableMapOf:java.util.Map()"
        emptyMutableMap.signature shouldBe "java.util.Map()"
        emptyMutableMap.typeFullName shouldBe "java.util.Map"

        val List(singleMap) = build.ast.isCall.nameExact("mapOf").codeExact("""mapOf("only" to 1)""").l: @unchecked
        singleMap.methodFullName shouldBe "kotlin.collections.mapOf:java.util.Map(kotlin.Pair)"
        singleMap.signature shouldBe "java.util.Map(kotlin.Pair)"
        singleMap.typeFullName shouldBe "java.util.Map"

        val List(numbersMap) =
          build.ast.isCall.nameExact("mapOf").codeExact("""mapOf("key1" to 1, "key2" to 2)""").l: @unchecked
        numbersMap.methodFullName shouldBe "kotlin.collections.mapOf:java.util.Map(kotlin.Pair[])"
        numbersMap.signature shouldBe "java.util.Map(kotlin.Pair[])"
        numbersMap.typeFullName shouldBe "java.util.Map"

        val List(mutableMapSingle) =
          build.ast.isCall.nameExact("mutableMapOf").codeExact("""mutableMapOf("one" to 1)""").l: @unchecked
        mutableMapSingle.methodFullName shouldBe "kotlin.collections.mutableMapOf:java.util.Map(kotlin.Pair[])"
        mutableMapSingle.signature shouldBe "java.util.Map(kotlin.Pair[])"
        mutableMapSingle.typeFullName shouldBe "java.util.Map"

        val List(mutableMapMany) =
          build.ast.isCall.nameExact("mutableMapOf").codeExact("""mutableMapOf("one" to 1, "two" to 2)""").l: @unchecked
        mutableMapMany.methodFullName shouldBe "kotlin.collections.mutableMapOf:java.util.Map(kotlin.Pair[])"
        mutableMapMany.signature shouldBe "java.util.Map(kotlin.Pair[])"
        mutableMapMany.typeFullName shouldBe "java.util.Map"

        val toCalls = build.ast.isCall.nameExact("to").l.sortBy(_.code)
        toCalls.map(_.methodFullName).distinct shouldBe List("kotlin.to:kotlin.Pair(java.lang.Object,java.lang.Object)")
        toCalls.map(_.signature).distinct shouldBe List("kotlin.Pair(java.lang.Object,java.lang.Object)")
        toCalls.map(_.typeFullName).distinct shouldBe List("kotlin.Pair")
        toCalls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.STATIC_DISPATCH)

        val List(listNotNullSingle) =
          build.ast.isCall.nameExact("listOfNotNull").codeExact("""listOfNotNull("one")""").l: @unchecked
        listNotNullSingle.methodFullName shouldBe "kotlin.collections.listOfNotNull:java.util.List(java.lang.Object)"
        listNotNullSingle.signature shouldBe "java.util.List(java.lang.Object)"
        listNotNullSingle.typeFullName shouldBe "java.util.List"

        val List(listNotNullMany) =
          build.ast.isCall.nameExact("listOfNotNull").codeExact("""listOfNotNull("one", null)""").l: @unchecked
        listNotNullMany.methodFullName shouldBe "kotlin.collections.listOfNotNull:java.util.List(java.lang.Object[])"
        listNotNullMany.signature shouldBe "java.util.List(java.lang.Object[])"
        listNotNullMany.typeFullName shouldBe "java.util.List"

        val List(arrayListEmpty) =
          build.ast.isCall.nameExact("arrayListOf").codeExact("arrayListOf<String>()").l: @unchecked
        arrayListEmpty.methodFullName shouldBe "kotlin.collections.arrayListOf:java.util.ArrayList()"
        arrayListEmpty.signature shouldBe "java.util.ArrayList()"
        arrayListEmpty.typeFullName shouldBe "java.util.ArrayList"

        val List(arrayListSingle) =
          build.ast.isCall.nameExact("arrayListOf").codeExact("""arrayListOf("one")""").l: @unchecked
        arrayListSingle.methodFullName shouldBe "kotlin.collections.arrayListOf:java.util.ArrayList(java.lang.Object[])"
        arrayListSingle.signature shouldBe "java.util.ArrayList(java.lang.Object[])"
        arrayListSingle.typeFullName shouldBe "java.util.ArrayList"

        val List(arrayListMany) =
          build.ast.isCall.nameExact("arrayListOf").codeExact("""arrayListOf("one", "two")""").l: @unchecked
        arrayListMany.methodFullName shouldBe "kotlin.collections.arrayListOf:java.util.ArrayList(java.lang.Object[])"
        arrayListMany.signature shouldBe "java.util.ArrayList(java.lang.Object[])"
        arrayListMany.typeFullName shouldBe "java.util.ArrayList"

        val List(singleList) = build.ast.isCall.nameExact("listOf").codeExact("listOf(1)").l: @unchecked
        singleList.methodFullName shouldBe "kotlin.collections.listOf:java.util.List(java.lang.Object)"
        singleList.signature shouldBe "java.util.List(java.lang.Object)"
        singleList.typeFullName shouldBe "java.util.List"

        val List(values) = build.ast.isCall.nameExact("listOf").codeExact("listOf(1, 2)").l: @unchecked
        values.methodFullName shouldBe "kotlin.collections.listOf:java.util.List(java.lang.Object[])"
        values.signature shouldBe "java.util.List(java.lang.Object[])"
        values.typeFullName shouldBe "java.util.List"

        val List(empty) = build.ast.isCall.nameExact("mutableListOf").codeExact("mutableListOf<Int>()").l: @unchecked
        empty.methodFullName shouldBe "kotlin.collections.mutableListOf:java.util.List()"
        empty.signature shouldBe "java.util.List()"
        empty.typeFullName shouldBe "java.util.List"

        val List(mutableListSingle) =
          build.ast.isCall.nameExact("mutableListOf").codeExact("""mutableListOf("one")""").l: @unchecked
        mutableListSingle.methodFullName shouldBe "kotlin.collections.mutableListOf:java.util.List(java.lang.Object[])"
        mutableListSingle.signature shouldBe "java.util.List(java.lang.Object[])"
        mutableListSingle.typeFullName shouldBe "java.util.List"

        val List(mutableListMany) =
          build.ast.isCall.nameExact("mutableListOf").codeExact("""mutableListOf("one", "two")""").l: @unchecked
        mutableListMany.methodFullName shouldBe "kotlin.collections.mutableListOf:java.util.List(java.lang.Object[])"
        mutableListMany.signature shouldBe "java.util.List(java.lang.Object[])"
        mutableListMany.typeFullName shouldBe "java.util.List"

        val localTypes = build.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        List(
          "emptyMapValue",
          "emptyMapViaMapOf",
          "emptyMutableMap",
          "singleMap",
          "numbersMap",
          "mutableMapSingle",
          "mutableMapMany"
        ).foreach { name =>
          localTypes should contain(name -> "java.util.Map")
        }
        List(
          "emptyListValue",
          "listNotNullSingle",
          "listNotNullMany",
          "singleList",
          "values",
          "empty",
          "mutableListSingle",
          "mutableListMany"
        ).foreach { name =>
          localTypes should contain(name -> "java.util.List")
        }
        localTypes should contain("arrayListEmpty" -> "java.util.ArrayList")
        localTypes should contain("arrayListSingle" -> "java.util.ArrayList")
        localTypes should contain("arrayListMany" -> "java.util.ArrayList")
      }
    }

    "resolve Pair fields, components, and destructuring" in {
      withOxidizedCpg("""package demo
          |
          |fun pairFields(pair: Pair<String, Int>) {
          |  val fromTo = "name" to 1
          |  val explicit = Pair("other", 2)
          |  val firstFromParam = pair.first
          |  val secondFromParam = pair.second
          |  val firstFromTo = fromTo.first
          |  val secondFromTo = fromTo.second
          |  val firstExplicit = explicit.first
          |  val secondExplicit = explicit.second
          |  val componentFirst = pair.component1()
          |  val componentSecond = pair.component2()
          |  val (destructuredFirst, destructuredSecond) = pair
          |  val (toFirst, toSecond) = fromTo
          |  println(firstFromParam)
          |  println(secondFromParam)
          |  println(firstFromTo)
          |  println(secondFromTo)
          |  println(firstExplicit)
          |  println(secondExplicit)
          |  println(componentFirst)
          |  println(componentSecond)
          |  println(destructuredFirst)
          |  println(destructuredSecond)
          |  println(toFirst)
          |  println(toSecond)
          |}
          |""".stripMargin) { cpg =>
        val List(pairFields) = cpg.method.nameExact("pairFields").l: @unchecked

        pairFields.parameter.nameExact("pair").typeFullName.l shouldBe List("kotlin.Pair")

        val localTypes = pairFields.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "fromTo"             -> "kotlin.Pair",
          "explicit"           -> "kotlin.Pair",
          "firstFromParam"     -> "java.lang.String",
          "secondFromParam"    -> "int",
          "firstFromTo"        -> "java.lang.String",
          "secondFromTo"       -> "int",
          "firstExplicit"      -> "java.lang.String",
          "secondExplicit"     -> "int",
          "componentFirst"     -> "java.lang.String",
          "componentSecond"    -> "int",
          "destructuredFirst"  -> "java.lang.String",
          "destructuredSecond" -> "int",
          "toFirst"            -> "java.lang.String",
          "toSecond"           -> "int"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }

        Map(
          "pair.first"      -> "java.lang.String",
          "pair.second"     -> "int",
          "fromTo.first"    -> "java.lang.String",
          "fromTo.second"   -> "int",
          "explicit.first"  -> "java.lang.String",
          "explicit.second" -> "int"
        ).foreach { case (code, typeFullName) =>
          val List(fieldAccess) =
            pairFields.ast.isCall.nameExact(Operators.fieldAccess).codeExact(code).l: @unchecked
          fieldAccess.typeFullName shouldBe typeFullName
        }

        val component1Calls = pairFields.ast.isCall.nameExact("component1").codeExact("pair.component1()").l
        component1Calls.map(_.methodFullName).distinct shouldBe List("kotlin.Pair.component1:java.lang.Object()")
        component1Calls.map(_.signature).distinct shouldBe List("java.lang.Object()")
        component1Calls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        component1Calls.map(_.typeFullName).distinct shouldBe List("java.lang.String")

        val component2Calls = pairFields.ast.isCall.nameExact("component2").codeExact("pair.component2()").l
        component2Calls.map(_.methodFullName).distinct shouldBe List("kotlin.Pair.component2:java.lang.Object()")
        component2Calls.map(_.signature).distinct shouldBe List("java.lang.Object()")
        component2Calls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        component2Calls.map(_.typeFullName).distinct shouldBe List("int")

        pairFields.ast.isCall.nameExact("component1").codeExact("fromTo.component1()").typeFullName.l shouldBe
          List("java.lang.String")
        pairFields.ast.isCall.nameExact("component2").codeExact("fromTo.component2()").typeFullName.l shouldBe
          List("int")
      }
    }

    "resolve Triple fields, components, and destructuring" in {
      withOxidizedCpg("""package demo
          |
          |fun tripleFields(triple: Triple<String, Int, Boolean>) {
          |  val explicit = Triple("other", 2, true)
          |  val firstFromParam = triple.first
          |  val secondFromParam = triple.second
          |  val thirdFromParam = triple.third
          |  val firstExplicit = explicit.first
          |  val secondExplicit = explicit.second
          |  val thirdExplicit = explicit.third
          |  val componentFirst = triple.component1()
          |  val componentSecond = triple.component2()
          |  val componentThird = triple.component3()
          |  val (destructuredFirst, destructuredSecond, destructuredThird) = triple
          |  val (explicitFirst, explicitSecond, explicitThird) = explicit
          |  println(firstFromParam)
          |  println(secondFromParam)
          |  println(thirdFromParam)
          |  println(firstExplicit)
          |  println(secondExplicit)
          |  println(thirdExplicit)
          |  println(componentFirst)
          |  println(componentSecond)
          |  println(componentThird)
          |  println(destructuredFirst)
          |  println(destructuredSecond)
          |  println(destructuredThird)
          |  println(explicitFirst)
          |  println(explicitSecond)
          |  println(explicitThird)
          |}
          |""".stripMargin) { cpg =>
        val List(tripleFields) = cpg.method.nameExact("tripleFields").l: @unchecked

        tripleFields.parameter.nameExact("triple").typeFullName.l shouldBe List("kotlin.Triple")

        val localTypes = tripleFields.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "explicit"           -> "kotlin.Triple",
          "firstFromParam"     -> "java.lang.String",
          "secondFromParam"    -> "int",
          "thirdFromParam"     -> "boolean",
          "firstExplicit"      -> "java.lang.String",
          "secondExplicit"     -> "int",
          "thirdExplicit"      -> "boolean",
          "componentFirst"     -> "java.lang.String",
          "componentSecond"    -> "int",
          "componentThird"     -> "boolean",
          "destructuredFirst"  -> "java.lang.String",
          "destructuredSecond" -> "int",
          "destructuredThird"  -> "boolean",
          "explicitFirst"      -> "java.lang.String",
          "explicitSecond"     -> "int",
          "explicitThird"      -> "boolean"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }

        Map(
          "triple.first"    -> "java.lang.String",
          "triple.second"   -> "int",
          "triple.third"    -> "boolean",
          "explicit.first"  -> "java.lang.String",
          "explicit.second" -> "int",
          "explicit.third"  -> "boolean"
        ).foreach { case (code, typeFullName) =>
          val List(fieldAccess) =
            tripleFields.ast.isCall.nameExact(Operators.fieldAccess).codeExact(code).l: @unchecked
          fieldAccess.typeFullName shouldBe typeFullName
        }

        val component1Calls = tripleFields.ast.isCall.nameExact("component1").codeExact("triple.component1()").l
        component1Calls.map(_.methodFullName).distinct shouldBe List("kotlin.Triple.component1:java.lang.Object()")
        component1Calls.map(_.signature).distinct shouldBe List("java.lang.Object()")
        component1Calls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        component1Calls.map(_.typeFullName).distinct shouldBe List("java.lang.String")

        val component2Calls = tripleFields.ast.isCall.nameExact("component2").codeExact("triple.component2()").l
        component2Calls.map(_.methodFullName).distinct shouldBe List("kotlin.Triple.component2:java.lang.Object()")
        component2Calls.map(_.signature).distinct shouldBe List("java.lang.Object()")
        component2Calls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        component2Calls.map(_.typeFullName).distinct shouldBe List("int")

        val component3Calls = tripleFields.ast.isCall.nameExact("component3").codeExact("triple.component3()").l
        component3Calls.map(_.methodFullName).distinct shouldBe List("kotlin.Triple.component3:java.lang.Object()")
        component3Calls.map(_.signature).distinct shouldBe List("java.lang.Object()")
        component3Calls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        component3Calls.map(_.typeFullName).distinct shouldBe List("boolean")

        tripleFields.ast.isCall.nameExact("component1").codeExact("explicit.component1()").typeFullName.l shouldBe
          List("java.lang.String")
        tripleFields.ast.isCall.nameExact("component2").codeExact("explicit.component2()").typeFullName.l shouldBe
          List("int")
        tripleFields.ast.isCall.nameExact("component3").codeExact("explicit.component3()").typeFullName.l shouldBe
          List("boolean")
      }
    }

    "resolve set factories and mutable set member calls" in {
      withOxidizedCpg("""package demo
          |
          |fun setFactoryMutation(values: Set<String>, mutableValues: MutableSet<String>) {
          |  val empty = emptySet<String>()
          |  val typedEmpty = setOf<String>()
          |  val single = setOf("one")
          |  val many = setOf("one", "two")
          |  val nullable = setOfNotNull("one", null)
          |  val mutableEmpty = mutableSetOf<String>()
          |  val mutableSingle = mutableSetOf("one")
          |  val hash = hashSetOf("one")
          |  val linked = linkedSetOf("one")
          |  val hasValue = values.contains("one")
          |  val emptyMember = values.isEmpty()
          |  val notEmptyExt = values.isNotEmpty()
          |  val added = mutableValues.add("two")
          |  val removed = mutableValues.remove("two")
          |  val addedAll = mutableValues.addAll(listOf("a"))
          |  val removedAll = mutableValues.removeAll(listOf("b"))
          |  val retainedAll = mutableValues.retainAll(listOf("c"))
          |  mutableValues.clear()
          |  println(empty)
          |  println(typedEmpty)
          |  println(single)
          |  println(many)
          |  println(nullable)
          |  println(mutableEmpty)
          |  println(mutableSingle)
          |  println(hash)
          |  println(linked)
          |  println(hasValue)
          |  println(emptyMember)
          |  println(notEmptyExt)
          |  println(added)
          |  println(removed)
          |  println(addedAll)
          |  println(removedAll)
          |  println(retainedAll)
          |  println(mutableValues)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("emptySet<String>()", "emptySet", "kotlin.collections.emptySet", "java.util.Set()", "java.util.Set"),
          ("setOf<String>()", "setOf", "kotlin.collections.setOf", "java.util.Set()", "java.util.Set"),
          ("""setOf("one")""", "setOf", "kotlin.collections.setOf", "java.util.Set(java.lang.Object)", "java.util.Set"),
          (
            """setOf("one", "two")""",
            "setOf",
            "kotlin.collections.setOf",
            "java.util.Set(java.lang.Object[])",
            "java.util.Set"
          ),
          (
            """setOfNotNull("one", null)""",
            "setOfNotNull",
            "kotlin.collections.setOfNotNull",
            "java.util.Set(java.lang.Object[])",
            "java.util.Set"
          ),
          (
            "mutableSetOf<String>()",
            "mutableSetOf",
            "kotlin.collections.mutableSetOf",
            "java.util.Set()",
            "java.util.Set"
          ),
          (
            """mutableSetOf("one")""",
            "mutableSetOf",
            "kotlin.collections.mutableSetOf",
            "java.util.Set(java.lang.Object[])",
            "java.util.Set"
          ),
          (
            """hashSetOf("one")""",
            "hashSetOf",
            "kotlin.collections.hashSetOf",
            "java.util.HashSet(java.lang.Object[])",
            "java.util.HashSet"
          ),
          (
            """linkedSetOf("one")""",
            "linkedSetOf",
            "kotlin.collections.linkedSetOf",
            "java.util.LinkedHashSet(java.lang.Object[])",
            "java.util.LinkedHashSet"
          )
        ).foreach { case (code, name, fullNameBase, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List(
          ("""values.contains("one")""", "contains", "kotlin.collections.Set.contains", "boolean(java.lang.Object)"),
          ("values.isEmpty()", "isEmpty", "kotlin.collections.Set.isEmpty", "boolean()"),
          ("""mutableValues.add("two")""", "add", "kotlin.collections.MutableSet.add", "boolean(java.lang.Object)"),
          (
            """mutableValues.remove("two")""",
            "remove",
            "kotlin.collections.MutableSet.remove",
            "boolean(java.lang.Object)"
          ),
          (
            """mutableValues.addAll(listOf("a"))""",
            "addAll",
            "kotlin.collections.MutableSet.addAll",
            "boolean(java.util.Collection)"
          ),
          (
            """mutableValues.removeAll(listOf("b"))""",
            "removeAll",
            "kotlin.collections.MutableSet.removeAll",
            "boolean(java.util.Collection)"
          ),
          (
            """mutableValues.retainAll(listOf("c"))""",
            "retainAll",
            "kotlin.collections.MutableSet.retainAll",
            "boolean(java.util.Collection)"
          ),
          ("mutableValues.clear()", "clear", "kotlin.collections.MutableSet.clear", "void()")
        ).foreach { case (code, name, fullNameBase, signature) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe (if (name == "clear") "void" else "boolean")
          call.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        }

        val List(isNotEmptyCall) = cpg.call.nameExact("isNotEmpty").codeExact("values.isNotEmpty()").l: @unchecked
        isNotEmptyCall.methodFullName shouldBe "kotlin.collections.isNotEmpty:boolean(java.util.Collection)"
        isNotEmptyCall.signature shouldBe "boolean(java.util.Collection)"
        isNotEmptyCall.typeFullName shouldBe "boolean"
        isNotEmptyCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        cpg.local
          .nameExact("empty", "typedEmpty", "single", "many", "nullable", "mutableEmpty", "mutableSingle")
          .typeFullName
          .l shouldBe List.fill(7)("java.util.Set")
        cpg.local.nameExact("hash").typeFullName.l shouldBe List("java.util.HashSet")
        cpg.local.nameExact("linked").typeFullName.l shouldBe List("java.util.LinkedHashSet")
        cpg.local
          .nameExact(
            "hasValue",
            "emptyMember",
            "notEmptyExt",
            "added",
            "removed",
            "addedAll",
            "removedAll",
            "retainedAll"
          )
          .typeFullName
          .l shouldBe List.fill(8)("boolean")
      }
    }

    "resolve list member and mutable list calls" in {
      withOxidizedCpg("""package demo
          |
          |fun listMembers(values: List<String>, mutableValues: MutableList<String>) {
          |  val viaGet = values.get(0)
          |  val hasAll = values.containsAll(listOf("one"))
          |  val empty = values.isEmpty()
          |  val notEmpty = values.isNotEmpty()
          |  val index = values.indexOf("one")
          |  val lastIndex = values.lastIndexOf("one")
          |  val added = mutableValues.add("two")
          |  val addedAt = mutableValues.add(0, "zero")
          |  val addedAll = mutableValues.addAll(listOf("a"))
          |  val addedAllAt = mutableValues.addAll(0, listOf("a"))
          |  val removed = mutableValues.remove("two")
          |  val removedAt = mutableValues.removeAt(0)
          |  val removedAll = mutableValues.removeAll(listOf("b"))
          |  val retainedAll = mutableValues.retainAll(listOf("c"))
          |  val previous = mutableValues.set(0, "new")
          |  mutableValues.clear()
          |  println(viaGet)
          |  println(hasAll)
          |  println(empty)
          |  println(notEmpty)
          |  println(index)
          |  println(lastIndex)
          |  println(added)
          |  println(addedAt)
          |  println(addedAll)
          |  println(addedAllAt)
          |  println(removed)
          |  println(removedAt)
          |  println(removedAll)
          |  println(retainedAll)
          |  println(previous)
          |  println(mutableValues)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("values.get(0)", "get", "kotlin.collections.List.get", "java.lang.Object(int)", "java.lang.String"),
          (
            """values.containsAll(listOf("one"))""",
            "containsAll",
            "kotlin.collections.List.containsAll",
            "boolean(java.util.Collection)",
            "boolean"
          ),
          ("values.isEmpty()", "isEmpty", "kotlin.collections.List.isEmpty", "boolean()", "boolean"),
          ("""values.indexOf("one")""", "indexOf", "kotlin.collections.List.indexOf", "int(java.lang.Object)", "int"),
          (
            """values.lastIndexOf("one")""",
            "lastIndexOf",
            "kotlin.collections.List.lastIndexOf",
            "int(java.lang.Object)",
            "int"
          ),
          (
            """mutableValues.add("two")""",
            "add",
            "kotlin.collections.MutableList.add",
            "boolean(java.lang.Object)",
            "boolean"
          ),
          (
            """mutableValues.add(0, "zero")""",
            "add",
            "kotlin.collections.MutableList.add",
            "void(int,java.lang.Object)",
            "void"
          ),
          (
            """mutableValues.addAll(listOf("a"))""",
            "addAll",
            "kotlin.collections.MutableList.addAll",
            "boolean(java.util.Collection)",
            "boolean"
          ),
          (
            """mutableValues.addAll(0, listOf("a"))""",
            "addAll",
            "kotlin.collections.MutableList.addAll",
            "boolean(int,java.util.Collection)",
            "boolean"
          ),
          (
            """mutableValues.remove("two")""",
            "remove",
            "kotlin.collections.MutableList.remove",
            "boolean(java.lang.Object)",
            "boolean"
          ),
          (
            "mutableValues.removeAt(0)",
            "removeAt",
            "kotlin.collections.MutableList.removeAt",
            "java.lang.Object(int)",
            "java.lang.String"
          ),
          (
            """mutableValues.removeAll(listOf("b"))""",
            "removeAll",
            "kotlin.collections.MutableList.removeAll",
            "boolean(java.util.Collection)",
            "boolean"
          ),
          (
            """mutableValues.retainAll(listOf("c"))""",
            "retainAll",
            "kotlin.collections.MutableList.retainAll",
            "boolean(java.util.Collection)",
            "boolean"
          ),
          (
            """mutableValues.set(0, "new")""",
            "set",
            "kotlin.collections.MutableList.set",
            "java.lang.Object(int,java.lang.Object)",
            "java.lang.String"
          ),
          ("mutableValues.clear()", "clear", "kotlin.collections.MutableList.clear", "void()", "void")
        ).foreach { case (code, name, fullNameBase, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        }

        val List(isNotEmptyCall) = cpg.call.nameExact("isNotEmpty").codeExact("values.isNotEmpty()").l: @unchecked
        isNotEmptyCall.methodFullName shouldBe "kotlin.collections.isNotEmpty:boolean(java.util.Collection)"
        isNotEmptyCall.signature shouldBe "boolean(java.util.Collection)"
        isNotEmptyCall.typeFullName shouldBe "boolean"
        isNotEmptyCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val localTypes =
          cpg.local.nameNot("values", "mutableValues").map(local => local.name -> local.typeFullName).toMap
        localTypes should contain("viaGet" -> "java.lang.String")
        localTypes should contain("removedAt" -> "java.lang.String")
        localTypes should contain("previous" -> "java.lang.String")
        localTypes should contain("addedAt" -> "void")
        List("hasAll", "empty", "notEmpty", "added", "addedAll", "addedAllAt", "removed", "removedAll", "retainedAll")
          .foreach { name =>
            localTypes should contain(name -> "boolean")
          }
        localTypes should contain("index" -> "int")
        localTypes should contain("lastIndex" -> "int")
      }
    }

    "resolve collection property field access types" in {
      withOxidizedCpg("""package demo
          |
          |fun collectionProperties(values: List<String>, mutableValues: MutableList<String>, setValues: Set<String>, collectionValues: Collection<String>) {
          |  val listSize = values.size
          |  val mutableListSize = mutableValues.size
          |  val setSize = setValues.size
          |  val collectionSize = collectionValues.size
          |  val listIndices = values.indices
          |  val mutableListIndices = mutableValues.indices
          |  val listLastIndex = values.lastIndex
          |  val mutableListLastIndex = mutableValues.lastIndex
          |  println(listSize)
          |  println(mutableListSize)
          |  println(setSize)
          |  println(collectionSize)
          |  println(listIndices)
          |  println(mutableListIndices)
          |  println(listLastIndex)
          |  println(mutableListLastIndex)
          |}
          |""".stripMargin) { cpg =>
        List(
          "values.size",
          "mutableValues.size",
          "setValues.size",
          "collectionValues.size",
          "values.lastIndex",
          "mutableValues.lastIndex"
        ).foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.fieldAccess).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.fieldAccess
          call.typeFullName shouldBe "int"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List("values.indices", "mutableValues.indices").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.fieldAccess).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.fieldAccess
          call.typeFullName shouldBe "kotlin.ranges.IntRange"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "listSize",
            "mutableListSize",
            "setSize",
            "collectionSize",
            "listLastIndex",
            "mutableListLastIndex"
          )
          .typeFullName
          .l shouldBe List.fill(6)("int")
        cpg.local.nameExact("listIndices", "mutableListIndices").typeFullName.l shouldBe
          List.fill(2)("kotlin.ranges.IntRange")
      }
    }

    "resolve array property field access types" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayProperties(strings: Array<String>, ints: IntArray, size: Int) {
          |  val generated = ByteArray(size)
          |  val stringSize = strings.size
          |  val intSize = ints.size
          |  val generatedSize = generated.size
          |  val stringIndices = strings.indices
          |  val intIndices = ints.indices
          |  val stringLastIndex = strings.lastIndex
          |  val intLastIndex = ints.lastIndex
          |  println(stringSize)
          |  println(intSize)
          |  println(generatedSize)
          |  println(stringIndices)
          |  println(intIndices)
          |  println(stringLastIndex)
          |  println(intLastIndex)
          |}
          |""".stripMargin) { cpg =>
        List("strings.size", "ints.size", "generated.size", "strings.lastIndex", "ints.lastIndex").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.fieldAccess).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.fieldAccess
          call.typeFullName shouldBe "int"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List("strings.indices", "ints.indices").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.fieldAccess).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.fieldAccess
          call.typeFullName shouldBe "kotlin.ranges.IntRange"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact("stringSize", "intSize", "generatedSize", "stringLastIndex", "intLastIndex")
          .typeFullName
          .l shouldBe List.fill(5)("int")
        cpg.local.nameExact("stringIndices", "intIndices").typeFullName.l shouldBe
          List.fill(2)("kotlin.ranges.IntRange")
      }
    }

    "resolve array member calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayMembers(strings: Array<String>, ints: IntArray) {
          |  val sGet = strings.get(0)
          |  val iGet = ints.get(0)
          |  val sSet = strings.set(0, "x")
          |  val iSet = ints.set(0, 1)
          |  val sContains = strings.contains("x")
          |  val iContains = ints.contains(1)
          |  val sIndex = strings.indexOf("x")
          |  val iIndex = ints.indexOf(1)
          |  val sLastIndex = strings.lastIndexOf("x")
          |  val iLastIndex = ints.lastIndexOf(1)
          |  val sEmpty = strings.isEmpty()
          |  val iEmpty = ints.isEmpty()
          |  val sNotEmpty = strings.isNotEmpty()
          |  val iNotEmpty = ints.isNotEmpty()
          |  val sIterator = strings.iterator()
          |  val iIterator = ints.iterator()
          |}
          |""".stripMargin) { cpg =>
        val List(arrayMembers) = cpg.method.nameExact("arrayMembers").l: @unchecked

        List(
          (
            "strings.get(0)",
            "get",
            "kotlin.Array.get:java.lang.Object(int)",
            "java.lang.Object(int)",
            "java.lang.String",
            DispatchTypes.DYNAMIC_DISPATCH
          ),
          ("ints.get(0)", "get", "kotlin.IntArray.get:int(int)", "int(int)", "int", DispatchTypes.DYNAMIC_DISPATCH),
          (
            """strings.set(0, "x")""",
            "set",
            "kotlin.Array.set:void(int,java.lang.Object)",
            "void(int,java.lang.Object)",
            "void",
            DispatchTypes.DYNAMIC_DISPATCH
          ),
          (
            "ints.set(0, 1)",
            "set",
            "kotlin.IntArray.set:void(int,int)",
            "void(int,int)",
            "void",
            DispatchTypes.DYNAMIC_DISPATCH
          ),
          (
            "strings.iterator()",
            "iterator",
            "kotlin.Array.iterator:java.util.Iterator()",
            "java.util.Iterator()",
            "java.util.Iterator",
            DispatchTypes.DYNAMIC_DISPATCH
          ),
          (
            "ints.iterator()",
            "iterator",
            "kotlin.IntArray.iterator:kotlin.collections.IntIterator()",
            "kotlin.collections.IntIterator()",
            "kotlin.collections.IntIterator",
            DispatchTypes.DYNAMIC_DISPATCH
          ),
          (
            """strings.contains("x")""",
            "contains",
            "kotlin.collections.contains:boolean(java.lang.Object[],java.lang.Object)",
            "boolean(java.lang.Object[],java.lang.Object)",
            "boolean",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            "ints.contains(1)",
            "contains",
            "kotlin.collections.contains:boolean(int[],int)",
            "boolean(int[],int)",
            "boolean",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            """strings.indexOf("x")""",
            "indexOf",
            "kotlin.collections.indexOf:int(java.lang.Object[],java.lang.Object)",
            "int(java.lang.Object[],java.lang.Object)",
            "int",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            "ints.indexOf(1)",
            "indexOf",
            "kotlin.collections.indexOf:int(int[],int)",
            "int(int[],int)",
            "int",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            """strings.lastIndexOf("x")""",
            "lastIndexOf",
            "kotlin.collections.lastIndexOf:int(java.lang.Object[],java.lang.Object)",
            "int(java.lang.Object[],java.lang.Object)",
            "int",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            "ints.lastIndexOf(1)",
            "lastIndexOf",
            "kotlin.collections.lastIndexOf:int(int[],int)",
            "int(int[],int)",
            "int",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            "strings.isEmpty()",
            "isEmpty",
            "kotlin.collections.isEmpty:boolean(java.lang.Object[])",
            "boolean(java.lang.Object[])",
            "boolean",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            "ints.isEmpty()",
            "isEmpty",
            "kotlin.collections.isEmpty:boolean(int[])",
            "boolean(int[])",
            "boolean",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            "strings.isNotEmpty()",
            "isNotEmpty",
            "kotlin.collections.isNotEmpty:boolean(java.lang.Object[])",
            "boolean(java.lang.Object[])",
            "boolean",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            "ints.isNotEmpty()",
            "isNotEmpty",
            "kotlin.collections.isNotEmpty:boolean(int[])",
            "boolean(int[])",
            "boolean",
            DispatchTypes.STATIC_DISPATCH
          )
        ).foreach { case (code, name, fullName, signature, typeFullName, dispatchType) =>
          val List(call) = arrayMembers.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe fullName
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe dispatchType
        }

        val localTypes = arrayMembers.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "sGet"       -> "java.lang.String",
          "iGet"       -> "int",
          "sSet"       -> "void",
          "iSet"       -> "void",
          "sContains"  -> "boolean",
          "iContains"  -> "boolean",
          "sIndex"     -> "int",
          "iIndex"     -> "int",
          "sLastIndex" -> "int",
          "iLastIndex" -> "int",
          "sEmpty"     -> "boolean",
          "iEmpty"     -> "boolean",
          "sNotEmpty"  -> "boolean",
          "iNotEmpty"  -> "boolean",
          "sIterator"  -> "java.util.Iterator",
          "iIterator"  -> "kotlin.collections.IntIterator"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve array extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayExtensions(strings: Array<String>, ints: IntArray) {
          |  val sFirst = strings.first()
          |  val iFirst = ints.first()
          |  val sFirstPred = strings.first { it.isNotEmpty() }
          |  val iFirstPred = ints.first { it > 0 }
          |  val sMap = strings.map { it.length }
          |  val iMap = ints.map { it.toString() }
          |  val sFilter = strings.filter { it.isNotEmpty() }
          |  val iFilter = ints.filter { it > 0 }
          |  val sForEach = strings.forEach { println(it) }
          |  val iForEach = ints.forEach { println(it) }
          |  val sAny = strings.any()
          |  val iAny = ints.any()
          |  val sAnyPred = strings.any { it.isNotEmpty() }
          |  val iAnyPred = ints.any { it > 0 }
          |  val sCount = strings.count()
          |  val iCount = ints.count()
          |  val sToList = strings.toList()
          |  val iToList = ints.toList()
          |  val sAsSequence = strings.asSequence()
          |  val iAsSequence = ints.asSequence()
          |}
          |""".stripMargin) { cpg =>
        val List(arrayExtensions) = cpg.method.nameExact("arrayExtensions").l: @unchecked

        List(
          (
            "strings.first()",
            "first",
            "kotlin.collections.first:java.lang.Object(java.lang.Object[])",
            "java.lang.Object(java.lang.Object[])",
            "java.lang.String"
          ),
          ("ints.first()", "first", "kotlin.collections.first:int(int[])", "int(int[])", "int"),
          (
            "strings.first { it.isNotEmpty() }",
            "first",
            "kotlin.collections.first:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "ints.first { it > 0 }",
            "first",
            "kotlin.collections.first:int(int[],kotlin.jvm.functions.Function1)",
            "int(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.map { it.length }",
            "map",
            "kotlin.collections.map:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.map { it.toString() }",
            "map",
            "kotlin.collections.map:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.filter { it.isNotEmpty() }",
            "filter",
            "kotlin.collections.filter:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.filter { it > 0 }",
            "filter",
            "kotlin.collections.filter:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.forEach { println(it) }",
            "forEach",
            "kotlin.collections.forEach:void(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "void(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "void"
          ),
          (
            "ints.forEach { println(it) }",
            "forEach",
            "kotlin.collections.forEach:void(int[],kotlin.jvm.functions.Function1)",
            "void(int[],kotlin.jvm.functions.Function1)",
            "void"
          ),
          (
            "strings.any()",
            "any",
            "kotlin.collections.any:boolean(java.lang.Object[])",
            "boolean(java.lang.Object[])",
            "boolean"
          ),
          ("ints.any()", "any", "kotlin.collections.any:boolean(int[])", "boolean(int[])", "boolean"),
          (
            "strings.any { it.isNotEmpty() }",
            "any",
            "kotlin.collections.any:boolean(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "boolean(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          (
            "ints.any { it > 0 }",
            "any",
            "kotlin.collections.any:boolean(int[],kotlin.jvm.functions.Function1)",
            "boolean(int[],kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          (
            "strings.count()",
            "count",
            "kotlin.collections.count:int(java.lang.Object[])",
            "int(java.lang.Object[])",
            "int"
          ),
          ("ints.count()", "count", "kotlin.collections.count:int(int[])", "int(int[])", "int"),
          (
            "strings.toList()",
            "toList",
            "kotlin.collections.toList:java.util.List(java.lang.Object[])",
            "java.util.List(java.lang.Object[])",
            "java.util.List"
          ),
          (
            "ints.toList()",
            "toList",
            "kotlin.collections.toList:java.util.List(int[])",
            "java.util.List(int[])",
            "java.util.List"
          ),
          (
            "strings.asSequence()",
            "asSequence",
            "kotlin.collections.asSequence:kotlin.sequences.Sequence(java.lang.Object[])",
            "kotlin.sequences.Sequence(java.lang.Object[])",
            "kotlin.sequences.Sequence"
          ),
          (
            "ints.asSequence()",
            "asSequence",
            "kotlin.collections.asSequence:kotlin.sequences.Sequence(int[])",
            "kotlin.sequences.Sequence(int[])",
            "kotlin.sequences.Sequence"
          )
        ).foreach { case (code, name, fullName, signature, typeFullName) =>
          val List(call) = arrayExtensions.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe fullName
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = arrayExtensions.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "sFirst"      -> "java.lang.String",
          "iFirst"      -> "int",
          "sFirstPred"  -> "java.lang.String",
          "iFirstPred"  -> "int",
          "sMap"        -> "java.util.List",
          "iMap"        -> "java.util.List",
          "sFilter"     -> "java.util.List",
          "iFilter"     -> "java.util.List",
          "sForEach"    -> "void",
          "iForEach"    -> "void",
          "sAny"        -> "boolean",
          "iAny"        -> "boolean",
          "sAnyPred"    -> "boolean",
          "iAnyPred"    -> "boolean",
          "sCount"      -> "int",
          "iCount"      -> "int",
          "sToList"     -> "java.util.List",
          "iToList"     -> "java.util.List",
          "sAsSequence" -> "kotlin.sequences.Sequence",
          "iAsSequence" -> "kotlin.sequences.Sequence"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve additional array extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayExtensionMore(strings: Array<String>, nullableStrings: Array<String?>, ints: IntArray) {
          |  val sElementAt = strings.elementAt(0)
          |  val iElementAt = ints.elementAt(0)
          |  val sElementAtOrNull = strings.elementAtOrNull(0)
          |  val iElementAtOrNull = ints.elementAtOrNull(0)
          |  val sElementAtOrElse = strings.elementAtOrElse(0) { "fallback" }
          |  val iElementAtOrElse = ints.elementAtOrElse(0) { -1 }
          |  val sIndexFirst = strings.indexOfFirst { it.isNotEmpty() }
          |  val iIndexFirst = ints.indexOfFirst { it > 0 }
          |  val sIndexLast = strings.indexOfLast { it.isNotEmpty() }
          |  val iIndexLast = ints.indexOfLast { it > 0 }
          |  val sFilterIndexed = strings.filterIndexed { index, item -> index > 0 && item.isNotEmpty() }
          |  val iFilterIndexed = ints.filterIndexed { index, item -> index > 0 && item > 0 }
          |  val sFlatMapIndexed = strings.flatMapIndexed { index, item -> listOf(item + index.toString()) }
          |  val iFlatMapIndexed = ints.flatMapIndexed { index, item -> listOf(item + index) }
          |  val sMapIndexed = strings.mapIndexed { index, item -> item + index.toString() }
          |  val iMapIndexed = ints.mapIndexed { index, item -> item + index }
          |  val sMapIndexedNotNull = strings.mapIndexedNotNull { index, item -> item + index.toString() }
          |  val iMapIndexedNotNull = ints.mapIndexedNotNull { index, item -> item + index }
          |  val sOnEach = strings.onEach { println(it) }
          |  val iOnEach = ints.onEach { println(it) }
          |  val sOnEachIndexed = strings.onEachIndexed { index, item -> println(item + index.toString()) }
          |  val iOnEachIndexed = ints.onEachIndexed { index, item -> println(item + index) }
          |  val sForEachIndexed = strings.forEachIndexed { index, item -> println(item + index.toString()) }
          |  val iForEachIndexed = ints.forEachIndexed { index, item -> println(item + index) }
          |  val sDrop = strings.drop(1)
          |  val iDrop = ints.drop(1)
          |  val sTake = strings.take(1)
          |  val iTake = ints.take(1)
          |  val sDropLast = strings.dropLast(1)
          |  val iDropLast = ints.dropLast(1)
          |  val sTakeLast = strings.takeLast(1)
          |  val iTakeLast = ints.takeLast(1)
          |  val sDropWhile = strings.dropWhile { it.isNotEmpty() }
          |  val iDropWhile = ints.dropWhile { it > 0 }
          |  val sTakeWhile = strings.takeWhile { it.isNotEmpty() }
          |  val iTakeWhile = ints.takeWhile { it > 0 }
          |  val sDropLastWhile = strings.dropLastWhile { it.isEmpty() }
          |  val iDropLastWhile = ints.dropLastWhile { it == 0 }
          |  val sTakeLastWhile = strings.takeLastWhile { it.isNotEmpty() }
          |  val iTakeLastWhile = ints.takeLastWhile { it > 0 }
          |  val sFilterNotNull = nullableStrings.filterNotNull()
          |  val sAsIterable = strings.asIterable()
          |  val iAsIterable = ints.asIterable()
          |  val sWithIndex = strings.withIndex()
          |  val iWithIndex = ints.withIndex()
          |  val sToSet = strings.toSet()
          |  val iToSet = ints.toSet()
          |  val sToMutableList = strings.toMutableList()
          |  val iToMutableList = ints.toMutableList()
          |  val sToMutableSet = strings.toMutableSet()
          |  val iToMutableSet = ints.toMutableSet()
          |  val sToHashSet = strings.toHashSet()
          |  val iToHashSet = ints.toHashSet()
          |  val sToCollection = strings.toCollection(mutableListOf<String>())
          |  val iToCollection = ints.toCollection(mutableListOf<Int>())
          |}
          |""".stripMargin) { cpg =>
        val List(arrayExtensionMore) = cpg.method.nameExact("arrayExtensionMore").l: @unchecked

        List(
          (
            "strings.elementAt(0)",
            "elementAt",
            "kotlin.collections.elementAt:java.lang.Object(java.lang.Object[],int)",
            "java.lang.Object(java.lang.Object[],int)",
            "java.lang.String"
          ),
          ("ints.elementAt(0)", "elementAt", "kotlin.collections.elementAt:int(int[],int)", "int(int[],int)", "int"),
          (
            "strings.elementAtOrNull(0)",
            "elementAtOrNull",
            "kotlin.collections.elementAtOrNull:java.lang.Object(java.lang.Object[],int)",
            "java.lang.Object(java.lang.Object[],int)",
            "java.lang.String"
          ),
          (
            "ints.elementAtOrNull(0)",
            "elementAtOrNull",
            "kotlin.collections.elementAtOrNull:int(int[],int)",
            "int(int[],int)",
            "int"
          ),
          (
            """strings.elementAtOrElse(0) { "fallback" }""",
            "elementAtOrElse",
            "kotlin.collections.elementAtOrElse:java.lang.Object(java.lang.Object[],int,kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Object[],int,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "ints.elementAtOrElse(0) { -1 }",
            "elementAtOrElse",
            "kotlin.collections.elementAtOrElse:int(int[],int,kotlin.jvm.functions.Function1)",
            "int(int[],int,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.indexOfFirst { it.isNotEmpty() }",
            "indexOfFirst",
            "kotlin.collections.indexOfFirst:int(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "ints.indexOfFirst { it > 0 }",
            "indexOfFirst",
            "kotlin.collections.indexOfFirst:int(int[],kotlin.jvm.functions.Function1)",
            "int(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.indexOfLast { it.isNotEmpty() }",
            "indexOfLast",
            "kotlin.collections.indexOfLast:int(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "ints.indexOfLast { it > 0 }",
            "indexOfLast",
            "kotlin.collections.indexOfLast:int(int[],kotlin.jvm.functions.Function1)",
            "int(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.filterIndexed { index, item -> index > 0 && item.isNotEmpty() }",
            "filterIndexed",
            "kotlin.collections.filterIndexed:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "ints.filterIndexed { index, item -> index > 0 && item > 0 }",
            "filterIndexed",
            "kotlin.collections.filterIndexed:java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "strings.flatMapIndexed { index, item -> listOf(item + index.toString()) }",
            "flatMapIndexed",
            "kotlin.collections.flatMapIndexed:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "ints.flatMapIndexed { index, item -> listOf(item + index) }",
            "flatMapIndexed",
            "kotlin.collections.flatMapIndexed:java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "strings.mapIndexed { index, item -> item + index.toString() }",
            "mapIndexed",
            "kotlin.collections.mapIndexed:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "ints.mapIndexed { index, item -> item + index }",
            "mapIndexed",
            "kotlin.collections.mapIndexed:java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "strings.mapIndexedNotNull { index, item -> item + index.toString() }",
            "mapIndexedNotNull",
            "kotlin.collections.mapIndexedNotNull:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "ints.mapIndexedNotNull { index, item -> item + index }",
            "mapIndexedNotNull",
            "kotlin.collections.mapIndexedNotNull:java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List(int[],kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "strings.onEach { println(it) }",
            "onEach",
            "kotlin.collections.onEach:java.lang.Object[](java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Object[](java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.String[]"
          ),
          (
            "ints.onEach { println(it) }",
            "onEach",
            "kotlin.collections.onEach:int[](int[],kotlin.jvm.functions.Function1)",
            "int[](int[],kotlin.jvm.functions.Function1)",
            "int[]"
          ),
          (
            "strings.onEachIndexed { index, item -> println(item + index.toString()) }",
            "onEachIndexed",
            "kotlin.collections.onEachIndexed:java.lang.Object[](java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.Object[](java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.String[]"
          ),
          (
            "ints.onEachIndexed { index, item -> println(item + index) }",
            "onEachIndexed",
            "kotlin.collections.onEachIndexed:int[](int[],kotlin.jvm.functions.Function2)",
            "int[](int[],kotlin.jvm.functions.Function2)",
            "int[]"
          ),
          (
            "strings.forEachIndexed { index, item -> println(item + index.toString()) }",
            "forEachIndexed",
            "kotlin.collections.forEachIndexed:void(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "void(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "void"
          ),
          (
            "ints.forEachIndexed { index, item -> println(item + index) }",
            "forEachIndexed",
            "kotlin.collections.forEachIndexed:void(int[],kotlin.jvm.functions.Function2)",
            "void(int[],kotlin.jvm.functions.Function2)",
            "void"
          ),
          (
            "strings.drop(1)",
            "drop",
            "kotlin.collections.drop:java.util.List(java.lang.Object[],int)",
            "java.util.List(java.lang.Object[],int)",
            "java.util.List"
          ),
          (
            "ints.drop(1)",
            "drop",
            "kotlin.collections.drop:java.util.List(int[],int)",
            "java.util.List(int[],int)",
            "java.util.List"
          ),
          (
            "strings.take(1)",
            "take",
            "kotlin.collections.take:java.util.List(java.lang.Object[],int)",
            "java.util.List(java.lang.Object[],int)",
            "java.util.List"
          ),
          (
            "ints.take(1)",
            "take",
            "kotlin.collections.take:java.util.List(int[],int)",
            "java.util.List(int[],int)",
            "java.util.List"
          ),
          (
            "strings.dropLast(1)",
            "dropLast",
            "kotlin.collections.dropLast:java.util.List(java.lang.Object[],int)",
            "java.util.List(java.lang.Object[],int)",
            "java.util.List"
          ),
          (
            "ints.dropLast(1)",
            "dropLast",
            "kotlin.collections.dropLast:java.util.List(int[],int)",
            "java.util.List(int[],int)",
            "java.util.List"
          ),
          (
            "strings.takeLast(1)",
            "takeLast",
            "kotlin.collections.takeLast:java.util.List(java.lang.Object[],int)",
            "java.util.List(java.lang.Object[],int)",
            "java.util.List"
          ),
          (
            "ints.takeLast(1)",
            "takeLast",
            "kotlin.collections.takeLast:java.util.List(int[],int)",
            "java.util.List(int[],int)",
            "java.util.List"
          ),
          (
            "strings.dropWhile { it.isNotEmpty() }",
            "dropWhile",
            "kotlin.collections.dropWhile:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.dropWhile { it > 0 }",
            "dropWhile",
            "kotlin.collections.dropWhile:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.takeWhile { it.isNotEmpty() }",
            "takeWhile",
            "kotlin.collections.takeWhile:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.takeWhile { it > 0 }",
            "takeWhile",
            "kotlin.collections.takeWhile:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.dropLastWhile { it.isEmpty() }",
            "dropLastWhile",
            "kotlin.collections.dropLastWhile:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.dropLastWhile { it == 0 }",
            "dropLastWhile",
            "kotlin.collections.dropLastWhile:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.takeLastWhile { it.isNotEmpty() }",
            "takeLastWhile",
            "kotlin.collections.takeLastWhile:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.takeLastWhile { it > 0 }",
            "takeLastWhile",
            "kotlin.collections.takeLastWhile:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "nullableStrings.filterNotNull()",
            "filterNotNull",
            "kotlin.collections.filterNotNull:java.util.List(java.lang.Object[])",
            "java.util.List(java.lang.Object[])",
            "java.util.List"
          ),
          (
            "strings.asIterable()",
            "asIterable",
            "kotlin.collections.asIterable:java.lang.Iterable(java.lang.Object[])",
            "java.lang.Iterable(java.lang.Object[])",
            "java.lang.Iterable"
          ),
          (
            "ints.asIterable()",
            "asIterable",
            "kotlin.collections.asIterable:java.lang.Iterable(int[])",
            "java.lang.Iterable(int[])",
            "java.lang.Iterable"
          ),
          (
            "strings.withIndex()",
            "withIndex",
            "kotlin.collections.withIndex:java.lang.Iterable(java.lang.Object[])",
            "java.lang.Iterable(java.lang.Object[])",
            "java.lang.Iterable"
          ),
          (
            "ints.withIndex()",
            "withIndex",
            "kotlin.collections.withIndex:java.lang.Iterable(int[])",
            "java.lang.Iterable(int[])",
            "java.lang.Iterable"
          ),
          (
            "strings.toSet()",
            "toSet",
            "kotlin.collections.toSet:java.util.Set(java.lang.Object[])",
            "java.util.Set(java.lang.Object[])",
            "java.util.Set"
          ),
          (
            "ints.toSet()",
            "toSet",
            "kotlin.collections.toSet:java.util.Set(int[])",
            "java.util.Set(int[])",
            "java.util.Set"
          ),
          (
            "strings.toMutableList()",
            "toMutableList",
            "kotlin.collections.toMutableList:java.util.List(java.lang.Object[])",
            "java.util.List(java.lang.Object[])",
            "java.util.List"
          ),
          (
            "ints.toMutableList()",
            "toMutableList",
            "kotlin.collections.toMutableList:java.util.List(int[])",
            "java.util.List(int[])",
            "java.util.List"
          ),
          (
            "strings.toMutableSet()",
            "toMutableSet",
            "kotlin.collections.toMutableSet:java.util.Set(java.lang.Object[])",
            "java.util.Set(java.lang.Object[])",
            "java.util.Set"
          ),
          (
            "ints.toMutableSet()",
            "toMutableSet",
            "kotlin.collections.toMutableSet:java.util.Set(int[])",
            "java.util.Set(int[])",
            "java.util.Set"
          ),
          (
            "strings.toHashSet()",
            "toHashSet",
            "kotlin.collections.toHashSet:java.util.HashSet(java.lang.Object[])",
            "java.util.HashSet(java.lang.Object[])",
            "java.util.HashSet"
          ),
          (
            "ints.toHashSet()",
            "toHashSet",
            "kotlin.collections.toHashSet:java.util.HashSet(int[])",
            "java.util.HashSet(int[])",
            "java.util.HashSet"
          ),
          (
            "strings.toCollection(mutableListOf<String>())",
            "toCollection",
            "kotlin.collections.toCollection:java.util.Collection(java.lang.Object[],java.util.Collection)",
            "java.util.Collection(java.lang.Object[],java.util.Collection)",
            "java.util.List"
          ),
          (
            "ints.toCollection(mutableListOf<Int>())",
            "toCollection",
            "kotlin.collections.toCollection:java.util.Collection(int[],java.util.Collection)",
            "java.util.Collection(int[],java.util.Collection)",
            "java.util.List"
          )
        ).foreach { case (code, name, fullName, signature, typeFullName) =>
          val List(call) = arrayExtensionMore.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe fullName
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = arrayExtensionMore.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "sElementAt"         -> "java.lang.String",
          "iElementAt"         -> "int",
          "sElementAtOrNull"   -> "java.lang.String",
          "iElementAtOrNull"   -> "int",
          "sElementAtOrElse"   -> "java.lang.String",
          "iElementAtOrElse"   -> "int",
          "sIndexFirst"        -> "int",
          "iIndexFirst"        -> "int",
          "sIndexLast"         -> "int",
          "iIndexLast"         -> "int",
          "sFilterIndexed"     -> "java.util.List",
          "iFilterIndexed"     -> "java.util.List",
          "sFlatMapIndexed"    -> "java.util.List",
          "iFlatMapIndexed"    -> "java.util.List",
          "sMapIndexed"        -> "java.util.List",
          "iMapIndexed"        -> "java.util.List",
          "sMapIndexedNotNull" -> "java.util.List",
          "iMapIndexedNotNull" -> "java.util.List",
          "sOnEach"            -> "java.lang.String[]",
          "iOnEach"            -> "int[]",
          "sOnEachIndexed"     -> "java.lang.String[]",
          "iOnEachIndexed"     -> "int[]",
          "sForEachIndexed"    -> "void",
          "iForEachIndexed"    -> "void",
          "sDrop"              -> "java.util.List",
          "iDrop"              -> "java.util.List",
          "sTake"              -> "java.util.List",
          "iTake"              -> "java.util.List",
          "sDropLast"          -> "java.util.List",
          "iDropLast"          -> "java.util.List",
          "sTakeLast"          -> "java.util.List",
          "iTakeLast"          -> "java.util.List",
          "sDropWhile"         -> "java.util.List",
          "iDropWhile"         -> "java.util.List",
          "sTakeWhile"         -> "java.util.List",
          "iTakeWhile"         -> "java.util.List",
          "sDropLastWhile"     -> "java.util.List",
          "iDropLastWhile"     -> "java.util.List",
          "sTakeLastWhile"     -> "java.util.List",
          "iTakeLastWhile"     -> "java.util.List",
          "sFilterNotNull"     -> "java.util.List",
          "sAsIterable"        -> "java.lang.Iterable",
          "iAsIterable"        -> "java.lang.Iterable",
          "sWithIndex"         -> "java.lang.Iterable",
          "iWithIndex"         -> "java.lang.Iterable",
          "sToSet"             -> "java.util.Set",
          "iToSet"             -> "java.util.Set",
          "sToMutableList"     -> "java.util.List",
          "iToMutableList"     -> "java.util.List",
          "sToMutableSet"      -> "java.util.Set",
          "iToMutableSet"      -> "java.util.Set",
          "sToHashSet"         -> "java.util.HashSet",
          "iToHashSet"         -> "java.util.HashSet",
          "sToCollection"      -> "java.util.List",
          "iToCollection"      -> "java.util.List"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve array aggregation and ordering extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayAggregations(strings: Array<String>, ints: IntArray) {
          |  val iSum = ints.sum()
          |  val iAverage = ints.average()
          |  val iMin = ints.minOrNull()
          |  val iMax = ints.maxOrNull()
          |  val sMin = strings.minOrNull()
          |  val sMax = strings.maxOrNull()
          |  val sMaxBy = strings.maxByOrNull { it.length }
          |  val iMaxBy = ints.maxByOrNull { it }
          |  val sMinBy = strings.minByOrNull { it.length }
          |  val iMinBy = ints.minByOrNull { it }
          |  val sMaxOf = strings.maxOf { it.length }
          |  val iMaxOf = ints.maxOf { it }
          |  val sMaxOfOrNull = strings.maxOfOrNull { it.length }
          |  val iMaxOfOrNull = ints.maxOfOrNull { it }
          |  val sSumOf = strings.sumOf { it.length }
          |  val iSumOf = ints.sumOf { it }
          |  val sReduce = strings.reduce { acc, item -> acc + item }
          |  val iReduce = ints.reduce { acc, item -> acc + item }
          |  val sReduceOrNull = strings.reduceOrNull { acc, item -> acc + item }
          |  val iReduceOrNull = ints.reduceOrNull { acc, item -> acc + item }
          |  val sReduceIndexed = strings.reduceIndexed { index, acc, item -> acc + item + index.toString() }
          |  val iReduceIndexed = ints.reduceIndexed { index, acc, item -> acc + item + index }
          |  val sReduceIndexedOrNull = strings.reduceIndexedOrNull { index, acc, item -> acc + item + index.toString() }
          |  val iReduceIndexedOrNull = ints.reduceIndexedOrNull { index, acc, item -> acc + item + index }
          |  val sReduceRight = strings.reduceRight { item, acc -> acc + item }
          |  val iReduceRight = ints.reduceRight { item, acc -> acc + item }
          |  val sReduceRightOrNull = strings.reduceRightOrNull { item, acc -> acc + item }
          |  val iReduceRightOrNull = ints.reduceRightOrNull { item, acc -> acc + item }
          |  val sReduceRightIndexed = strings.reduceRightIndexed { index, item, acc -> acc + item + index.toString() }
          |  val iReduceRightIndexed = ints.reduceRightIndexed { index, item, acc -> acc + item + index }
          |  val sReduceRightIndexedOrNull = strings.reduceRightIndexedOrNull { index, item, acc -> acc + item + index.toString() }
          |  val iReduceRightIndexedOrNull = ints.reduceRightIndexedOrNull { index, item, acc -> acc + item + index }
          |  val sFold = strings.fold("seed") { acc, item -> acc + item }
          |  val iFold = ints.fold(0) { acc, item -> acc + item }
          |  val sFoldIndexed = strings.foldIndexed("seed") { index, acc, item -> acc + item + index.toString() }
          |  val iFoldIndexed = ints.foldIndexed(0) { index, acc, item -> acc + item + index }
          |  val sSorted = strings.sorted()
          |  val iSorted = ints.sorted()
          |  val sSortedDescending = strings.sortedDescending()
          |  val iSortedDescending = ints.sortedDescending()
          |  val sSortedBy = strings.sortedBy { it.length }
          |  val iSortedBy = ints.sortedBy { it }
          |  val sSortedByDescending = strings.sortedByDescending { it.length }
          |  val iSortedByDescending = ints.sortedByDescending { it }
          |  val sDistinct = strings.distinct()
          |  val iDistinct = ints.distinct()
          |  val sDistinctBy = strings.distinctBy { it.length }
          |  val iDistinctBy = ints.distinctBy { it }
          |  val sReversed = strings.reversed()
          |  val iReversed = ints.reversed()
          |  val sJoin = strings.joinToString()
          |  val iJoin = ints.joinToString()
          |}
          |""".stripMargin) { cpg =>
        val List(arrayAggregations) = cpg.method.nameExact("arrayAggregations").l: @unchecked

        List(
          ("ints.sum()", "sum", "kotlin.collections.sum:int(int[])", "int(int[])", "int"),
          ("ints.average()", "average", "kotlin.collections.average:double(int[])", "double(int[])", "double"),
          ("ints.minOrNull()", "minOrNull", "kotlin.collections.minOrNull:int(int[])", "int(int[])", "int"),
          ("ints.maxOrNull()", "maxOrNull", "kotlin.collections.maxOrNull:int(int[])", "int(int[])", "int"),
          (
            "strings.minOrNull()",
            "minOrNull",
            "kotlin.collections.minOrNull:java.lang.Comparable(java.lang.Comparable[])",
            "java.lang.Comparable(java.lang.Comparable[])",
            "java.lang.String"
          ),
          (
            "strings.maxOrNull()",
            "maxOrNull",
            "kotlin.collections.maxOrNull:java.lang.Comparable(java.lang.Comparable[])",
            "java.lang.Comparable(java.lang.Comparable[])",
            "java.lang.String"
          ),
          (
            "strings.maxByOrNull { it.length }",
            "maxByOrNull",
            "kotlin.collections.maxByOrNull:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "ints.maxByOrNull { it }",
            "maxByOrNull",
            "kotlin.collections.maxByOrNull:int(int[],kotlin.jvm.functions.Function1)",
            "int(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.minByOrNull { it.length }",
            "minByOrNull",
            "kotlin.collections.minByOrNull:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "ints.minByOrNull { it }",
            "minByOrNull",
            "kotlin.collections.minByOrNull:int(int[],kotlin.jvm.functions.Function1)",
            "int(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.maxOf { it.length }",
            "maxOf",
            "kotlin.collections.maxOf:java.lang.Comparable(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Comparable(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "ints.maxOf { it }",
            "maxOf",
            "kotlin.collections.maxOf:java.lang.Comparable(int[],kotlin.jvm.functions.Function1)",
            "java.lang.Comparable(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.maxOfOrNull { it.length }",
            "maxOfOrNull",
            "kotlin.collections.maxOfOrNull:java.lang.Comparable(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Comparable(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "ints.maxOfOrNull { it }",
            "maxOfOrNull",
            "kotlin.collections.maxOfOrNull:java.lang.Comparable(int[],kotlin.jvm.functions.Function1)",
            "java.lang.Comparable(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.sumOf { it.length }",
            "sumOf",
            "kotlin.collections.sumOf:int(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "ints.sumOf { it }",
            "sumOf",
            "kotlin.collections.sumOf:int(int[],kotlin.jvm.functions.Function1)",
            "int(int[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.reduce { acc, item -> acc + item }",
            "reduce",
            "kotlin.collections.reduce:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.String"
          ),
          (
            "ints.reduce { acc, item -> acc + item }",
            "reduce",
            "kotlin.collections.reduce:int(int[],kotlin.jvm.functions.Function2)",
            "int(int[],kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "strings.reduceOrNull { acc, item -> acc + item }",
            "reduceOrNull",
            "kotlin.collections.reduceOrNull:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.String"
          ),
          (
            "ints.reduceOrNull { acc, item -> acc + item }",
            "reduceOrNull",
            "kotlin.collections.reduceOrNull:int(int[],kotlin.jvm.functions.Function2)",
            "int(int[],kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "strings.reduceIndexed { index, acc, item -> acc + item + index.toString() }",
            "reduceIndexed",
            "kotlin.collections.reduceIndexed:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.String"
          ),
          (
            "ints.reduceIndexed { index, acc, item -> acc + item + index }",
            "reduceIndexed",
            "kotlin.collections.reduceIndexed:int(int[],kotlin.jvm.functions.Function3)",
            "int(int[],kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "strings.reduceIndexedOrNull { index, acc, item -> acc + item + index.toString() }",
            "reduceIndexedOrNull",
            "kotlin.collections.reduceIndexedOrNull:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.String"
          ),
          (
            "ints.reduceIndexedOrNull { index, acc, item -> acc + item + index }",
            "reduceIndexedOrNull",
            "kotlin.collections.reduceIndexedOrNull:int(int[],kotlin.jvm.functions.Function3)",
            "int(int[],kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "strings.reduceRight { item, acc -> acc + item }",
            "reduceRight",
            "kotlin.collections.reduceRight:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.String"
          ),
          (
            "ints.reduceRight { item, acc -> acc + item }",
            "reduceRight",
            "kotlin.collections.reduceRight:int(int[],kotlin.jvm.functions.Function2)",
            "int(int[],kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "strings.reduceRightOrNull { item, acc -> acc + item }",
            "reduceRightOrNull",
            "kotlin.collections.reduceRightOrNull:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function2)",
            "java.lang.String"
          ),
          (
            "ints.reduceRightOrNull { item, acc -> acc + item }",
            "reduceRightOrNull",
            "kotlin.collections.reduceRightOrNull:int(int[],kotlin.jvm.functions.Function2)",
            "int(int[],kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "strings.reduceRightIndexed { index, item, acc -> acc + item + index.toString() }",
            "reduceRightIndexed",
            "kotlin.collections.reduceRightIndexed:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.String"
          ),
          (
            "ints.reduceRightIndexed { index, item, acc -> acc + item + index }",
            "reduceRightIndexed",
            "kotlin.collections.reduceRightIndexed:int(int[],kotlin.jvm.functions.Function3)",
            "int(int[],kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "strings.reduceRightIndexedOrNull { index, item, acc -> acc + item + index.toString() }",
            "reduceRightIndexedOrNull",
            "kotlin.collections.reduceRightIndexedOrNull:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function3)",
            "java.lang.String"
          ),
          (
            "ints.reduceRightIndexedOrNull { index, item, acc -> acc + item + index }",
            "reduceRightIndexedOrNull",
            "kotlin.collections.reduceRightIndexedOrNull:int(int[],kotlin.jvm.functions.Function3)",
            "int(int[],kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            """strings.fold("seed") { acc, item -> acc + item }""",
            "fold",
            "kotlin.collections.fold:java.lang.Object(java.lang.Object[],java.lang.Object,kotlin.jvm.functions.Function2)",
            "java.lang.Object(java.lang.Object[],java.lang.Object,kotlin.jvm.functions.Function2)",
            "java.lang.String"
          ),
          (
            "ints.fold(0) { acc, item -> acc + item }",
            "fold",
            "kotlin.collections.fold:java.lang.Object(int[],java.lang.Object,kotlin.jvm.functions.Function2)",
            "java.lang.Object(int[],java.lang.Object,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            """strings.foldIndexed("seed") { index, acc, item -> acc + item + index.toString() }""",
            "foldIndexed",
            "kotlin.collections.foldIndexed:java.lang.Object(java.lang.Object[],java.lang.Object,kotlin.jvm.functions.Function3)",
            "java.lang.Object(java.lang.Object[],java.lang.Object,kotlin.jvm.functions.Function3)",
            "java.lang.String"
          ),
          (
            "ints.foldIndexed(0) { index, acc, item -> acc + item + index }",
            "foldIndexed",
            "kotlin.collections.foldIndexed:java.lang.Object(int[],java.lang.Object,kotlin.jvm.functions.Function3)",
            "java.lang.Object(int[],java.lang.Object,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "strings.sorted()",
            "sorted",
            "kotlin.collections.sorted:java.util.List(java.lang.Comparable[])",
            "java.util.List(java.lang.Comparable[])",
            "java.util.List"
          ),
          (
            "ints.sorted()",
            "sorted",
            "kotlin.collections.sorted:java.util.List(int[])",
            "java.util.List(int[])",
            "java.util.List"
          ),
          (
            "strings.sortedDescending()",
            "sortedDescending",
            "kotlin.collections.sortedDescending:java.util.List(java.lang.Comparable[])",
            "java.util.List(java.lang.Comparable[])",
            "java.util.List"
          ),
          (
            "ints.sortedDescending()",
            "sortedDescending",
            "kotlin.collections.sortedDescending:java.util.List(int[])",
            "java.util.List(int[])",
            "java.util.List"
          ),
          (
            "strings.sortedBy { it.length }",
            "sortedBy",
            "kotlin.collections.sortedBy:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.sortedBy { it }",
            "sortedBy",
            "kotlin.collections.sortedBy:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.sortedByDescending { it.length }",
            "sortedByDescending",
            "kotlin.collections.sortedByDescending:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.sortedByDescending { it }",
            "sortedByDescending",
            "kotlin.collections.sortedByDescending:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.distinct()",
            "distinct",
            "kotlin.collections.distinct:java.util.List(java.lang.Object[])",
            "java.util.List(java.lang.Object[])",
            "java.util.List"
          ),
          (
            "ints.distinct()",
            "distinct",
            "kotlin.collections.distinct:java.util.List(int[])",
            "java.util.List(int[])",
            "java.util.List"
          ),
          (
            "strings.distinctBy { it.length }",
            "distinctBy",
            "kotlin.collections.distinctBy:java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "ints.distinctBy { it }",
            "distinctBy",
            "kotlin.collections.distinctBy:java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List(int[],kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "strings.reversed()",
            "reversed",
            "kotlin.collections.reversed:java.util.List(java.lang.Object[])",
            "java.util.List(java.lang.Object[])",
            "java.util.List"
          ),
          (
            "ints.reversed()",
            "reversed",
            "kotlin.collections.reversed:java.util.List(int[])",
            "java.util.List(int[])",
            "java.util.List"
          ),
          (
            "strings.joinToString()",
            "joinToString",
            "kotlin.collections.joinToString:java.lang.String(java.lang.Object[],java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.String(java.lang.Object[],java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "ints.joinToString()",
            "joinToString",
            "kotlin.collections.joinToString:java.lang.String(int[],java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.String(int[],java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          )
        ).foreach { case (code, name, fullName, signature, typeFullName) =>
          val List(call) = arrayAggregations.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe fullName
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = arrayAggregations.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "iSum"                      -> "int",
          "iAverage"                  -> "double",
          "iMin"                      -> "int",
          "iMax"                      -> "int",
          "sMin"                      -> "java.lang.String",
          "sMax"                      -> "java.lang.String",
          "sMaxBy"                    -> "java.lang.String",
          "iMaxBy"                    -> "int",
          "sMinBy"                    -> "java.lang.String",
          "iMinBy"                    -> "int",
          "sMaxOf"                    -> "int",
          "iMaxOf"                    -> "int",
          "sMaxOfOrNull"              -> "int",
          "iMaxOfOrNull"              -> "int",
          "sSumOf"                    -> "int",
          "iSumOf"                    -> "int",
          "sReduce"                   -> "java.lang.String",
          "iReduce"                   -> "int",
          "sReduceOrNull"             -> "java.lang.String",
          "iReduceOrNull"             -> "int",
          "sReduceIndexed"            -> "java.lang.String",
          "iReduceIndexed"            -> "int",
          "sReduceIndexedOrNull"      -> "java.lang.String",
          "iReduceIndexedOrNull"      -> "int",
          "sReduceRight"              -> "java.lang.String",
          "iReduceRight"              -> "int",
          "sReduceRightOrNull"        -> "java.lang.String",
          "iReduceRightOrNull"        -> "int",
          "sReduceRightIndexed"       -> "java.lang.String",
          "iReduceRightIndexed"       -> "int",
          "sReduceRightIndexedOrNull" -> "java.lang.String",
          "iReduceRightIndexedOrNull" -> "int",
          "sFold"                     -> "java.lang.String",
          "iFold"                     -> "int",
          "sFoldIndexed"              -> "java.lang.String",
          "iFoldIndexed"              -> "int",
          "sSorted"                   -> "java.util.List",
          "iSorted"                   -> "java.util.List",
          "sSortedDescending"         -> "java.util.List",
          "iSortedDescending"         -> "java.util.List",
          "sSortedBy"                 -> "java.util.List",
          "iSortedBy"                 -> "java.util.List",
          "sSortedByDescending"       -> "java.util.List",
          "iSortedByDescending"       -> "java.util.List",
          "sDistinct"                 -> "java.util.List",
          "iDistinct"                 -> "java.util.List",
          "sDistinctBy"               -> "java.util.List",
          "iDistinctBy"               -> "java.util.List",
          "sReversed"                 -> "java.util.List",
          "iReversed"                 -> "java.util.List",
          "sJoin"                     -> "java.lang.String",
          "iJoin"                     -> "java.lang.String"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve array destination and pairing extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayDestinations(strings: Array<String>, ints: IntArray, pairs: Array<Pair<String, Int>>, nested: Array<Array<String>>) {
          |  val sZipList = strings.zip(ints.toList())
          |  val iZipList = ints.zip(strings.toList())
          |  val sZipTransform = strings.zip(ints.toList()) { text, number -> text + number.toString() }
          |  val iZipTransform = ints.zip(strings.toList()) { number, text -> text + number.toString() }
          |  val unzipped = pairs.unzip()
          |  val partitioned = strings.partition { it.isNotEmpty() }
          |  val iPartitioned = ints.partition { it > 0 }
          |  val grouped = strings.groupBy { it.length }
          |  val iGrouped = ints.groupBy { it }
          |  val groupedValue = strings.groupBy({ it.length }, { it })
          |  val iGroupedValue = ints.groupBy({ it }, { it.toString() })
          |  val sGrouping = strings.groupingBy { it.length }
          |  val iGrouping = ints.groupingBy { it }
          |  val associated = strings.associate { it to it.length }
          |  val iAssociated = ints.associate { it to it.toString() }
          |  val associatedBy = strings.associateBy { it.length }
          |  val iAssociatedBy = ints.associateBy { it }
          |  val associatedByValue = strings.associateBy({ it.length }, { it })
          |  val iAssociatedByValue = ints.associateBy({ it }, { it.toString() })
          |  val associatedWith = strings.associateWith { it.length }
          |  val iAssociatedWith = ints.associateWith { it.toString() }
          |  val flattened = nested.flatten()
          |  val pairMap = pairs.toMap()
          |  val pairMapTo = pairs.toMap(mutableMapOf<String, Int>())
          |  val sFilterTo = strings.filterTo(mutableListOf<String>()) { it.isNotEmpty() }
          |  val iFilterTo = ints.filterTo(mutableListOf<Int>()) { it > 0 }
          |  val sFilterNotTo = strings.filterNotTo(mutableListOf<String>()) { it.isEmpty() }
          |  val iFilterNotTo = ints.filterNotTo(mutableListOf<Int>()) { it == 0 }
          |  val sFilterIndexedTo = strings.filterIndexedTo(mutableListOf<String>()) { index, item -> index > 0 && item.isNotEmpty() }
          |  val iFilterIndexedTo = ints.filterIndexedTo(mutableListOf<Int>()) { index, item -> index > 0 && item > 0 }
          |  val sFilterNotNullTo = strings.filterNotNullTo(mutableListOf<String>())
          |  val sMapTo = strings.mapTo(mutableListOf<Int>()) { it.length }
          |  val iMapTo = ints.mapTo(mutableListOf<String>()) { it.toString() }
          |  val sMapNotNullTo = strings.mapNotNullTo(mutableListOf<Int>()) { it.length }
          |  val iMapNotNullTo = ints.mapNotNullTo(mutableListOf<String>()) { it.toString() }
          |  val sMapIndexedTo = strings.mapIndexedTo(mutableListOf<String>()) { index, item -> item + index.toString() }
          |  val iMapIndexedTo = ints.mapIndexedTo(mutableListOf<Int>()) { index, item -> item + index }
          |  val sMapIndexedNotNullTo = strings.mapIndexedNotNullTo(mutableListOf<String>()) { index, item -> item + index.toString() }
          |  val iMapIndexedNotNullTo = ints.mapIndexedNotNullTo(mutableListOf<Int>()) { index, item -> item + index }
          |  val sFlatMapTo = strings.flatMapTo(mutableListOf<String>()) { listOf(it, it.uppercase()) }
          |  val iFlatMapTo = ints.flatMapTo(mutableListOf<String>()) { listOf(it.toString()) }
          |  val sFlatMapIndexedTo = strings.flatMapIndexedTo(mutableListOf<String>()) { index, item -> listOf(item + index.toString()) }
          |  val iFlatMapIndexedTo = ints.flatMapIndexedTo(mutableListOf<Int>()) { index, item -> listOf(item + index) }
          |  val sAssociateTo = strings.associateTo(mutableMapOf<String, Int>()) { it to it.length }
          |  val iAssociateTo = ints.associateTo(mutableMapOf<Int, String>()) { it to it.toString() }
          |  val sAssociateByTo = strings.associateByTo(mutableMapOf<Int, String>()) { it.length }
          |  val iAssociateByTo = ints.associateByTo(mutableMapOf<Int, Int>()) { it }
          |  val sAssociateByValueTo = strings.associateByTo(mutableMapOf<Int, String>(), { it.length }, { it })
          |  val iAssociateByValueTo = ints.associateByTo(mutableMapOf<Int, String>(), { it }, { it.toString() })
          |  val sAssociateWithTo = strings.associateWithTo(mutableMapOf<String, Int>()) { it.length }
          |  val iAssociateWithTo = ints.associateWithTo(mutableMapOf<Int, String>()) { it.toString() }
          |  val sGroupByTo = strings.groupByTo(mutableMapOf<Int, MutableList<String>>()) { it.length }
          |  val iGroupByTo = ints.groupByTo(mutableMapOf<Int, MutableList<Int>>()) { it }
          |  val sGroupByValueTo = strings.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it.length }, { it })
          |  val iGroupByValueTo = ints.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it }, { it.toString() })
          |}
          |""".stripMargin) { cpg =>
        val List(arrayDestinations) = cpg.method.nameExact("arrayDestinations").l: @unchecked

        val objectArray = "java.lang.Object[]"
        val intArray    = "int[]"
        val collectionFunction1Signature =
          (receiver: String) => s"java.util.Collection($receiver,java.util.Collection,kotlin.jvm.functions.Function1)"
        val collectionFunction2Signature =
          (receiver: String) => s"java.util.Collection($receiver,java.util.Collection,kotlin.jvm.functions.Function2)"
        val mapFunction1Signature =
          (receiver: String) => s"java.util.Map($receiver,java.util.Map,kotlin.jvm.functions.Function1)"
        val mapFunction2Signature = (receiver: String) =>
          s"java.util.Map($receiver,java.util.Map,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)"

        List(
          ("strings.zip(ints.toList())", "zip", s"java.util.List($objectArray,java.lang.Iterable)", "java.util.List"),
          ("ints.zip(strings.toList())", "zip", s"java.util.List($intArray,java.lang.Iterable)", "java.util.List"),
          (
            "strings.zip(ints.toList()) { text, number -> text + number.toString() }",
            "zip",
            s"java.util.List($objectArray,java.lang.Iterable,kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "ints.zip(strings.toList()) { number, text -> text + number.toString() }",
            "zip",
            s"java.util.List($intArray,java.lang.Iterable,kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          ("pairs.unzip()", "unzip", "kotlin.Pair(kotlin.Pair[])", "kotlin.Pair"),
          (
            "strings.partition { it.isNotEmpty() }",
            "partition",
            s"kotlin.Pair($objectArray,kotlin.jvm.functions.Function1)",
            "kotlin.Pair"
          ),
          (
            "ints.partition { it > 0 }",
            "partition",
            s"kotlin.Pair($intArray,kotlin.jvm.functions.Function1)",
            "kotlin.Pair"
          ),
          (
            "strings.groupBy { it.length }",
            "groupBy",
            s"java.util.Map($objectArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "ints.groupBy { it }",
            "groupBy",
            s"java.util.Map($intArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "strings.groupBy({ it.length }, { it })",
            "groupBy",
            s"java.util.Map($objectArray,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "ints.groupBy({ it }, { it.toString() })",
            "groupBy",
            s"java.util.Map($intArray,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "strings.groupingBy { it.length }",
            "groupingBy",
            s"kotlin.collections.Grouping($objectArray,kotlin.jvm.functions.Function1)",
            "kotlin.collections.Grouping"
          ),
          (
            "ints.groupingBy { it }",
            "groupingBy",
            s"kotlin.collections.Grouping($intArray,kotlin.jvm.functions.Function1)",
            "kotlin.collections.Grouping"
          ),
          (
            "strings.associate { it to it.length }",
            "associate",
            s"java.util.Map($objectArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "ints.associate { it to it.toString() }",
            "associate",
            s"java.util.Map($intArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "strings.associateBy { it.length }",
            "associateBy",
            s"java.util.Map($objectArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "ints.associateBy { it }",
            "associateBy",
            s"java.util.Map($intArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "strings.associateBy({ it.length }, { it })",
            "associateBy",
            s"java.util.Map($objectArray,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "ints.associateBy({ it }, { it.toString() })",
            "associateBy",
            s"java.util.Map($intArray,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "strings.associateWith { it.length }",
            "associateWith",
            s"java.util.Map($objectArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "ints.associateWith { it.toString() }",
            "associateWith",
            s"java.util.Map($intArray,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          ("nested.flatten()", "flatten", "java.util.List(java.lang.Object[][])", "java.util.List"),
          ("pairs.toMap()", "toMap", "java.util.Map(kotlin.Pair[])", "java.util.Map"),
          (
            "pairs.toMap(mutableMapOf<String, Int>())",
            "toMap",
            "java.util.Map(kotlin.Pair[],java.util.Map)",
            "java.util.Map"
          ),
          (
            "strings.filterTo(mutableListOf<String>()) { it.isNotEmpty() }",
            "filterTo",
            collectionFunction1Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.filterTo(mutableListOf<Int>()) { it > 0 }",
            "filterTo",
            collectionFunction1Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.filterNotTo(mutableListOf<String>()) { it.isEmpty() }",
            "filterNotTo",
            collectionFunction1Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.filterNotTo(mutableListOf<Int>()) { it == 0 }",
            "filterNotTo",
            collectionFunction1Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.filterIndexedTo(mutableListOf<String>()) { index, item -> index > 0 && item.isNotEmpty() }",
            "filterIndexedTo",
            collectionFunction2Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.filterIndexedTo(mutableListOf<Int>()) { index, item -> index > 0 && item > 0 }",
            "filterIndexedTo",
            collectionFunction2Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.filterNotNullTo(mutableListOf<String>())",
            "filterNotNullTo",
            s"java.util.Collection($objectArray,java.util.Collection)",
            "java.util.List"
          ),
          (
            "strings.mapTo(mutableListOf<Int>()) { it.length }",
            "mapTo",
            collectionFunction1Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.mapTo(mutableListOf<String>()) { it.toString() }",
            "mapTo",
            collectionFunction1Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.mapNotNullTo(mutableListOf<Int>()) { it.length }",
            "mapNotNullTo",
            collectionFunction1Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.mapNotNullTo(mutableListOf<String>()) { it.toString() }",
            "mapNotNullTo",
            collectionFunction1Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.mapIndexedTo(mutableListOf<String>()) { index, item -> item + index.toString() }",
            "mapIndexedTo",
            collectionFunction2Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.mapIndexedTo(mutableListOf<Int>()) { index, item -> item + index }",
            "mapIndexedTo",
            collectionFunction2Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.mapIndexedNotNullTo(mutableListOf<String>()) { index, item -> item + index.toString() }",
            "mapIndexedNotNullTo",
            collectionFunction2Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.mapIndexedNotNullTo(mutableListOf<Int>()) { index, item -> item + index }",
            "mapIndexedNotNullTo",
            collectionFunction2Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.flatMapTo(mutableListOf<String>()) { listOf(it, it.uppercase()) }",
            "flatMapTo",
            collectionFunction1Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.flatMapTo(mutableListOf<String>()) { listOf(it.toString()) }",
            "flatMapTo",
            collectionFunction1Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.flatMapIndexedTo(mutableListOf<String>()) { index, item -> listOf(item + index.toString()) }",
            "flatMapIndexedTo",
            collectionFunction2Signature(objectArray),
            "java.util.List"
          ),
          (
            "ints.flatMapIndexedTo(mutableListOf<Int>()) { index, item -> listOf(item + index) }",
            "flatMapIndexedTo",
            collectionFunction2Signature(intArray),
            "java.util.List"
          ),
          (
            "strings.associateTo(mutableMapOf<String, Int>()) { it to it.length }",
            "associateTo",
            mapFunction1Signature(objectArray),
            "java.util.Map"
          ),
          (
            "ints.associateTo(mutableMapOf<Int, String>()) { it to it.toString() }",
            "associateTo",
            mapFunction1Signature(intArray),
            "java.util.Map"
          ),
          (
            "strings.associateByTo(mutableMapOf<Int, String>()) { it.length }",
            "associateByTo",
            mapFunction1Signature(objectArray),
            "java.util.Map"
          ),
          (
            "ints.associateByTo(mutableMapOf<Int, Int>()) { it }",
            "associateByTo",
            mapFunction1Signature(intArray),
            "java.util.Map"
          ),
          (
            "strings.associateByTo(mutableMapOf<Int, String>(), { it.length }, { it })",
            "associateByTo",
            mapFunction2Signature(objectArray),
            "java.util.Map"
          ),
          (
            "ints.associateByTo(mutableMapOf<Int, String>(), { it }, { it.toString() })",
            "associateByTo",
            mapFunction2Signature(intArray),
            "java.util.Map"
          ),
          (
            "strings.associateWithTo(mutableMapOf<String, Int>()) { it.length }",
            "associateWithTo",
            mapFunction1Signature(objectArray),
            "java.util.Map"
          ),
          (
            "ints.associateWithTo(mutableMapOf<Int, String>()) { it.toString() }",
            "associateWithTo",
            mapFunction1Signature(intArray),
            "java.util.Map"
          ),
          (
            "strings.groupByTo(mutableMapOf<Int, MutableList<String>>()) { it.length }",
            "groupByTo",
            mapFunction1Signature(objectArray),
            "java.util.Map"
          ),
          (
            "ints.groupByTo(mutableMapOf<Int, MutableList<Int>>()) { it }",
            "groupByTo",
            mapFunction1Signature(intArray),
            "java.util.Map"
          ),
          (
            "strings.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it.length }, { it })",
            "groupByTo",
            mapFunction2Signature(objectArray),
            "java.util.Map"
          ),
          (
            "ints.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it }, { it.toString() })",
            "groupByTo",
            mapFunction2Signature(intArray),
            "java.util.Map"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = arrayDestinations.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          val expectedFullName =
            if (signature.startsWith(Defines.UnresolvedSignature)) s"${Defines.UnresolvedNamespace}.$name:$signature"
            else s"kotlin.collections.$name:$signature"
          call.methodFullName shouldBe expectedFullName
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = arrayDestinations.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "sZipList"             -> "java.util.List",
          "iZipList"             -> "java.util.List",
          "sZipTransform"        -> "java.util.List",
          "iZipTransform"        -> "java.util.List",
          "unzipped"             -> "kotlin.Pair",
          "partitioned"          -> "kotlin.Pair",
          "iPartitioned"         -> "kotlin.Pair",
          "grouped"              -> "java.util.Map",
          "iGrouped"             -> "java.util.Map",
          "groupedValue"         -> "java.util.Map",
          "iGroupedValue"        -> "java.util.Map",
          "sGrouping"            -> "kotlin.collections.Grouping",
          "iGrouping"            -> "kotlin.collections.Grouping",
          "associated"           -> "java.util.Map",
          "iAssociated"          -> "java.util.Map",
          "associatedBy"         -> "java.util.Map",
          "iAssociatedBy"        -> "java.util.Map",
          "associatedByValue"    -> "java.util.Map",
          "iAssociatedByValue"   -> "java.util.Map",
          "associatedWith"       -> "java.util.Map",
          "iAssociatedWith"      -> "java.util.Map",
          "flattened"            -> "java.util.List",
          "pairMap"              -> "java.util.Map",
          "pairMapTo"            -> "java.util.Map",
          "sFilterTo"            -> "java.util.List",
          "iFilterTo"            -> "java.util.List",
          "sFilterNotTo"         -> "java.util.List",
          "iFilterNotTo"         -> "java.util.List",
          "sFilterIndexedTo"     -> "java.util.List",
          "iFilterIndexedTo"     -> "java.util.List",
          "sFilterNotNullTo"     -> "java.util.List",
          "sMapTo"               -> "java.util.List",
          "iMapTo"               -> "java.util.List",
          "sMapNotNullTo"        -> "java.util.List",
          "iMapNotNullTo"        -> "java.util.List",
          "sMapIndexedTo"        -> "java.util.List",
          "iMapIndexedTo"        -> "java.util.List",
          "sMapIndexedNotNullTo" -> "java.util.List",
          "iMapIndexedNotNullTo" -> "java.util.List",
          "sFlatMapTo"           -> "java.util.List",
          "iFlatMapTo"           -> "java.util.List",
          "sFlatMapIndexedTo"    -> "java.util.List",
          "iFlatMapIndexedTo"    -> "java.util.List",
          "sAssociateTo"         -> "java.util.Map",
          "iAssociateTo"         -> "java.util.Map",
          "sAssociateByTo"       -> "java.util.Map",
          "iAssociateByTo"       -> "java.util.Map",
          "sAssociateByValueTo"  -> "java.util.Map",
          "iAssociateByValueTo"  -> "java.util.Map",
          "sAssociateWithTo"     -> "java.util.Map",
          "iAssociateWithTo"     -> "java.util.Map",
          "sGroupByTo"           -> "java.util.Map",
          "iGroupByTo"           -> "java.util.Map",
          "sGroupByValueTo"      -> "java.util.Map",
          "iGroupByValueTo"      -> "java.util.Map"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve array utility extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun arrayUtilities(
          |  strings: Array<String>,
          |  moreStrings: Array<String>,
          |  ints: IntArray,
          |  moreInts: IntArray,
          |  nested: Array<Array<String>>,
          |  moreNested: Array<Array<String>>
          |) {
          |  val sCopy = strings.copyOf()
          |  val iCopy = ints.copyOf()
          |  val sCopySize = strings.copyOf(4)
          |  val iCopySize = ints.copyOf(4)
          |  val sCopyRange = strings.copyOfRange(0, 1)
          |  val iCopyRange = ints.copyOfRange(0, 1)
          |  val sSliceRange = strings.sliceArray(0..1)
          |  val iSliceRange = ints.sliceArray(0..1)
          |  val sSliceIndices = strings.sliceArray(listOf(0, 1))
          |  val iSliceIndices = ints.sliceArray(listOf(0, 1))
          |  val sPlusElement = strings.plus("x")
          |  val iPlusElement = ints.plus(1)
          |  val sPlusArray = strings.plus(moreStrings)
          |  val iPlusArray = ints.plus(moreInts)
          |  val sContentEquals = strings.contentEquals(moreStrings)
          |  val iContentEquals = ints.contentEquals(moreInts)
          |  val nContentDeepEquals = nested.contentDeepEquals(moreNested)
          |  val sContentHash = strings.contentHashCode()
          |  val iContentHash = ints.contentHashCode()
          |  val nContentDeepHash = nested.contentDeepHashCode()
          |  val sContentString = strings.contentToString()
          |  val iContentString = ints.contentToString()
          |  val nContentDeepString = nested.contentDeepToString()
          |  val sSortedArray = strings.sortedArray()
          |  val iSortedArray = ints.sortedArray()
          |  val sSortedArrayDescending = strings.sortedArrayDescending()
          |  val iSortedArrayDescending = ints.sortedArrayDescending()
          |  val sSortedArrayWith = strings.sortedArrayWith(compareBy { it.length })
          |  strings.sort()
          |  ints.sort()
          |  strings.sortDescending()
          |  ints.sortDescending()
          |  strings.sortWith(compareBy { it.length })
          |  strings.fill("x")
          |  ints.fill(1)
          |  strings.reverse()
          |  ints.reverse()
          |}
          |""".stripMargin) { cpg =>
        val List(arrayUtilities) = cpg.method.nameExact("arrayUtilities").l: @unchecked
        val objectArray          = "java.lang.Object[]"
        val comparableArray      = "java.lang.Comparable[]"

        List(
          ("strings.copyOf()", "copyOf", s"$objectArray($objectArray)", "java.lang.String[]"),
          ("ints.copyOf()", "copyOf", "int[](int[])", "int[]"),
          ("strings.copyOf(4)", "copyOf", s"$objectArray($objectArray,int)", "java.lang.String[]"),
          ("ints.copyOf(4)", "copyOf", "int[](int[],int)", "int[]"),
          ("strings.copyOfRange(0, 1)", "copyOfRange", s"$objectArray($objectArray,int,int)", "java.lang.String[]"),
          ("ints.copyOfRange(0, 1)", "copyOfRange", "int[](int[],int,int)", "int[]"),
          (
            "strings.sliceArray(0..1)",
            "sliceArray",
            s"$objectArray($objectArray,kotlin.ranges.IntRange)",
            "java.lang.String[]"
          ),
          ("ints.sliceArray(0..1)", "sliceArray", "int[](int[],kotlin.ranges.IntRange)", "int[]"),
          (
            "strings.sliceArray(listOf(0, 1))",
            "sliceArray",
            s"$objectArray($objectArray,java.util.Collection)",
            "java.lang.String[]"
          ),
          ("ints.sliceArray(listOf(0, 1))", "sliceArray", "int[](int[],java.util.Collection)", "int[]"),
          ("strings.plus(\"x\")", "plus", s"$objectArray($objectArray,java.lang.Object)", "java.lang.String[]"),
          ("ints.plus(1)", "plus", "int[](int[],int)", "int[]"),
          ("strings.plus(moreStrings)", "plus", s"$objectArray($objectArray,$objectArray)", "java.lang.String[]"),
          ("ints.plus(moreInts)", "plus", "int[](int[],int[])", "int[]"),
          ("strings.contentEquals(moreStrings)", "contentEquals", s"boolean($objectArray,$objectArray)", "boolean"),
          ("ints.contentEquals(moreInts)", "contentEquals", "boolean(int[],int[])", "boolean"),
          (
            "nested.contentDeepEquals(moreNested)",
            "contentDeepEquals",
            s"boolean($objectArray,$objectArray)",
            "boolean"
          ),
          ("strings.contentHashCode()", "contentHashCode", s"int($objectArray)", "int"),
          ("ints.contentHashCode()", "contentHashCode", "int(int[])", "int"),
          ("nested.contentDeepHashCode()", "contentDeepHashCode", s"int($objectArray)", "int"),
          ("strings.contentToString()", "contentToString", s"java.lang.String($objectArray)", "java.lang.String"),
          ("ints.contentToString()", "contentToString", "java.lang.String(int[])", "java.lang.String"),
          (
            "nested.contentDeepToString()",
            "contentDeepToString",
            s"java.lang.String($objectArray)",
            "java.lang.String"
          ),
          ("strings.sortedArray()", "sortedArray", s"$comparableArray($comparableArray)", "java.lang.String[]"),
          ("ints.sortedArray()", "sortedArray", "int[](int[])", "int[]"),
          (
            "strings.sortedArrayDescending()",
            "sortedArrayDescending",
            s"$comparableArray($comparableArray)",
            "java.lang.String[]"
          ),
          ("ints.sortedArrayDescending()", "sortedArrayDescending", "int[](int[])", "int[]"),
          (
            "strings.sortedArrayWith(compareBy { it.length })",
            "sortedArrayWith",
            s"$objectArray($objectArray,java.util.Comparator)",
            "java.lang.String[]"
          ),
          ("strings.sort()", "sort", s"void($comparableArray)", "void"),
          ("ints.sort()", "sort", "void(int[])", "void"),
          ("strings.sortDescending()", "sortDescending", s"void($comparableArray)", "void"),
          ("ints.sortDescending()", "sortDescending", "void(int[])", "void"),
          ("strings.sortWith(compareBy { it.length })", "sortWith", s"void($objectArray,java.util.Comparator)", "void"),
          ("strings.fill(\"x\")", "fill", s"void($objectArray,java.lang.Object,int,int)", "void"),
          ("ints.fill(1)", "fill", "void(int[],int,int,int)", "void"),
          ("strings.reverse()", "reverse", s"void($objectArray)", "void"),
          ("ints.reverse()", "reverse", "void(int[])", "void")
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = arrayUtilities.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = arrayUtilities.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "sCopy"                  -> "java.lang.String[]",
          "iCopy"                  -> "int[]",
          "sCopySize"              -> "java.lang.String[]",
          "iCopySize"              -> "int[]",
          "sCopyRange"             -> "java.lang.String[]",
          "iCopyRange"             -> "int[]",
          "sSliceRange"            -> "java.lang.String[]",
          "iSliceRange"            -> "int[]",
          "sSliceIndices"          -> "java.lang.String[]",
          "iSliceIndices"          -> "int[]",
          "sPlusElement"           -> "java.lang.String[]",
          "iPlusElement"           -> "int[]",
          "sPlusArray"             -> "java.lang.String[]",
          "iPlusArray"             -> "int[]",
          "sContentEquals"         -> "boolean",
          "iContentEquals"         -> "boolean",
          "nContentDeepEquals"     -> "boolean",
          "sContentHash"           -> "int",
          "iContentHash"           -> "int",
          "nContentDeepHash"       -> "int",
          "sContentString"         -> "java.lang.String",
          "iContentString"         -> "java.lang.String",
          "nContentDeepString"     -> "java.lang.String",
          "sSortedArray"           -> "java.lang.String[]",
          "iSortedArray"           -> "int[]",
          "sSortedArrayDescending" -> "java.lang.String[]",
          "iSortedArrayDescending" -> "int[]",
          "sSortedArrayWith"       -> "java.lang.String[]"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve collection array sequence defaulting and random extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun defaults(
          |  values: List<String>,
          |  nullableValues: List<String>?,
          |  set: Set<String>,
          |  nullableSet: Set<String>?,
          |  map: Map<String, Int>,
          |  nullableMap: Map<String, Int>?,
          |  strings: Array<String>,
          |  nullableStrings: Array<String>?,
          |  ints: IntArray,
          |  nullableInts: IntArray?,
          |  seq: Sequence<String>
          |) {
          |  val valuesIfEmpty = values.ifEmpty { listOf("fallback") }
          |  val setIfEmpty = set.ifEmpty { setOf("fallback") }
          |  val mapIfEmpty = map.ifEmpty { mapOf("fallback" to 1) }
          |  val stringsIfEmpty = strings.ifEmpty { arrayOf("fallback") }
          |  val intsIfEmpty = ints.ifEmpty { intArrayOf(1) }
          |  val seqIfEmpty = seq.ifEmpty { sequenceOf("fallback") }
          |  val valuesOrEmpty = nullableValues.orEmpty()
          |  val setOrEmpty = nullableSet.orEmpty()
          |  val mapOrEmpty = nullableMap.orEmpty()
          |  val stringsOrEmpty = nullableStrings.orEmpty()
          |  val intsOrEmpty = nullableInts.orEmpty()
          |  val valueRandom = values.random()
          |  val valueRandomOrNull = values.randomOrNull()
          |  val stringRandom = strings.random()
          |  val stringRandomOrNull = strings.randomOrNull()
          |  val intRandom = ints.random()
          |  val intRandomOrNull = ints.randomOrNull()
          |  val seqRandom = seq.random()
          |  val seqRandomOrNull = seq.randomOrNull()
          |}
          |""".stripMargin) { cpg =>
        val List(defaults) = cpg.method.nameExact("defaults").l: @unchecked
        val objectArray    = "java.lang.Object[]"

        List(
          (
            """values.ifEmpty { listOf("fallback") }""",
            "ifEmpty",
            "java.lang.Object(java.util.Collection&java.lang.Object,kotlin.jvm.functions.Function0)",
            "java.util.List"
          ),
          (
            """set.ifEmpty { setOf("fallback") }""",
            "ifEmpty",
            "java.lang.Object(java.util.Collection&java.lang.Object,kotlin.jvm.functions.Function0)",
            "java.util.Set"
          ),
          (
            """map.ifEmpty { mapOf("fallback" to 1) }""",
            "ifEmpty",
            "java.lang.Object(java.util.Map&java.lang.Object,kotlin.jvm.functions.Function0)",
            "java.util.Map"
          ),
          (
            """strings.ifEmpty { arrayOf("fallback") }""",
            "ifEmpty",
            s"java.lang.Object($objectArray&java.lang.Object,kotlin.jvm.functions.Function0)",
            "java.lang.String[]"
          ),
          ("""ints.ifEmpty { intArrayOf(1) }""", "ifEmpty", "int[](int[],kotlin.jvm.functions.Function0)", "int[]"),
          (
            """seq.ifEmpty { sequenceOf("fallback") }""",
            "ifEmpty",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.jvm.functions.Function0)",
            "kotlin.sequences.Sequence"
          ),
          ("nullableValues.orEmpty()", "orEmpty", "java.util.List(java.util.List)", "java.util.List"),
          ("nullableSet.orEmpty()", "orEmpty", "java.util.Set(java.util.Set)", "java.util.Set"),
          ("nullableMap.orEmpty()", "orEmpty", "java.util.Map(java.util.Map)", "java.util.Map"),
          ("nullableStrings.orEmpty()", "orEmpty", s"$objectArray($objectArray)", "java.lang.String[]"),
          ("nullableInts.orEmpty()", "orEmpty", "int[](int[])", "int[]"),
          ("values.random()", "random", "java.lang.Object(java.util.Collection)", "java.lang.String"),
          ("values.randomOrNull()", "randomOrNull", "java.lang.Object(java.util.Collection)", "java.lang.String"),
          ("strings.random()", "random", s"java.lang.Object($objectArray)", "java.lang.String"),
          ("strings.randomOrNull()", "randomOrNull", s"java.lang.Object($objectArray)", "java.lang.String"),
          ("ints.random()", "random", "int(int[])", "int"),
          ("ints.randomOrNull()", "randomOrNull", "int(int[])", "int")
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = defaults.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          val namespace  = if (signature.startsWith("kotlin.sequences.")) "kotlin.sequences" else "kotlin.collections"
          call.methodFullName shouldBe s"$namespace.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List(
          ("seq.random()", "random", s"${Defines.UnresolvedSignature}(0)"),
          ("seq.randomOrNull()", "randomOrNull", s"${Defines.UnresolvedSignature}(0)")
        ).foreach { case (code, name, signature) =>
          val List(call) = defaults.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"${Defines.UnresolvedNamespace}.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe TypeConstants.Any
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = defaults.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "valuesIfEmpty"      -> "java.util.List",
          "setIfEmpty"         -> "java.util.Set",
          "mapIfEmpty"         -> "java.util.Map",
          "stringsIfEmpty"     -> "java.lang.String[]",
          "intsIfEmpty"        -> "int[]",
          "seqIfEmpty"         -> "kotlin.sequences.Sequence",
          "valuesOrEmpty"      -> "java.util.List",
          "setOrEmpty"         -> "java.util.Set",
          "mapOrEmpty"         -> "java.util.Map",
          "stringsOrEmpty"     -> "java.lang.String[]",
          "intsOrEmpty"        -> "int[]",
          "valueRandom"        -> "java.lang.String",
          "valueRandomOrNull"  -> "java.lang.String",
          "stringRandom"       -> "java.lang.String",
          "stringRandomOrNull" -> "java.lang.String",
          "intRandom"          -> "int",
          "intRandomOrNull"    -> "int",
          "seqRandom"          -> TypeConstants.Any,
          "seqRandomOrNull"    -> TypeConstants.Any
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve range expressions and range helper calls" in {
      withOxidizedCpg("""package demo
          |
          |fun ranges(total: Int) {
          |  val closed = 0..10
          |  val untilRange = 0 until 10
          |  val down = 10 downTo 0
          |  val steppedCall = (0..10).step(2)
          |  val steppedInfix = 0..10 step 2
          |  val charRange = 'a'..'z'
          |  val longRange = 1L..3L
          |  val longUntil = 1L until 4L
          |  val longDown = 4L downTo 1L
          |  val longStep = (1L..4L).step(2L)
          |  val charDown = 'z' downTo 'a'
          |  val charStep = ('a'..'z').step(2)
          |  for (i in 0 until total) {
          |    println(i)
          |  }
          |  println(closed)
          |  println(untilRange)
          |  println(down)
          |  println(steppedCall)
          |  println(steppedInfix)
          |  println(charRange)
          |  println(longRange)
          |  println(longUntil)
          |  println(longDown)
          |  println(longStep)
          |  println(charDown)
          |  println(charStep)
          |}
          |""".stripMargin) { cpg =>
        val List(ranges) = cpg.method.nameExact("ranges").l: @unchecked

        val localTypes = ranges.ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "closed"       -> "kotlin.ranges.IntRange",
          "untilRange"   -> "kotlin.ranges.IntRange",
          "down"         -> "kotlin.ranges.IntProgression",
          "steppedCall"  -> "kotlin.ranges.IntProgression",
          "steppedInfix" -> "kotlin.ranges.IntProgression",
          "charRange"    -> "kotlin.ranges.CharRange",
          "longRange"    -> "kotlin.ranges.LongRange",
          "longUntil"    -> "kotlin.ranges.LongRange",
          "longDown"     -> "kotlin.ranges.LongProgression",
          "longStep"     -> "kotlin.ranges.LongProgression",
          "charDown"     -> "kotlin.ranges.CharProgression",
          "charStep"     -> "kotlin.ranges.CharProgression",
          "i"            -> "int"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }

        ranges.ast.isCall.nameExact(Operators.range).codeExact("0..10").typeFullName.distinct.l shouldBe
          List("kotlin.ranges.IntRange")
        ranges.ast.isCall.nameExact(Operators.range).codeExact("'a'..'z'").typeFullName.distinct.l shouldBe
          List("kotlin.ranges.CharRange")
        ranges.ast.isCall.nameExact(Operators.range).codeExact("1L..3L").typeFullName.l shouldBe
          List("kotlin.ranges.LongRange")

        List(
          ("0 until 10", "until", "kotlin.ranges.until", "kotlin.ranges.IntRange(int,int)", "kotlin.ranges.IntRange"),
          (
            "10 downTo 0",
            "downTo",
            "kotlin.ranges.downTo",
            "kotlin.ranges.IntProgression(int,int)",
            "kotlin.ranges.IntProgression"
          ),
          (
            "(0..10).step(2)",
            "step",
            "kotlin.ranges.step",
            "kotlin.ranges.IntProgression(kotlin.ranges.IntProgression,int)",
            "kotlin.ranges.IntProgression"
          ),
          (
            "0..10 step 2",
            "step",
            "kotlin.ranges.step",
            "kotlin.ranges.IntProgression(kotlin.ranges.IntProgression,int)",
            "kotlin.ranges.IntProgression"
          ),
          (
            "1L until 4L",
            "until",
            "kotlin.ranges.until",
            "kotlin.ranges.LongRange(long,long)",
            "kotlin.ranges.LongRange"
          ),
          (
            "4L downTo 1L",
            "downTo",
            "kotlin.ranges.downTo",
            "kotlin.ranges.LongProgression(long,long)",
            "kotlin.ranges.LongProgression"
          ),
          (
            "(1L..4L).step(2L)",
            "step",
            "kotlin.ranges.step",
            "kotlin.ranges.LongProgression(kotlin.ranges.LongProgression,long)",
            "kotlin.ranges.LongProgression"
          ),
          (
            "'z' downTo 'a'",
            "downTo",
            "kotlin.ranges.downTo",
            "kotlin.ranges.CharProgression(char,char)",
            "kotlin.ranges.CharProgression"
          ),
          (
            "('a'..'z').step(2)",
            "step",
            "kotlin.ranges.step",
            "kotlin.ranges.CharProgression(kotlin.ranges.CharProgression,int)",
            "kotlin.ranges.CharProgression"
          )
        ).foreach { case (code, name, fullNameBase, signature, typeFullName) =>
          val List(call) = ranges.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val List(iteratorCall) =
          ranges.ast.isCall.nameExact("iterator").codeExact("0 until total.iterator()").l: @unchecked
        iteratorCall.methodFullName shouldBe "kotlin.ranges.IntRange.iterator:java.util.Iterator()"
      }
    }

    "resolve collection operator calls" in {
      withOxidizedCpg("""package demo
          |
          |fun collectionOperators(values: List<String>, setValues: Set<String>, mutableValues: MutableList<String>, mutableSet: MutableSet<String>) {
          |  val listPlusElement = values + "x"
          |  val listPlusList = values + listOf("y")
          |  val listPlusNamed = values.plus("n")
          |  val listPlusNamedList = values.plus(listOf("n"))
          |  val listMinusElement = values - "x"
          |  val listMinusList = values - listOf("y")
          |  val listMinusNamed = values.minus("n")
          |  val listMinusNamedList = values.minus(listOf("n"))
          |  val setPlusElement = setValues + "x"
          |  val setPlusList = setValues + listOf("y")
          |  val setPlusNamed = setValues.plus("n")
          |  val setPlusNamedList = setValues.plus(listOf("n"))
          |  val setMinusElement = setValues - "x"
          |  val setMinusList = setValues - listOf("y")
          |  val setMinusNamed = setValues.minus("n")
          |  val setMinusNamedList = setValues.minus(listOf("n"))
          |  mutableValues += "add"
          |  mutableValues -= "remove"
          |  mutableSet += "add"
          |  mutableSet -= "remove"
          |  println(listPlusElement)
          |  println(listPlusList)
          |  println(listPlusNamed)
          |  println(listPlusNamedList)
          |  println(listMinusElement)
          |  println(listMinusList)
          |  println(listMinusNamed)
          |  println(listMinusNamedList)
          |  println(setPlusElement)
          |  println(setPlusList)
          |  println(setPlusNamed)
          |  println(setPlusNamedList)
          |  println(setMinusElement)
          |  println(setMinusList)
          |  println(setMinusNamed)
          |  println(setMinusNamedList)
          |  println(mutableValues)
          |  println(mutableSet)
          |}
          |""".stripMargin) { cpg =>
        List("values + \"x\"", """values + listOf("y")""").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.addition).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.addition
          call.typeFullName shouldBe "java.util.List"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List("values - \"x\"", """values - listOf("y")""").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.subtraction).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.subtraction
          call.typeFullName shouldBe "java.util.List"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List("setValues + \"x\"", """setValues + listOf("y")""").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.addition).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.addition
          call.typeFullName shouldBe "java.util.Set"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List("setValues - \"x\"", """setValues - listOf("y")""").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.subtraction).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.subtraction
          call.typeFullName shouldBe "java.util.Set"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List(
          (
            """values.plus("n")""",
            "plus",
            "kotlin.collections.plus",
            "java.util.List(java.util.Collection,java.lang.Object)",
            "java.util.List"
          ),
          (
            """values.plus(listOf("n"))""",
            "plus",
            "kotlin.collections.plus",
            "java.util.List(java.util.Collection,java.lang.Iterable)",
            "java.util.List"
          ),
          (
            """values.minus("n")""",
            "minus",
            "kotlin.collections.minus",
            "java.util.List(java.lang.Iterable,java.lang.Object)",
            "java.util.List"
          ),
          (
            """values.minus(listOf("n"))""",
            "minus",
            "kotlin.collections.minus",
            "java.util.List(java.lang.Iterable,java.lang.Iterable)",
            "java.util.List"
          ),
          (
            """setValues.plus("n")""",
            "plus",
            "kotlin.collections.plus",
            "java.util.Set(java.util.Set,java.lang.Object)",
            "java.util.Set"
          ),
          (
            """setValues.plus(listOf("n"))""",
            "plus",
            "kotlin.collections.plus",
            "java.util.Set(java.util.Set,java.lang.Iterable)",
            "java.util.Set"
          ),
          (
            """setValues.minus("n")""",
            "minus",
            "kotlin.collections.minus",
            "java.util.Set(java.util.Set,java.lang.Object)",
            "java.util.Set"
          ),
          (
            """setValues.minus(listOf("n"))""",
            "minus",
            "kotlin.collections.minus",
            "java.util.Set(java.util.Set,java.lang.Iterable)",
            "java.util.Set"
          )
        ).foreach { case (code, name, fullNameBase, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List(
          (Operators.assignmentPlus, "mutableValues += \"add\""),
          (Operators.assignmentPlus, "mutableSet += \"add\""),
          (Operators.assignmentMinus, "mutableValues -= \"remove\""),
          (Operators.assignmentMinus, "mutableSet -= \"remove\"")
        ).foreach { case (name, code) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe name
          call.typeFullName shouldBe "void"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "listPlusElement",
            "listPlusList",
            "listPlusNamed",
            "listPlusNamedList",
            "listMinusElement",
            "listMinusList",
            "listMinusNamed",
            "listMinusNamedList"
          )
          .typeFullName
          .l shouldBe List.fill(8)("java.util.List")
        cpg.local
          .nameExact(
            "setPlusElement",
            "setPlusList",
            "setPlusNamed",
            "setPlusNamedList",
            "setMinusElement",
            "setMinusList",
            "setMinusNamed",
            "setMinusNamedList"
          )
          .typeFullName
          .l shouldBe List.fill(8)("java.util.Set")
      }
    }

    "resolve collection member extension and iterator calls" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun process(msg: String): Int {
          |    val values = listOf("a", msg)
          |    val hasMsg = values.contains(msg)
          |    val mapped = values.map { item -> item }
          |    val filtered = values.filter { item -> item != "" }
          |    values.forEach { item -> println(item) }
          |    val iterator = values.iterator()
          |    val hasAny = iterator.hasNext()
          |    val nextValue = iterator.next()
          |    return 0
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(process) = cpg.method.fullNameExact("demo.Foo.process:int(java.lang.String)").l: @unchecked

        val List(containsCall) = process.ast.isCall.nameExact("contains").l: @unchecked
        containsCall.methodFullName shouldBe "kotlin.collections.List.contains:boolean(java.lang.Object)"
        containsCall.signature shouldBe "boolean(java.lang.Object)"
        containsCall.typeFullName shouldBe "boolean"
        containsCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

        val List(mapCall) = process.ast.isCall.nameExact("map").l: @unchecked
        mapCall.methodFullName shouldBe "kotlin.collections.map:java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        mapCall.signature shouldBe "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        mapCall.typeFullName shouldBe "java.util.List"
        mapCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(filterCall) = process.ast.isCall.nameExact("filter").l: @unchecked
        filterCall.methodFullName shouldBe "kotlin.collections.filter:java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        filterCall.signature shouldBe "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        filterCall.typeFullName shouldBe "java.util.List"
        filterCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(forEachCall) = process.ast.isCall.nameExact("forEach").l: @unchecked
        forEachCall.methodFullName shouldBe "kotlin.collections.forEach:void(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        forEachCall.signature shouldBe "void(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        forEachCall.typeFullName shouldBe "void"
        forEachCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(iteratorCall) = process.ast.isCall.nameExact("iterator").l: @unchecked
        iteratorCall.methodFullName shouldBe "java.util.List.iterator:java.util.Iterator()"
        iteratorCall.signature shouldBe "java.util.Iterator()"
        iteratorCall.typeFullName shouldBe "java.util.Iterator"
        iteratorCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

        val List(hasNextCall) = process.ast.isCall.nameExact("hasNext").l: @unchecked
        hasNextCall.methodFullName shouldBe "kotlin.collections.Iterator.hasNext:boolean()"
        hasNextCall.signature shouldBe "boolean()"
        hasNextCall.typeFullName shouldBe "boolean"
        hasNextCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

        val List(nextCall) = process.ast.isCall.nameExact("next").l: @unchecked
        nextCall.methodFullName shouldBe "kotlin.collections.Iterator.next:java.lang.Object()"
        nextCall.signature shouldBe "java.lang.Object()"
        nextCall.typeFullName shouldBe "java.lang.Object"
        nextCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

        process.ast.isLocal.nameExact("hasMsg").typeFullName.l shouldBe List("boolean")
        process.ast.isLocal.nameExact("mapped").typeFullName.l shouldBe List("java.util.List")
        process.ast.isLocal.nameExact("filtered").typeFullName.l shouldBe List("java.util.List")
        process.ast.isLocal.nameExact("iterator").typeFullName.l shouldBe List("java.util.Iterator")
        process.ast.isLocal.nameExact("hasAny").typeFullName.l shouldBe List("boolean")
        process.ast.isLocal.nameExact("nextValue").typeFullName.l shouldBe List("java.lang.Object")
      }
    }

    "resolve collection and text callback extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun callbacks(text: String, values: List<String>, nullableValues: List<String?>) {
          |  val textEach = text.onEach { println(it) }
          |  val valuesEach = values.onEach { println(it) }
          |  val filteredNot = values.filterNot { it.isEmpty() }
          |  val filteredNotNull = nullableValues.filterNotNull()
          |  val mappedNotNull = values.mapNotNull { it.takeIf { item -> item.isNotEmpty() } }
          |  val filteredIndexed = values.filterIndexed { index, item -> index > 0 && item.isNotEmpty() }
          |  val flatMappedIndexed = values.flatMapIndexed { index, item -> listOf(item + index.toString()) }
          |  val mappedIndexed = values.mapIndexed { index, item -> item + index.toString() }
          |  val mappedIndexedNotNull = values.mapIndexedNotNull { index, item -> item.takeIf { index >= 0 } }
          |  val valuesIndexedEach = values.onEachIndexed { index, item -> println(item + index.toString()) }
          |  values.forEachIndexed { index, item -> println(item + index.toString()) }
          |  println(textEach)
          |  println(valuesEach)
          |  println(filteredNot)
          |  println(filteredNotNull)
          |  println(mappedNotNull)
          |  println(filteredIndexed)
          |  println(flatMappedIndexed)
          |  println(mappedIndexed)
          |  println(mappedIndexedNotNull)
          |  println(valuesIndexedEach)
          |}
          |""".stripMargin) { cpg =>
        val List(textOnEachCall) = cpg.call.nameExact("onEach").code("text\\.onEach.*").l: @unchecked
        textOnEachCall.methodFullName shouldBe
          "kotlin.text.onEach:java.lang.CharSequence(java.lang.CharSequence,kotlin.jvm.functions.Function1)"
        textOnEachCall.signature shouldBe "java.lang.CharSequence(java.lang.CharSequence,kotlin.jvm.functions.Function1)"
        textOnEachCall.typeFullName shouldBe "java.lang.String"
        textOnEachCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(valuesOnEachCall) = cpg.call.nameExact("onEach").code("values\\.onEach.*").l: @unchecked
        valuesOnEachCall.methodFullName shouldBe
          "kotlin.collections.onEach:java.lang.Iterable(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        valuesOnEachCall.signature shouldBe "java.lang.Iterable(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        valuesOnEachCall.typeFullName shouldBe "java.util.List"
        valuesOnEachCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(valuesOnEachIndexedCall) = cpg.call.nameExact("onEachIndexed").l: @unchecked
        valuesOnEachIndexedCall.methodFullName shouldBe
          "kotlin.collections.onEachIndexed:java.lang.Iterable(java.lang.Iterable,kotlin.jvm.functions.Function2)"
        valuesOnEachIndexedCall.signature shouldBe "java.lang.Iterable(java.lang.Iterable,kotlin.jvm.functions.Function2)"
        valuesOnEachIndexedCall.typeFullName shouldBe "java.util.List"
        valuesOnEachIndexedCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        List("filterNot", "mapNotNull").foreach { name =>
          val List(call) = cpg.call.nameExact(name).l: @unchecked
          val signature  = "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.util.List"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val List(filterNotNullCall) = cpg.call.nameExact("filterNotNull").l: @unchecked
        filterNotNullCall.methodFullName shouldBe "kotlin.collections.filterNotNull:java.util.List(java.lang.Iterable)"
        filterNotNullCall.signature shouldBe "java.util.List(java.lang.Iterable)"
        filterNotNullCall.typeFullName shouldBe "java.util.List"
        filterNotNullCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        List("filterIndexed", "flatMapIndexed", "mapIndexed", "mapIndexedNotNull").foreach { name =>
          val List(call) = cpg.call.nameExact(name).l: @unchecked
          val signature  = "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function2)"
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.util.List"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val List(forEachIndexedCall) = cpg.call.nameExact("forEachIndexed").l: @unchecked
        forEachIndexedCall.methodFullName shouldBe
          "kotlin.collections.forEachIndexed:void(java.lang.Iterable,kotlin.jvm.functions.Function2)"
        forEachIndexedCall.signature shouldBe "void(java.lang.Iterable,kotlin.jvm.functions.Function2)"
        forEachIndexedCall.typeFullName shouldBe "void"
        forEachIndexedCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        cpg.local.nameExact("textEach").typeFullName.l shouldBe List("java.lang.String")
        cpg.local
          .nameExact(
            "valuesEach",
            "filteredNot",
            "filteredNotNull",
            "mappedNotNull",
            "filteredIndexed",
            "flatMappedIndexed",
            "mappedIndexed",
            "mappedIndexedNotNull",
            "valuesIndexedEach"
          )
          .typeFullName
          .l shouldBe List.fill(9)("java.util.List")
      }
    }

    "resolve collection terminal and element extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun terminals(values: List<String>) {
          |  val anyPlain = values.any()
          |  val anyMatch = values.any { it.isNotEmpty() }
          |  val allMatch = values.all { it.isNotEmpty() }
          |  val nonePlain = values.none()
          |  val noneMatch = values.none { it.isEmpty() }
          |  val countPlain = values.count()
          |  val countMatch = values.count { it.isNotEmpty() }
          |  val firstPlain = values.first()
          |  val firstMatch = values.first { it.isNotEmpty() }
          |  val firstOrNullPlain = values.firstOrNull()
          |  val firstOrNullMatch = values.firstOrNull { it.isNotEmpty() }
          |  val lastPlain = values.last()
          |  val lastMatch = values.last { it.isNotEmpty() }
          |  val lastOrNullPlain = values.lastOrNull()
          |  val lastOrNullMatch = values.lastOrNull { it.isNotEmpty() }
          |  val singlePlain = values.single()
          |  val singleMatch = values.single { it.isNotEmpty() }
          |  val singleOrNullPlain = values.singleOrNull()
          |  val singleOrNullMatch = values.singleOrNull { it.isNotEmpty() }
          |  val element = values.elementAt(1)
          |  val elementOrNull = values.elementAtOrNull(1)
          |  val elementOrElse = values.elementAtOrElse(1) { "fallback" }
          |  val found = values.find { it.isNotEmpty() }
          |  val foundLast = values.findLast { it.isNotEmpty() }
          |  val indexFirst = values.indexOfFirst { it.isNotEmpty() }
          |  val indexLast = values.indexOfLast { it.isNotEmpty() }
          |  println(anyPlain)
          |  println(anyMatch)
          |  println(allMatch)
          |  println(nonePlain)
          |  println(noneMatch)
          |  println(countPlain)
          |  println(countMatch)
          |  println(firstPlain)
          |  println(firstMatch)
          |  println(firstOrNullPlain)
          |  println(firstOrNullMatch)
          |  println(lastPlain)
          |  println(lastMatch)
          |  println(lastOrNullPlain)
          |  println(lastOrNullMatch)
          |  println(singlePlain)
          |  println(singleMatch)
          |  println(singleOrNullPlain)
          |  println(singleOrNullMatch)
          |  println(element)
          |  println(elementOrNull)
          |  println(elementOrElse)
          |  println(found)
          |  println(foundLast)
          |  println(indexFirst)
          |  println(indexLast)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("values.any()", "any", "boolean(java.lang.Iterable)", "boolean"),
          (
            "values.any { it.isNotEmpty() }",
            "any",
            "boolean(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          (
            "values.all { it.isNotEmpty() }",
            "all",
            "boolean(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          ("values.none()", "none", "boolean(java.lang.Iterable)", "boolean"),
          (
            "values.none { it.isEmpty() }",
            "none",
            "boolean(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          ("values.count()", "count", "int(java.util.Collection)", "int"),
          (
            "values.count { it.isNotEmpty() }",
            "count",
            "int(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "int"
          ),
          ("values.first()", "first", "java.lang.Object(java.util.List)", "java.lang.String"),
          (
            "values.first { it.isNotEmpty() }",
            "first",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("values.firstOrNull()", "firstOrNull", "java.lang.Object(java.util.List)", "java.lang.String"),
          (
            "values.firstOrNull { it.isNotEmpty() }",
            "firstOrNull",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("values.last()", "last", "java.lang.Object(java.util.List)", "java.lang.String"),
          (
            "values.last { it.isNotEmpty() }",
            "last",
            "java.lang.Object(java.util.List,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("values.lastOrNull()", "lastOrNull", "java.lang.Object(java.util.List)", "java.lang.String"),
          (
            "values.lastOrNull { it.isNotEmpty() }",
            "lastOrNull",
            "java.lang.Object(java.util.List,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("values.single()", "single", "java.lang.Object(java.util.List)", "java.lang.String"),
          (
            "values.single { it.isNotEmpty() }",
            "single",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("values.singleOrNull()", "singleOrNull", "java.lang.Object(java.util.List)", "java.lang.String"),
          (
            "values.singleOrNull { it.isNotEmpty() }",
            "singleOrNull",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("values.elementAt(1)", "elementAt", "java.lang.Object(java.util.List,int)", "java.lang.String"),
          ("values.elementAtOrNull(1)", "elementAtOrNull", "java.lang.Object(java.util.List,int)", "java.lang.String"),
          (
            """values.elementAtOrElse(1) { "fallback" }""",
            "elementAtOrElse",
            "java.lang.Object(java.util.List,int,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "values.find { it.isNotEmpty() }",
            "find",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "values.findLast { it.isNotEmpty() }",
            "findLast",
            "java.lang.Object(java.util.List,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "values.indexOfFirst { it.isNotEmpty() }",
            "indexOfFirst",
            "int(java.util.List,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.indexOfLast { it.isNotEmpty() }",
            "indexOfLast",
            "int(java.util.List,kotlin.jvm.functions.Function1)",
            "int"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact("anyPlain", "anyMatch", "allMatch", "nonePlain", "noneMatch")
          .typeFullName
          .l shouldBe List.fill(5)("boolean")
        cpg.local.nameExact("countPlain", "countMatch", "indexFirst", "indexLast").typeFullName.l shouldBe
          List.fill(4)("int")
        cpg.local
          .nameExact(
            "firstPlain",
            "firstMatch",
            "firstOrNullPlain",
            "firstOrNullMatch",
            "lastPlain",
            "lastMatch",
            "lastOrNullPlain",
            "lastOrNullMatch",
            "singlePlain",
            "singleMatch",
            "singleOrNullPlain",
            "singleOrNullMatch",
            "element",
            "elementOrNull",
            "elementOrElse",
            "found",
            "foundLast"
          )
          .typeFullName
          .l shouldBe List.fill(17)("java.lang.String")
      }
    }

    "resolve collection slicing and ordering extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun slicing(values: List<String>) {
          |  val taken = values.take(2)
          |  val takenLast = values.takeLast(2)
          |  val takenWhile = values.takeWhile { it.isNotEmpty() }
          |  val takenLastWhile = values.takeLastWhile { it.isNotEmpty() }
          |  val dropped = values.drop(1)
          |  val droppedLast = values.dropLast(1)
          |  val droppedWhile = values.dropWhile { it.isEmpty() }
          |  val droppedLastWhile = values.dropLastWhile { it.isEmpty() }
          |  val reversed = values.reversed()
          |  val asReversed = values.asReversed()
          |  val sorted = values.sorted()
          |  val sortedDescending = values.sortedDescending()
          |  val sortedBy = values.sortedBy { it.length }
          |  val sortedByDescending = values.sortedByDescending { it.length }
          |  val distinct = values.distinct()
          |  val distinctBy = values.distinctBy { it.length }
          |  val shuffled = values.shuffled()
          |  println(taken)
          |  println(takenLast)
          |  println(takenWhile)
          |  println(takenLastWhile)
          |  println(dropped)
          |  println(droppedLast)
          |  println(droppedWhile)
          |  println(droppedLastWhile)
          |  println(reversed)
          |  println(asReversed)
          |  println(sorted)
          |  println(sortedDescending)
          |  println(sortedBy)
          |  println(sortedByDescending)
          |  println(distinct)
          |  println(distinctBy)
          |  println(shuffled)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("values.take(2)", "take", "java.util.List(java.lang.Iterable,int)"),
          ("values.takeLast(2)", "takeLast", "java.util.List(java.util.List,int)"),
          (
            "values.takeWhile { it.isNotEmpty() }",
            "takeWhile",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.takeLastWhile { it.isNotEmpty() }",
            "takeLastWhile",
            "java.util.List(java.util.List,kotlin.jvm.functions.Function1)"
          ),
          ("values.drop(1)", "drop", "java.util.List(java.lang.Iterable,int)"),
          ("values.dropLast(1)", "dropLast", "java.util.List(java.util.List,int)"),
          (
            "values.dropWhile { it.isEmpty() }",
            "dropWhile",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.dropLastWhile { it.isEmpty() }",
            "dropLastWhile",
            "java.util.List(java.util.List,kotlin.jvm.functions.Function1)"
          ),
          ("values.reversed()", "reversed", "java.util.List(java.lang.Iterable)"),
          ("values.asReversed()", "asReversed", "java.util.List(java.util.List)"),
          ("values.sorted()", "sorted", "java.util.List(java.lang.Iterable)"),
          ("values.sortedDescending()", "sortedDescending", "java.util.List(java.lang.Iterable)"),
          (
            "values.sortedBy { it.length }",
            "sortedBy",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.sortedByDescending { it.length }",
            "sortedByDescending",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
          ),
          ("values.distinct()", "distinct", "java.util.List(java.lang.Iterable)"),
          (
            "values.distinctBy { it.length }",
            "distinctBy",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)"
          ),
          ("values.shuffled()", "shuffled", "java.util.List(java.lang.Iterable)")
        ).foreach { case (code, name, signature) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.util.List"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "taken",
            "takenLast",
            "takenWhile",
            "takenLastWhile",
            "dropped",
            "droppedLast",
            "droppedWhile",
            "droppedLastWhile",
            "reversed",
            "asReversed",
            "sorted",
            "sortedDescending",
            "sortedBy",
            "sortedByDescending",
            "distinct",
            "distinctBy",
            "shuffled"
          )
          .typeFullName
          .l shouldBe List.fill(17)("java.util.List")
      }
    }

    "resolve collection conversion and view extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun conversions(
          |  values: List<String>,
          |  nullableValues: List<String?>,
          |  pairs: List<Pair<String, Int>>,
          |  comparator: java.util.Comparator<String>
          |) {
          |  val listCopy = values.toList()
          |  val mutableListCopy = values.toMutableList()
          |  val setCopy = values.toSet()
          |  val mutableSetCopy = values.toMutableSet()
          |  val hashSetCopy = values.toHashSet()
          |  val sortedSetCopy = values.toSortedSet()
          |  val sortedSetCopyWithComparator = values.toSortedSet(comparator)
          |  val collectionCopy = values.toCollection(mutableListOf<String>())
          |  val iterView = values.asIterable()
          |  val sequenceView = values.asSequence()
          |  val indexedView = values.withIndex()
          |  val noNulls = nullableValues.requireNoNulls()
          |  val mapCopy = pairs.toMap()
          |  val mutableMapCopy = pairs.toMap(mutableMapOf<String, Int>())
          |  println(listCopy)
          |  println(mutableListCopy)
          |  println(setCopy)
          |  println(mutableSetCopy)
          |  println(hashSetCopy)
          |  println(sortedSetCopy)
          |  println(sortedSetCopyWithComparator)
          |  println(collectionCopy)
          |  println(iterView)
          |  println(sequenceView)
          |  println(indexedView)
          |  println(noNulls)
          |  println(mapCopy)
          |  println(mutableMapCopy)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("values.toList()", "toList", "java.util.List(java.lang.Iterable)", "java.util.List"),
          ("values.toMutableList()", "toMutableList", "java.util.List(java.util.Collection)", "java.util.List"),
          ("values.toSet()", "toSet", "java.util.Set(java.lang.Iterable)", "java.util.Set"),
          ("values.toMutableSet()", "toMutableSet", "java.util.Set(java.lang.Iterable)", "java.util.Set"),
          ("values.toHashSet()", "toHashSet", "java.util.HashSet(java.lang.Iterable)", "java.util.HashSet"),
          ("values.toSortedSet()", "toSortedSet", "java.util.SortedSet(java.lang.Iterable)", "java.util.SortedSet"),
          (
            "values.toSortedSet(comparator)",
            "toSortedSet",
            "java.util.SortedSet(java.lang.Iterable,java.util.Comparator)",
            "java.util.SortedSet"
          ),
          (
            "values.toCollection(mutableListOf<String>())",
            "toCollection",
            "java.util.Collection(java.lang.Iterable,java.util.Collection)",
            "java.util.List"
          ),
          ("values.asIterable()", "asIterable", "java.lang.Iterable(java.lang.Iterable)", "java.lang.Iterable"),
          (
            "values.asSequence()",
            "asSequence",
            "kotlin.sequences.Sequence(java.lang.Iterable)",
            "kotlin.sequences.Sequence"
          ),
          ("values.withIndex()", "withIndex", "java.lang.Iterable(java.lang.Iterable)", "java.lang.Iterable"),
          ("nullableValues.requireNoNulls()", "requireNoNulls", "java.util.List(java.util.List)", "java.util.List"),
          ("pairs.toMap()", "toMap", "java.util.Map(java.lang.Iterable)", "java.util.Map"),
          (
            "pairs.toMap(mutableMapOf<String, Int>())",
            "toMap",
            "java.util.Map(java.lang.Iterable,java.util.Map)",
            "java.util.Map"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local.nameExact("listCopy", "mutableListCopy", "collectionCopy", "noNulls").typeFullName.l shouldBe
          List.fill(4)("java.util.List")
        cpg.local.nameExact("setCopy", "mutableSetCopy").typeFullName.l shouldBe List.fill(2)("java.util.Set")
        cpg.local.nameExact("hashSetCopy").typeFullName.l shouldBe List("java.util.HashSet")
        cpg.local.nameExact("sortedSetCopy", "sortedSetCopyWithComparator").typeFullName.l shouldBe
          List.fill(2)("java.util.SortedSet")
        cpg.local.nameExact("iterView", "indexedView").typeFullName.l shouldBe List.fill(2)("java.lang.Iterable")
        cpg.local.nameExact("sequenceView").typeFullName.l shouldBe List("kotlin.sequences.Sequence")
        cpg.local.nameExact("mapCopy", "mutableMapCopy").typeFullName.l shouldBe List.fill(2)("java.util.Map")
      }
    }

    "resolve map extension and view calls" in {
      withOxidizedCpg("""package demo
          |
          |fun mapOps(
          |    values: Map<String, Int>,
          |    stringValues: Map<String, String>,
          |    comparator: java.util.Comparator<String>,
          |    entryComparator: java.util.Comparator<Map.Entry<String, Int>>
          |) {
          |  val filtered = values.filter { entry -> entry.value > 0 }
          |  val filteredKeys = values.filterKeys { key -> key.isNotEmpty() }
          |  val filteredValues = values.filterValues { value -> value > 0 }
          |  val mapped = values.map { entry -> entry.key + entry.value.toString() }
          |  val mappedKeys = values.mapKeys { entry -> entry.key.uppercase() }
          |  val mappedValues = values.mapValues { entry -> entry.value + 1 }
          |  val anyPlain = values.any()
          |  val anyPredicate = values.any { entry -> entry.value > 0 }
          |  val allPredicate = values.all { entry -> entry.value > 0 }
          |  val nonePlain = values.none()
          |  val nonePredicate = values.none { entry -> entry.value > 0 }
          |  val countPlain = values.count()
          |  val countPredicate = values.count { entry -> entry.value > 0 }
          |  values.forEach { entry -> println(entry.key) }
          |  val onEachCopy = values.onEach { entry -> println(entry.key) }
          |  val flatMapped = values.flatMap { entry -> listOf(entry.key) }
          |  val flatMappedTo = values.flatMapTo(mutableListOf<String>()) { entry -> listOf(entry.key) }
          |  val mappedTo = values.mapTo(mutableListOf<String>()) { entry -> entry.key }
          |  val minByEntry = values.minBy { entry -> entry.value }
          |  val maxByEntry = values.maxBy { entry -> entry.value }
          |  val minByEntryOrNull = values.minByOrNull { entry -> entry.value }
          |  val maxByEntryOrNull = values.maxByOrNull { entry -> entry.value }
          |  val minWithEntry = values.minWith(entryComparator)
          |  val maxWithEntry = values.maxWith(entryComparator)
          |  val minWithEntryOrNull = values.minWithOrNull(entryComparator)
          |  val maxWithEntryOrNull = values.maxWithOrNull(entryComparator)
          |  val minValue = values.minOf { entry -> entry.value }
          |  val maxValue = values.maxOf { entry -> entry.value }
          |  val minValueOrNull = values.minOfOrNull { entry -> entry.value }
          |  val maxValueOrNull = values.maxOfOrNull { entry -> entry.value }
          |  val firstKey = values.firstNotNullOf { entry -> entry.key }
          |  val firstKeyOrNull = values.firstNotNullOfOrNull { entry -> entry.key }
          |  val intTotal = values.sumOf { entry -> entry.value }
          |  val longTotal = values.sumOf { entry -> entry.value.toLong() }
          |  val doubleTotal = values.sumOf { entry -> entry.value.toDouble() }
          |  val copied = values.toMap()
          |  val copiedTo = values.toMap(mutableMapOf<String, Int>())
          |  val mutableCopy = values.toMutableMap()
          |  val iterableView = values.asIterable()
          |  val sequenceView = values.asSequence()
          |  val listedCopy = values.toList()
          |  val propertiesCopy = stringValues.toProperties()
          |  val sortedCopy = values.toSortedMap()
          |  val sortedCopyWithComparator = values.toSortedMap(comparator)
          |  val entries = values.entries
          |  val keys = values.keys
          |  val vals = values.values
          |  println(filtered)
          |  println(filteredKeys)
          |  println(filteredValues)
          |  println(mapped)
          |  println(mappedKeys)
          |  println(mappedValues)
          |  println(anyPlain)
          |  println(anyPredicate)
          |  println(allPredicate)
          |  println(nonePlain)
          |  println(nonePredicate)
          |  println(countPlain)
          |  println(countPredicate)
          |  println(onEachCopy)
          |  println(flatMapped)
          |  println(flatMappedTo)
          |  println(mappedTo)
          |  println(minByEntry)
          |  println(maxByEntry)
          |  println(minByEntryOrNull)
          |  println(maxByEntryOrNull)
          |  println(minWithEntry)
          |  println(maxWithEntry)
          |  println(minWithEntryOrNull)
          |  println(maxWithEntryOrNull)
          |  println(minValue)
          |  println(maxValue)
          |  println(minValueOrNull)
          |  println(maxValueOrNull)
          |  println(firstKey)
          |  println(firstKeyOrNull)
          |  println(intTotal)
          |  println(longTotal)
          |  println(doubleTotal)
          |  println(copied)
          |  println(copiedTo)
          |  println(mutableCopy)
          |  println(iterableView)
          |  println(sequenceView)
          |  println(listedCopy)
          |  println(propertiesCopy)
          |  println(sortedCopy)
          |  println(sortedCopyWithComparator)
          |  println(entries)
          |  println(keys)
          |  println(vals)
          |}
          |""".stripMargin) { cpg =>
        List(
          (
            "values.filter { entry -> entry.value > 0 }",
            "filter",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.filterKeys { key -> key.isNotEmpty() }",
            "filterKeys",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.filterValues { value -> value > 0 }",
            "filterValues",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.map { entry -> entry.key + entry.value.toString() }",
            "map",
            "java.util.List(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "values.mapKeys { entry -> entry.key.uppercase() }",
            "mapKeys",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.mapValues { entry -> entry.value + 1 }",
            "mapValues",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          ("values.any()", "any", "boolean(java.util.Map)", "boolean"),
          (
            "values.any { entry -> entry.value > 0 }",
            "any",
            "boolean(java.util.Map,kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          (
            "values.all { entry -> entry.value > 0 }",
            "all",
            "boolean(java.util.Map,kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          ("values.none()", "none", "boolean(java.util.Map)", "boolean"),
          (
            "values.none { entry -> entry.value > 0 }",
            "none",
            "boolean(java.util.Map,kotlin.jvm.functions.Function1)",
            "boolean"
          ),
          ("values.count()", "count", "int(java.util.Map)", "int"),
          (
            "values.count { entry -> entry.value > 0 }",
            "count",
            "int(java.util.Map,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.forEach { entry -> println(entry.key) }",
            "forEach",
            "void(java.util.Map,kotlin.jvm.functions.Function1)",
            "void"
          ),
          (
            "values.onEach { entry -> println(entry.key) }",
            "onEach",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.flatMap { entry -> listOf(entry.key) }",
            "flatMap",
            "java.util.List(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "values.flatMapTo(mutableListOf<String>()) { entry -> listOf(entry.key) }",
            "flatMapTo",
            "java.util.Collection(java.util.Map,java.util.Collection,kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "values.mapTo(mutableListOf<String>()) { entry -> entry.key }",
            "mapTo",
            "java.util.Collection(java.util.Map,java.util.Collection,kotlin.jvm.functions.Function1)",
            "java.util.List"
          ),
          (
            "values.minBy { entry -> entry.value }",
            "minBy",
            "java.util.Map$Entry(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map$Entry"
          ),
          (
            "values.maxBy { entry -> entry.value }",
            "maxBy",
            "java.util.Map$Entry(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map$Entry"
          ),
          (
            "values.minByOrNull { entry -> entry.value }",
            "minByOrNull",
            "java.util.Map$Entry(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map$Entry"
          ),
          (
            "values.maxByOrNull { entry -> entry.value }",
            "maxByOrNull",
            "java.util.Map$Entry(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map$Entry"
          ),
          (
            "values.minWith(entryComparator)",
            "minWith",
            "java.util.Map$Entry(java.util.Map,java.util.Comparator)",
            "java.util.Map$Entry"
          ),
          (
            "values.maxWith(entryComparator)",
            "maxWith",
            "java.util.Map$Entry(java.util.Map,java.util.Comparator)",
            "java.util.Map$Entry"
          ),
          (
            "values.minWithOrNull(entryComparator)",
            "minWithOrNull",
            "java.util.Map$Entry(java.util.Map,java.util.Comparator)",
            "java.util.Map$Entry"
          ),
          (
            "values.maxWithOrNull(entryComparator)",
            "maxWithOrNull",
            "java.util.Map$Entry(java.util.Map,java.util.Comparator)",
            "java.util.Map$Entry"
          ),
          (
            "values.minOf { entry -> entry.value }",
            "minOf",
            "java.lang.Comparable(java.util.Map,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.maxOf { entry -> entry.value }",
            "maxOf",
            "java.lang.Comparable(java.util.Map,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.minOfOrNull { entry -> entry.value }",
            "minOfOrNull",
            "java.lang.Comparable(java.util.Map,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.maxOfOrNull { entry -> entry.value }",
            "maxOfOrNull",
            "java.lang.Comparable(java.util.Map,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.firstNotNullOf { entry -> entry.key }",
            "firstNotNullOf",
            "java.lang.Object(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "values.firstNotNullOfOrNull { entry -> entry.key }",
            "firstNotNullOfOrNull",
            "java.lang.Object(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "values.sumOf { entry -> entry.value }",
            "sumOf",
            "int(java.util.Map,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.sumOf { entry -> entry.value.toLong() }",
            "sumOf",
            "long(java.util.Map,kotlin.jvm.functions.Function1)",
            "long"
          ),
          (
            "values.sumOf { entry -> entry.value.toDouble() }",
            "sumOf",
            "double(java.util.Map,kotlin.jvm.functions.Function1)",
            "double"
          ),
          ("values.toMap()", "toMap", "java.util.Map(java.util.Map)", "java.util.Map"),
          (
            "values.toMap(mutableMapOf<String, Int>())",
            "toMap",
            "java.util.Map(java.util.Map,java.util.Map)",
            "java.util.Map"
          ),
          ("values.toMutableMap()", "toMutableMap", "java.util.Map(java.util.Map)", "java.util.Map"),
          ("values.asIterable()", "asIterable", "java.lang.Iterable(java.util.Map)", "java.lang.Iterable"),
          (
            "values.asSequence()",
            "asSequence",
            "kotlin.sequences.Sequence(java.util.Map)",
            "kotlin.sequences.Sequence"
          ),
          ("values.toList()", "toList", "java.util.List(java.util.Map)", "java.util.List"),
          (
            "stringValues.toProperties()",
            "toProperties",
            "java.util.Properties(java.util.Map)",
            "java.util.Properties"
          ),
          ("values.toSortedMap()", "toSortedMap", "java.util.SortedMap(java.util.Map)", "java.util.SortedMap"),
          (
            "values.toSortedMap(comparator)",
            "toSortedMap",
            "java.util.SortedMap(java.util.Map,java.util.Comparator)",
            "java.util.SortedMap"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local.nameExact("intTotal").typeFullName.l shouldBe List("int")
        cpg.local.nameExact("longTotal").typeFullName.l shouldBe List("long")
        cpg.local.nameExact("doubleTotal").typeFullName.l shouldBe List("double")

        List(
          ("values.entries", "entries", "java.util.Set"),
          ("values.keys", "keys", "java.util.Set"),
          ("values.values", "values", "java.util.Collection")
        ).foreach { case (code, fieldName, typeFullName) =>
          val List(fieldAccess) = cpg.call.nameExact(Operators.fieldAccess).codeExact(code).l: @unchecked
          fieldAccess.methodFullName shouldBe Operators.fieldAccess
          fieldAccess.typeFullName shouldBe typeFullName
          fieldAccess.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          fieldAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List(fieldName)
        }

        val localTypes = cpg.local
          .nameExact(
            "filtered",
            "filteredKeys",
            "filteredValues",
            "mapped",
            "mappedKeys",
            "mappedValues",
            "anyPlain",
            "anyPredicate",
            "allPredicate",
            "nonePlain",
            "nonePredicate",
            "countPlain",
            "countPredicate",
            "onEachCopy",
            "flatMapped",
            "flatMappedTo",
            "mappedTo",
            "minByEntry",
            "maxByEntry",
            "minByEntryOrNull",
            "maxByEntryOrNull",
            "minWithEntry",
            "maxWithEntry",
            "minWithEntryOrNull",
            "maxWithEntryOrNull",
            "minValue",
            "maxValue",
            "minValueOrNull",
            "maxValueOrNull",
            "firstKey",
            "firstKeyOrNull",
            "intTotal",
            "longTotal",
            "doubleTotal",
            "copied",
            "copiedTo",
            "mutableCopy",
            "iterableView",
            "sequenceView",
            "listedCopy",
            "propertiesCopy",
            "sortedCopy",
            "sortedCopyWithComparator",
            "entries",
            "keys",
            "vals"
          )
          .map(local => local.name -> local.typeFullName)
          .toMap

        localTypes should contain("filtered" -> "java.util.Map")
        localTypes should contain("filteredKeys" -> "java.util.Map")
        localTypes should contain("filteredValues" -> "java.util.Map")
        localTypes should contain("mapped" -> "java.util.List")
        localTypes should contain("mappedKeys" -> "java.util.Map")
        localTypes should contain("mappedValues" -> "java.util.Map")
        localTypes should contain("anyPlain" -> "boolean")
        localTypes should contain("anyPredicate" -> "boolean")
        localTypes should contain("allPredicate" -> "boolean")
        localTypes should contain("nonePlain" -> "boolean")
        localTypes should contain("nonePredicate" -> "boolean")
        localTypes should contain("countPlain" -> "int")
        localTypes should contain("countPredicate" -> "int")
        localTypes should contain("onEachCopy" -> "java.util.Map")
        localTypes should contain("flatMapped" -> "java.util.List")
        localTypes should contain("flatMappedTo" -> "java.util.List")
        localTypes should contain("mappedTo" -> "java.util.List")
        localTypes should contain("minByEntry" -> "java.util.Map$Entry")
        localTypes should contain("maxByEntry" -> "java.util.Map$Entry")
        localTypes should contain("minByEntryOrNull" -> "java.util.Map$Entry")
        localTypes should contain("maxByEntryOrNull" -> "java.util.Map$Entry")
        localTypes should contain("minWithEntry" -> "java.util.Map$Entry")
        localTypes should contain("maxWithEntry" -> "java.util.Map$Entry")
        localTypes should contain("minWithEntryOrNull" -> "java.util.Map$Entry")
        localTypes should contain("maxWithEntryOrNull" -> "java.util.Map$Entry")
        localTypes should contain("minValue" -> "int")
        localTypes should contain("maxValue" -> "int")
        localTypes should contain("minValueOrNull" -> "int")
        localTypes should contain("maxValueOrNull" -> "int")
        localTypes should contain("firstKey" -> "java.lang.String")
        localTypes should contain("firstKeyOrNull" -> "java.lang.String")
        localTypes should contain("intTotal" -> "int")
        localTypes should contain("longTotal" -> "long")
        localTypes should contain("doubleTotal" -> "double")
        localTypes should contain("copied" -> "java.util.Map")
        localTypes should contain("copiedTo" -> "java.util.Map")
        localTypes should contain("mutableCopy" -> "java.util.Map")
        localTypes should contain("iterableView" -> "java.lang.Iterable")
        localTypes should contain("sequenceView" -> "kotlin.sequences.Sequence")
        localTypes should contain("listedCopy" -> "java.util.List")
        localTypes should contain("propertiesCopy" -> "java.util.Properties")
        localTypes should contain("sortedCopy" -> "java.util.SortedMap")
        localTypes should contain("sortedCopyWithComparator" -> "java.util.SortedMap")
        localTypes should contain("entries" -> "java.util.Set")
        localTypes should contain("keys" -> "java.util.Set")
        localTypes should contain("vals" -> "java.util.Collection")
      }
    }

    "resolve map aggregation and running extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun mapAggregations(values: Map<String, Int>) {
          |  val joined = values.joinToString()
          |  val joinedSep = values.joinToString(separator = "|")
          |  val joinedTo = values.joinTo(StringBuilder())
          |  val folded = values.fold(0) { acc, entry -> acc + entry.value }
          |  val foldedIndexed = values.foldIndexed(0) { index, acc, entry -> acc + entry.value + index }
          |  val reduced = values.reduce { acc, entry -> acc }
          |  val reducedIndexed = values.reduceIndexed { index, acc, entry -> acc }
          |  val reducedOrNull = values.reduceOrNull { acc, entry -> acc }
          |  val reducedIndexedOrNull = values.reduceIndexedOrNull { index, acc, entry -> acc }
          |  val runningFold = values.runningFold(0) { acc, entry -> acc + entry.value }
          |  val runningFoldIndexed = values.runningFoldIndexed(0) { index, acc, entry -> acc + entry.value + index }
          |  val runningReduce = values.runningReduce { acc, entry -> acc }
          |  val runningReduceIndexed = values.runningReduceIndexed { index, acc, entry -> acc }
          |  val scan = values.scan(0) { acc, entry -> acc + entry.value }
          |  val scanIndexed = values.scanIndexed(0) { index, acc, entry -> acc + entry.value + index }
          |  println(joined)
          |  println(joinedSep)
          |  println(joinedTo)
          |  println(folded)
          |  println(foldedIndexed)
          |  println(reduced)
          |  println(reducedIndexed)
          |  println(reducedOrNull)
          |  println(reducedIndexedOrNull)
          |  println(runningFold)
          |  println(runningFoldIndexed)
          |  println(runningReduce)
          |  println(runningReduceIndexed)
          |  println(scan)
          |  println(scanIndexed)
          |}
          |""".stripMargin) { cpg =>
        val joinToStringSignature =
          "java.lang.String(java.util.Map,java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)"
        List(
          ("values.joinToString()", "joinToString", joinToStringSignature, "java.lang.String"),
          ("""values.joinToString(separator = "|")""", "joinToString", joinToStringSignature, "java.lang.String"),
          (
            "values.joinTo(StringBuilder())",
            "joinTo",
            "java.lang.Appendable(java.util.Map,java.lang.Appendable,java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.StringBuilder"
          ),
          (
            "values.fold(0) { acc, entry -> acc + entry.value }",
            "fold",
            "java.lang.Object(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.foldIndexed(0) { index, acc, entry -> acc + entry.value + index }",
            "foldIndexed",
            "java.lang.Object(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.reduce { acc, entry -> acc }",
            "reduce",
            "java.lang.Object(java.util.Map,kotlin.jvm.functions.Function2)",
            "java.util.Map$Entry"
          ),
          (
            "values.reduceIndexed { index, acc, entry -> acc }",
            "reduceIndexed",
            "java.lang.Object(java.util.Map,kotlin.jvm.functions.Function3)",
            "java.util.Map$Entry"
          ),
          (
            "values.reduceOrNull { acc, entry -> acc }",
            "reduceOrNull",
            "java.lang.Object(java.util.Map,kotlin.jvm.functions.Function2)",
            "java.util.Map$Entry"
          ),
          (
            "values.reduceIndexedOrNull { index, acc, entry -> acc }",
            "reduceIndexedOrNull",
            "java.lang.Object(java.util.Map,kotlin.jvm.functions.Function3)",
            "java.util.Map$Entry"
          ),
          (
            "values.runningFold(0) { acc, entry -> acc + entry.value }",
            "runningFold",
            "java.util.List(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "values.runningFoldIndexed(0) { index, acc, entry -> acc + entry.value + index }",
            "runningFoldIndexed",
            "java.util.List(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function3)",
            "java.util.List"
          ),
          (
            "values.runningReduce { acc, entry -> acc }",
            "runningReduce",
            "java.util.List(java.util.Map,kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "values.runningReduceIndexed { index, acc, entry -> acc }",
            "runningReduceIndexed",
            "java.util.List(java.util.Map,kotlin.jvm.functions.Function3)",
            "java.util.List"
          ),
          (
            "values.scan(0) { acc, entry -> acc + entry.value }",
            "scan",
            "java.util.List(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          (
            "values.scanIndexed(0) { index, acc, entry -> acc + entry.value + index }",
            "scanIndexed",
            "java.util.List(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function3)",
            "java.util.List"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = cpg.local
          .nameExact(
            "joined",
            "joinedSep",
            "joinedTo",
            "folded",
            "foldedIndexed",
            "reduced",
            "reducedIndexed",
            "reducedOrNull",
            "reducedIndexedOrNull",
            "runningFold",
            "runningFoldIndexed",
            "runningReduce",
            "runningReduceIndexed",
            "scan",
            "scanIndexed"
          )
          .map(local => local.name -> local.typeFullName)
          .toMap

        localTypes should contain("joined" -> "java.lang.String")
        localTypes should contain("joinedSep" -> "java.lang.String")
        localTypes should contain("joinedTo" -> "java.lang.StringBuilder")
        localTypes should contain("folded" -> "int")
        localTypes should contain("foldedIndexed" -> "int")
        localTypes should contain("reduced" -> "java.util.Map$Entry")
        localTypes should contain("reducedIndexed" -> "java.util.Map$Entry")
        localTypes should contain("reducedOrNull" -> "java.util.Map$Entry")
        localTypes should contain("reducedIndexedOrNull" -> "java.util.Map$Entry")
        localTypes should contain("runningFold" -> "java.util.List")
        localTypes should contain("runningFoldIndexed" -> "java.util.List")
        localTypes should contain("runningReduce" -> "java.util.List")
        localTypes should contain("runningReduceIndexed" -> "java.util.List")
        localTypes should contain("scan" -> "java.util.List")
        localTypes should contain("scanIndexed" -> "java.util.List")
      }
    }

    "resolve map conversion and grouping extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun mapViews(values: Map<String, Int>) {
          |  val indexed = values.withIndex()
          |  val setCopy = values.toSet()
          |  val hashSetCopy = values.toHashSet()
          |  val mutableSetCopy = values.toMutableSet()
          |  val filteredNotNull = values.filterNotNull()
          |  val noNulls = values.requireNoNulls()
          |  val grouped = values.groupBy { entry -> entry.value }
          |  val groupedValue = values.groupBy({ entry -> entry.value }, { entry -> entry.key })
          |  val grouping = values.groupingBy { entry -> entry.value }
          |  val associated = values.associate { entry -> entry.key to entry.value }
          |  val associatedBy = values.associateBy { entry -> entry.value }
          |  val associatedByValue = values.associateBy({ entry -> entry.value }, { entry -> entry.key })
          |  val associatedWith = values.associateWith { entry -> entry.value }
          |  println(indexed)
          |  println(setCopy)
          |  println(hashSetCopy)
          |  println(mutableSetCopy)
          |  println(filteredNotNull)
          |  println(noNulls)
          |  println(grouped)
          |  println(groupedValue)
          |  println(grouping)
          |  println(associated)
          |  println(associatedBy)
          |  println(associatedByValue)
          |  println(associatedWith)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("values.withIndex()", "withIndex", "java.lang.Iterable(java.util.Map)", "java.lang.Iterable"),
          ("values.toSet()", "toSet", "java.util.Set(java.util.Map)", "java.util.Set"),
          ("values.toHashSet()", "toHashSet", "java.util.HashSet(java.util.Map)", "java.util.HashSet"),
          ("values.toMutableSet()", "toMutableSet", "java.util.Set(java.util.Map)", "java.util.Set"),
          ("values.filterNotNull()", "filterNotNull", "java.util.List(java.util.Map)", "java.util.List"),
          ("values.requireNoNulls()", "requireNoNulls", "java.util.List(java.util.Map)", "java.util.List"),
          (
            "values.groupBy { entry -> entry.value }",
            "groupBy",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.groupBy({ entry -> entry.value }, { entry -> entry.key })",
            "groupBy",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.groupingBy { entry -> entry.value }",
            "groupingBy",
            "kotlin.collections.Grouping(java.util.Map,kotlin.jvm.functions.Function1)",
            "kotlin.collections.Grouping"
          ),
          (
            "values.associate { entry -> entry.key to entry.value }",
            "associate",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateBy { entry -> entry.value }",
            "associateBy",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateBy({ entry -> entry.value }, { entry -> entry.key })",
            "associateBy",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateWith { entry -> entry.value }",
            "associateWith",
            "java.util.Map(java.util.Map,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = cpg.local
          .nameExact(
            "indexed",
            "setCopy",
            "hashSetCopy",
            "mutableSetCopy",
            "filteredNotNull",
            "noNulls",
            "grouped",
            "groupedValue",
            "grouping",
            "associated",
            "associatedBy",
            "associatedByValue",
            "associatedWith"
          )
          .map(local => local.name -> local.typeFullName)
          .toMap

        localTypes should contain("indexed" -> "java.lang.Iterable")
        localTypes should contain("setCopy" -> "java.util.Set")
        localTypes should contain("hashSetCopy" -> "java.util.HashSet")
        localTypes should contain("mutableSetCopy" -> "java.util.Set")
        localTypes should contain("filteredNotNull" -> "java.util.List")
        localTypes should contain("noNulls" -> "java.util.List")
        localTypes should contain("grouped" -> "java.util.Map")
        localTypes should contain("groupedValue" -> "java.util.Map")
        localTypes should contain("grouping" -> "kotlin.collections.Grouping")
        localTypes should contain("associated" -> "java.util.Map")
        localTypes should contain("associatedBy" -> "java.util.Map")
        localTypes should contain("associatedByValue" -> "java.util.Map")
        localTypes should contain("associatedWith" -> "java.util.Map")
      }
    }

    "resolve map destination extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun mapDestinations(values: Map<String, Int>) {
          |  val associated = values.associateTo(mutableMapOf<String, Int>()) { entry -> entry.key to entry.value }
          |  val associatedBy = values.associateByTo(mutableMapOf<Int, Map.Entry<String, Int>>()) { entry -> entry.value }
          |  val associatedByValue = values.associateByTo(mutableMapOf<Int, String>(), { entry -> entry.value }, { entry -> entry.key })
          |  val associatedWith = values.associateWithTo(mutableMapOf<Map.Entry<String, Int>, Int>()) { entry -> entry.value }
          |  val grouped = values.groupByTo(mutableMapOf<Int, MutableList<Map.Entry<String, Int>>>()) { entry -> entry.value }
          |  val groupedValue = values.groupByTo(mutableMapOf<Int, MutableList<String>>(), { entry -> entry.value }, { entry -> entry.key })
          |  println(associated)
          |  println(associatedBy)
          |  println(associatedByValue)
          |  println(associatedWith)
          |  println(grouped)
          |  println(groupedValue)
          |}
          |""".stripMargin) { cpg =>
        List(
          (
            "values.associateTo(mutableMapOf<String, Int>()) { entry -> entry.key to entry.value }",
            "associateTo",
            "java.util.Map(java.util.Map,java.util.Map,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.associateByTo(mutableMapOf<Int, Map.Entry<String, Int>>()) { entry -> entry.value }",
            "associateByTo",
            "java.util.Map(java.util.Map,java.util.Map,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.associateByTo(mutableMapOf<Int, String>(), { entry -> entry.value }, { entry -> entry.key })",
            "associateByTo",
            "java.util.Map(java.util.Map,java.util.Map,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.associateWithTo(mutableMapOf<Map.Entry<String, Int>, Int>()) { entry -> entry.value }",
            "associateWithTo",
            "java.util.Map(java.util.Map,java.util.Map,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.groupByTo(mutableMapOf<Int, MutableList<Map.Entry<String, Int>>>()) { entry -> entry.value }",
            "groupByTo",
            "java.util.Map(java.util.Map,java.util.Map,kotlin.jvm.functions.Function1)"
          ),
          (
            "values.groupByTo(mutableMapOf<Int, MutableList<String>>(), { entry -> entry.value }, { entry -> entry.key })",
            "groupByTo",
            "java.util.Map(java.util.Map,java.util.Map,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)"
          )
        ).foreach { case (code, name, signature) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.util.Map"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes = cpg.local
          .nameExact("associated", "associatedBy", "associatedByValue", "associatedWith", "grouped", "groupedValue")
          .map(local => local.name -> local.typeFullName)
          .toMap

        localTypes.values.toSet shouldBe Set("java.util.Map")
      }
    }

    "resolve map member and indexed value calls" in {
      withOxidizedCpg("""package demo
          |
          |fun mapMembers(values: Map<String, Int>, mutableValues: MutableMap<String, Int>, hashValues: HashMap<String, Int>) {
          |  val viaIndex = values["one"]
          |  val viaGet = values.get("one")
          |  val required = values.getValue("one")
          |  val defaulted = values.getOrDefault("missing", 0)
          |  val hasKey = values.containsKey("one")
          |  val hasValue = values.containsValue(1)
          |  val empty = values.isEmpty()
          |  val notEmpty = values.isNotEmpty()
          |  val size = values.size
          |  val previous = mutableValues.put("two", 2)
          |  val removed = mutableValues.remove("two")
          |  val hashHasKey = hashValues.containsKey("one")
          |  println(viaIndex)
          |  println(viaGet)
          |  println(required)
          |  println(defaulted)
          |  println(hasKey)
          |  println(hasValue)
          |  println(empty)
          |  println(notEmpty)
          |  println(size)
          |  println(previous)
          |  println(removed)
          |  println(hashHasKey)
          |}
          |""".stripMargin) { cpg =>
        val List(indexCall) = cpg.call.nameExact(Operators.indexAccess).codeExact("""values["one"]""").l: @unchecked
        indexCall.typeFullName shouldBe "int"
        indexCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        List(
          ("values.get(\"one\")", "get", "kotlin.collections.Map.get", "java.lang.Object(java.lang.Object)", "int"),
          (
            "values.getValue(\"one\")",
            "getValue",
            "kotlin.collections.getValue",
            "java.lang.Object(java.util.Map,java.lang.Object)",
            "int"
          ),
          (
            "values.getOrDefault(\"missing\", 0)",
            "getOrDefault",
            "kotlin.collections.Map.getOrDefault",
            "java.lang.Object(java.lang.Object,java.lang.Object)",
            "int"
          ),
          (
            "values.containsKey(\"one\")",
            "containsKey",
            "kotlin.collections.Map.containsKey",
            "boolean(java.lang.Object)",
            "boolean"
          ),
          (
            "values.containsValue(1)",
            "containsValue",
            "kotlin.collections.Map.containsValue",
            "boolean(java.lang.Object)",
            "boolean"
          ),
          ("values.isEmpty()", "isEmpty", "kotlin.collections.Map.isEmpty", "boolean()", "boolean"),
          ("values.isNotEmpty()", "isNotEmpty", "kotlin.collections.isNotEmpty", "boolean(java.util.Map)", "boolean"),
          (
            "mutableValues.put(\"two\", 2)",
            "put",
            "kotlin.collections.MutableMap.put",
            "java.lang.Object(java.lang.Object,java.lang.Object)",
            "int"
          ),
          (
            "mutableValues.remove(\"two\")",
            "remove",
            "kotlin.collections.MutableMap.remove",
            "java.lang.Object(java.lang.Object)",
            "int"
          ),
          (
            "hashValues.containsKey(\"one\")",
            "containsKey",
            "java.util.HashMap.containsKey",
            "boolean(java.lang.Object)",
            "boolean"
          )
        ).foreach { case (code, name, fullNameBase, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe (if (name == "getValue" || name == "isNotEmpty") {
                                        DispatchTypes.STATIC_DISPATCH
                                      } else {
                                        DispatchTypes.DYNAMIC_DISPATCH
                                      })
        }

        val List(sizeAccess) = cpg.call.nameExact(Operators.fieldAccess).codeExact("values.size").l: @unchecked
        sizeAccess.typeFullName shouldBe "int"
        sizeAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List("size")

        cpg.local
          .nameExact("viaIndex", "viaGet", "required", "defaulted", "previous", "removed")
          .typeFullName
          .l shouldBe List.fill(6)("int")
        cpg.local.nameExact("hasKey", "hasValue", "empty", "notEmpty", "hashHasKey").typeFullName.l shouldBe
          List.fill(5)("boolean")
        cpg.local.nameExact("size").typeFullName.l shouldBe List("int")
      }
    }

    "resolve map defaulting and mutation helper calls" in {
      withOxidizedCpg("""package demo
          |
          |fun mapDefaultMutation(values: Map<String, Int>, mutableValues: MutableMap<String, Int>) {
          |  val fallback = values.getOrElse("missing") { 0 }
          |  val inserted = mutableValues.getOrPut("missing") { 1 }
          |  mutableValues.set("two", 2)
          |  mutableValues["three"] = 3
          |  mutableValues.putAll(mapOf("four" to 4))
          |  mutableValues.clear()
          |  println(fallback)
          |  println(inserted)
          |  println(mutableValues)
          |}
          |""".stripMargin) { cpg =>
        List(
          (
            """values.getOrElse("missing") { 0 }""",
            "getOrElse",
            "kotlin.collections.getOrElse",
            "java.lang.Object(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function0)",
            "int",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            """mutableValues.getOrPut("missing") { 1 }""",
            "getOrPut",
            "kotlin.collections.getOrPut",
            "java.lang.Object(java.util.Map,java.lang.Object,kotlin.jvm.functions.Function0)",
            "int",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            """mutableValues.set("two", 2)""",
            "set",
            "kotlin.collections.set",
            "void(java.util.Map,java.lang.Object,java.lang.Object)",
            "void",
            DispatchTypes.STATIC_DISPATCH
          ),
          (
            """mutableValues.putAll(mapOf("four" to 4))""",
            "putAll",
            "kotlin.collections.MutableMap.putAll",
            "void(java.util.Map)",
            "void",
            DispatchTypes.DYNAMIC_DISPATCH
          ),
          (
            "mutableValues.clear()",
            "clear",
            "kotlin.collections.MutableMap.clear",
            "void()",
            "void",
            DispatchTypes.DYNAMIC_DISPATCH
          )
        ).foreach { case (code, name, fullNameBase, signature, typeFullName, dispatchType) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"$fullNameBase:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe dispatchType
        }

        val List(indexAssignment) =
          cpg.call.nameExact(Operators.assignment).codeExact("""mutableValues["three"] = 3""").l: @unchecked
        indexAssignment.typeFullName shouldBe "ANY"

        cpg.call.nameExact(Operators.indexAccess).codeExact("""mutableValues["three"]""").size shouldBe 1
        cpg.local.nameExact("fallback", "inserted").typeFullName.l shouldBe List("int", "int")
      }
    }

    "resolve map operator calls" in {
      withOxidizedCpg("""package demo
          |
          |fun mapOperators(values: Map<String, Int>, mutableValues: MutableMap<String, Int>) {
          |  val pairPlus = values + ("extra" to 1)
          |  val pairPlusNamed = values.plus("named" to 2)
          |  val iterablePlus = values + listOf("iter" to 3)
          |  val mapPlus = values + mapOf("map" to 4)
          |  val keyMinus = values - "old"
          |  val keyMinusNamed = values.minus("named")
          |  val iterableMinus = values - listOf("old", "older")
          |  mutableValues += "added" to 5
          |  mutableValues -= "removed"
          |  println(pairPlus)
          |  println(pairPlusNamed)
          |  println(iterablePlus)
          |  println(mapPlus)
          |  println(keyMinus)
          |  println(keyMinusNamed)
          |  println(iterableMinus)
          |  println(mutableValues)
          |}
          |""".stripMargin) { cpg =>
        List("""values + ("extra" to 1)""", """values + listOf("iter" to 3)""", """values + mapOf("map" to 4)""")
          .foreach { code =>
            val List(call) = cpg.call.nameExact(Operators.addition).codeExact(code).l: @unchecked
            call.methodFullName shouldBe Operators.addition
            call.typeFullName shouldBe "java.util.Map"
            call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          }

        List("values - \"old\"", """values - listOf("old", "older")""").foreach { code =>
          val List(call) = cpg.call.nameExact(Operators.subtraction).codeExact(code).l: @unchecked
          call.methodFullName shouldBe Operators.subtraction
          call.typeFullName shouldBe "java.util.Map"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val List(plusCall) = cpg.call.nameExact("plus").codeExact("""values.plus("named" to 2)""").l: @unchecked
        plusCall.methodFullName shouldBe "kotlin.collections.plus:java.util.Map(java.util.Map,kotlin.Pair)"
        plusCall.signature shouldBe "java.util.Map(java.util.Map,kotlin.Pair)"
        plusCall.typeFullName shouldBe "java.util.Map"
        plusCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(minusCall) = cpg.call.nameExact("minus").codeExact("""values.minus("named")""").l: @unchecked
        minusCall.methodFullName shouldBe "kotlin.collections.minus:java.util.Map(java.util.Map,java.lang.Object)"
        minusCall.signature shouldBe "java.util.Map(java.util.Map,java.lang.Object)"
        minusCall.typeFullName shouldBe "java.util.Map"
        minusCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        List(
          (Operators.assignmentPlus, """mutableValues += "added" to 5"""),
          (Operators.assignmentMinus, "mutableValues -= \"removed\"")
        ).foreach { case (name, code) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe name
          call.typeFullName shouldBe "void"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "pairPlus",
            "pairPlusNamed",
            "iterablePlus",
            "mapPlus",
            "keyMinus",
            "keyMinusNamed",
            "iterableMinus"
          )
          .typeFullName
          .l shouldBe List.fill(7)("java.util.Map")
      }
    }

    "resolve map view element types" in {
      withOxidizedCpg("""package demo
          |
          |fun mapViews(values: Map<String, Int>) {
          |  val firstValue = values.values.first()
          |  val firstKey = values.keys.first()
          |  val firstEntry = values.entries.first()
          |  val entryKey = firstEntry.key
          |  val entryValue = firstEntry.value
          |  values.values.forEach { value -> println(value) }
          |  values.keys.forEach { key -> println(key) }
          |  values.entries.forEach { entry ->
          |    println(entry.key)
          |    println(entry.value)
          |  }
          |  println(firstValue)
          |  println(firstKey)
          |  println(firstEntry)
          |  println(entryKey)
          |  println(entryValue)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("values.values.first()", "int"),
          ("values.keys.first()", "java.lang.String"),
          ("values.entries.first()", "java.util.Map$Entry")
        ).foreach { case (code, typeFullName) =>
          val List(call) = cpg.call.nameExact("first").codeExact(code).l: @unchecked
          call.methodFullName shouldBe "kotlin.collections.first:java.lang.Object(java.lang.Iterable)"
          call.signature shouldBe "java.lang.Object(java.lang.Iterable)"
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List(
          ("firstEntry.key", "key", "java.lang.String"),
          ("firstEntry.value", "value", "int"),
          ("entry.key", "key", "java.lang.String"),
          ("entry.value", "value", "int")
        ).foreach { case (code, fieldName, typeFullName) =>
          val fieldAccesses = cpg.call.nameExact(Operators.fieldAccess).codeExact(code).l
          fieldAccesses.nonEmpty shouldBe true
          fieldAccesses.foreach { fieldAccess =>
            fieldAccess.typeFullName shouldBe typeFullName
            fieldAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List(fieldName)
          }
        }

        List("values.values.forEach { value -> println(value) }", "values.keys.forEach { key -> println(key) }")
          .foreach { code =>
            val List(call) = cpg.call.nameExact("forEach").codeExact(code).l: @unchecked
            call.methodFullName shouldBe "kotlin.collections.forEach:void(java.lang.Iterable,kotlin.jvm.functions.Function1)"
            call.signature shouldBe "void(java.lang.Iterable,kotlin.jvm.functions.Function1)"
            call.typeFullName shouldBe "void"
            call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          }

        cpg.local.nameExact("firstValue").typeFullName.l shouldBe List("int")
        cpg.local.nameExact("firstKey").typeFullName.l shouldBe List("java.lang.String")
        cpg.local.nameExact("firstEntry").typeFullName.l shouldBe List("java.util.Map$Entry")
        cpg.local.nameExact("entryKey").typeFullName.l shouldBe List("java.lang.String")
        cpg.local.nameExact("entryValue").typeFullName.l shouldBe List("int")
      }
    }

    "resolve map entry destructuring component types" in {
      withOxidizedCpg("""package demo
          |
          |fun mapEntryDestructuring(values: Map<String, Int>) {
          |  val (localKey, localValue) = values.entries.first()
          |  values.entries.forEach { (lambdaKey, lambdaValue) ->
          |    println(lambdaKey)
          |    println(lambdaValue)
          |  }
          |  for ((loopKey, loopValue) in values) {
          |    println(loopKey)
          |    println(loopValue)
          |  }
          |  for ((entryKey, entryValue) in values.entries) {
          |    println(entryKey)
          |    println(entryValue)
          |  }
          |}
          |""".stripMargin) { cpg =>
        cpg.local.nameExact("localKey", "lambdaKey", "loopKey", "entryKey").typeFullName.l shouldBe
          List.fill(4)("java.lang.String")
        cpg.local.nameExact("localValue", "lambdaValue", "loopValue", "entryValue").typeFullName.l shouldBe
          List.fill(4)("int")

        val component1Calls = cpg.call.nameExact("component1").l
        component1Calls.size shouldBe 4
        component1Calls.foreach { call =>
          call.methodFullName shouldBe "kotlin.collections.component1:java.lang.Object(java.util.Map$Entry)"
          call.signature shouldBe "java.lang.Object(java.util.Map$Entry)"
          call.typeFullName shouldBe "java.lang.String"
          call.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        }

        val component2Calls = cpg.call.nameExact("component2").l
        component2Calls.size shouldBe 4
        component2Calls.foreach { call =>
          call.methodFullName shouldBe "kotlin.collections.component2:java.lang.Object(java.util.Map$Entry)"
          call.signature shouldBe "java.lang.Object(java.util.Map$Entry)"
          call.typeFullName shouldBe "int"
          call.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        }

        val List(mapIterator) = cpg.call.nameExact("iterator").codeExact("values.iterator()").l: @unchecked
        mapIterator.methodFullName shouldBe "java.util.Map.iterator:java.util.Iterator()"
        mapIterator.signature shouldBe "java.util.Iterator()"
        mapIterator.typeFullName shouldBe "java.util.Iterator"
        mapIterator.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

        val List(entriesIterator) = cpg.call.nameExact("iterator").codeExact("values.entries.iterator()").l: @unchecked
        entriesIterator.methodFullName shouldBe "java.util.Set.iterator:java.util.Iterator()"
        entriesIterator.signature shouldBe "java.util.Iterator()"
        entriesIterator.typeFullName shouldBe "java.util.Iterator"
        entriesIterator.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
      }
    }

    "resolve collection aggregation and accumulation extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun aggregations(values: List<Int>) {
          |  val joined = values.joinToString()
          |  val joinedSep = values.joinToString(separator = "|")
          |  val joinedTo = values.joinTo(StringBuilder())
          |  val folded = values.fold(0) { acc, item -> acc + item }
          |  val foldedIndexed = values.foldIndexed(0) { index, acc, item -> acc + item + index }
          |  val foldedRight = values.foldRight(0) { item, acc -> acc + item }
          |  val foldedRightIndexed = values.foldRightIndexed(0) { index, item, acc -> acc + item + index }
          |  val reduced = values.reduce { acc, item -> acc + item }
          |  val reducedIndexed = values.reduceIndexed { index, acc, item -> acc + item + index }
          |  val reducedOrNull = values.reduceOrNull { acc, item -> acc + item }
          |  val reducedIndexedOrNull = values.reduceIndexedOrNull { index, acc, item -> acc + item + index }
          |  val reducedRight = values.reduceRight { item, acc -> acc + item }
          |  val reducedRightIndexed = values.reduceRightIndexed { index, item, acc -> acc + item + index }
          |  val reducedRightOrNull = values.reduceRightOrNull { item, acc -> acc + item }
          |  val reducedRightIndexedOrNull = values.reduceRightIndexedOrNull { index, item, acc -> acc + item + index }
          |  val total = values.sum()
          |  val average = values.average()
          |  val minValue = values.minOrNull()
          |  val maxValue = values.maxOrNull()
          |  println(joined)
          |  println(joinedSep)
          |  println(joinedTo)
          |  println(folded)
          |  println(foldedIndexed)
          |  println(foldedRight)
          |  println(foldedRightIndexed)
          |  println(reduced)
          |  println(reducedIndexed)
          |  println(reducedOrNull)
          |  println(reducedIndexedOrNull)
          |  println(reducedRight)
          |  println(reducedRightIndexed)
          |  println(reducedRightOrNull)
          |  println(reducedRightIndexedOrNull)
          |  println(total)
          |  println(average)
          |  println(minValue)
          |  println(maxValue)
          |}
          |""".stripMargin) { cpg =>
        val joinToStringSignature =
          "java.lang.String(java.lang.Iterable,java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)"
        List(
          ("values.joinToString()", "joinToString", joinToStringSignature, "java.lang.String"),
          ("""values.joinToString(separator = "|")""", "joinToString", joinToStringSignature, "java.lang.String"),
          (
            "values.joinTo(StringBuilder())",
            "joinTo",
            "java.lang.Appendable(java.lang.Iterable,java.lang.Appendable,java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.StringBuilder"
          ),
          (
            "values.fold(0) { acc, item -> acc + item }",
            "fold",
            "java.lang.Object(java.lang.Iterable,java.lang.Object,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.foldIndexed(0) { index, acc, item -> acc + item + index }",
            "foldIndexed",
            "java.lang.Object(java.lang.Iterable,java.lang.Object,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.foldRight(0) { item, acc -> acc + item }",
            "foldRight",
            "java.lang.Object(java.util.List,java.lang.Object,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.foldRightIndexed(0) { index, item, acc -> acc + item + index }",
            "foldRightIndexed",
            "java.lang.Object(java.util.List,java.lang.Object,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.reduce { acc, item -> acc + item }",
            "reduce",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.reduceIndexed { index, acc, item -> acc + item + index }",
            "reduceIndexed",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.reduceOrNull { acc, item -> acc + item }",
            "reduceOrNull",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.reduceIndexedOrNull { index, acc, item -> acc + item + index }",
            "reduceIndexedOrNull",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.reduceRight { item, acc -> acc + item }",
            "reduceRight",
            "java.lang.Object(java.util.List,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.reduceRightIndexed { index, item, acc -> acc + item + index }",
            "reduceRightIndexed",
            "java.lang.Object(java.util.List,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.reduceRightOrNull { item, acc -> acc + item }",
            "reduceRightOrNull",
            "java.lang.Object(java.util.List,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.reduceRightIndexedOrNull { index, item, acc -> acc + item + index }",
            "reduceRightIndexedOrNull",
            "java.lang.Object(java.util.List,kotlin.jvm.functions.Function3)",
            "int"
          ),
          ("values.sum()", "sum", "int(java.lang.Iterable)", "int"),
          ("values.average()", "average", "double(java.lang.Iterable)", "double"),
          ("values.minOrNull()", "minOrNull", "java.lang.Comparable(java.lang.Iterable)", "int"),
          ("values.maxOrNull()", "maxOrNull", "java.lang.Comparable(java.lang.Iterable)", "int")
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local.nameExact("joined", "joinedSep").typeFullName.l shouldBe List.fill(2)("java.lang.String")
        cpg.local.nameExact("joinedTo").typeFullName.l shouldBe List("java.lang.StringBuilder")
        cpg.local
          .nameExact(
            "folded",
            "foldedIndexed",
            "foldedRight",
            "foldedRightIndexed",
            "reduced",
            "reducedIndexed",
            "reducedOrNull",
            "reducedIndexedOrNull",
            "reducedRight",
            "reducedRightIndexed",
            "reducedRightOrNull",
            "reducedRightIndexedOrNull",
            "total",
            "minValue",
            "maxValue"
          )
          .typeFullName
          .l shouldBe List.fill(15)("int")
        cpg.local.nameExact("average").typeFullName.l shouldBe List("double")
      }
    }

    "resolve collection windowing and running extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun collectionWindowing(values: List<String>, ints: List<Int>) {
          |  val chunked = values.chunked(2)
          |  val chunkedTransform = values.chunked(2) { it.joinToString() }
          |  val windowed = values.windowed(2)
          |  val windowedStep = values.windowed(2, 1)
          |  val windowedPartial = values.windowed(2, 1, true)
          |  val windowedTransform = values.windowed(2, 1, true) { it.joinToString() }
          |  val zippedNext = values.zipWithNext()
          |  val zippedNextTransform = values.zipWithNext { left, right -> left + right }
          |  val runningFold = ints.runningFold(0) { acc, item -> acc + item }
          |  val runningFoldIndexed = ints.runningFoldIndexed(0) { index, acc, item -> acc + item + index }
          |  val runningReduce = ints.runningReduce { acc, item -> acc + item }
          |  val runningReduceIndexed = ints.runningReduceIndexed { index, acc, item -> acc + item + index }
          |  val scan = ints.scan(0) { acc, item -> acc + item }
          |  val scanIndexed = ints.scanIndexed(0) { index, acc, item -> acc + item + index }
          |}
          |""".stripMargin) { cpg =>
        val List(collectionWindowing) = cpg.method.nameExact("collectionWindowing").l: @unchecked

        List(
          ("values.chunked(2)", "chunked", "java.util.List(java.lang.Iterable,int)"),
          (
            "values.chunked(2) { it.joinToString() }",
            "chunked",
            "java.util.List(java.lang.Iterable,int,kotlin.jvm.functions.Function1)"
          ),
          ("values.windowed(2)", "windowed", "java.util.List(java.lang.Iterable,int,int,boolean)"),
          ("values.windowed(2, 1)", "windowed", "java.util.List(java.lang.Iterable,int,int,boolean)"),
          ("values.windowed(2, 1, true)", "windowed", "java.util.List(java.lang.Iterable,int,int,boolean)"),
          (
            "values.windowed(2, 1, true) { it.joinToString() }",
            "windowed",
            "java.util.List(java.lang.Iterable,int,int,boolean,kotlin.jvm.functions.Function1)"
          ),
          ("values.zipWithNext()", "zipWithNext", "java.util.List(java.lang.Iterable)"),
          (
            "values.zipWithNext { left, right -> left + right }",
            "zipWithNext",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.runningFold(0) { acc, item -> acc + item }",
            "runningFold",
            "java.util.List(java.lang.Iterable,java.lang.Object,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.runningFoldIndexed(0) { index, acc, item -> acc + item + index }",
            "runningFoldIndexed",
            "java.util.List(java.lang.Iterable,java.lang.Object,kotlin.jvm.functions.Function3)"
          ),
          (
            "ints.runningReduce { acc, item -> acc + item }",
            "runningReduce",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.runningReduceIndexed { index, acc, item -> acc + item + index }",
            "runningReduceIndexed",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function3)"
          ),
          (
            "ints.scan(0) { acc, item -> acc + item }",
            "scan",
            "java.util.List(java.lang.Iterable,java.lang.Object,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.scanIndexed(0) { index, acc, item -> acc + item + index }",
            "scanIndexed",
            "java.util.List(java.lang.Iterable,java.lang.Object,kotlin.jvm.functions.Function3)"
          )
        ).foreach { case (code, name, signature) =>
          val List(call) = collectionWindowing.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.util.List"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        collectionWindowing.ast.isLocal.filterNot(_.name.startsWith("tmp_")).typeFullName.l.distinct shouldBe
          List("java.util.List")
      }
    }

    "resolve collection pairing and grouping extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun grouping(values: List<String>, numbers: List<Int>, pairs: List<Pair<String, Int>>, nested: List<List<String>>) {
          |  val zipped = values.zip(numbers)
          |  val zippedTransform = values.zip(numbers) { text, number -> text + number.toString() }
          |  val unzipped = pairs.unzip()
          |  val partitioned = values.partition { it.isNotEmpty() }
          |  val grouped = values.groupBy { it.length }
          |  val groupedValue = values.groupBy({ it.length }, { it })
          |  val valueGrouping = values.groupingBy { it.length }
          |  val associated = values.associate { it to it.length }
          |  val associatedBy = values.associateBy { it.length }
          |  val associatedByValue = values.associateBy({ it.length }, { it })
          |  val associatedWith = values.associateWith { it.length }
          |  val flattened = nested.flatten()
          |  val flatMapped = values.flatMap { listOf(it, it.uppercase()) }
          |  println(zipped)
          |  println(zippedTransform)
          |  println(unzipped)
          |  println(partitioned)
          |  println(grouped)
          |  println(groupedValue)
          |  println(valueGrouping)
          |  println(associated)
          |  println(associatedBy)
          |  println(associatedByValue)
          |  println(associatedWith)
          |  println(flattened)
          |  println(flatMapped)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("values.zip(numbers)", "zip", "java.util.List(java.lang.Iterable,java.lang.Iterable)", "java.util.List"),
          (
            "values.zip(numbers) { text, number -> text + number.toString() }",
            "zip",
            "java.util.List(java.lang.Iterable,java.lang.Iterable,kotlin.jvm.functions.Function2)",
            "java.util.List"
          ),
          ("pairs.unzip()", "unzip", "kotlin.Pair(java.lang.Iterable)", "kotlin.Pair"),
          (
            "values.partition { it.isNotEmpty() }",
            "partition",
            "kotlin.Pair(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "kotlin.Pair"
          ),
          (
            "values.groupBy { it.length }",
            "groupBy",
            "java.util.Map(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.groupBy({ it.length }, { it })",
            "groupBy",
            "java.util.Map(java.lang.Iterable,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.groupingBy { it.length }",
            "groupingBy",
            "kotlin.collections.Grouping(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "kotlin.collections.Grouping"
          ),
          (
            "values.associate { it to it.length }",
            "associate",
            "java.util.Map(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateBy { it.length }",
            "associateBy",
            "java.util.Map(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateBy({ it.length }, { it })",
            "associateBy",
            "java.util.Map(java.lang.Iterable,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateWith { it.length }",
            "associateWith",
            "java.util.Map(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          ("nested.flatten()", "flatten", "java.util.List(java.lang.Iterable)", "java.util.List"),
          (
            "values.flatMap { listOf(it, it.uppercase()) }",
            "flatMap",
            "java.util.List(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.util.List"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local.nameExact("zipped", "zippedTransform", "flattened", "flatMapped").typeFullName.l shouldBe
          List.fill(4)("java.util.List")
        cpg.local.nameExact("unzipped", "partitioned").typeFullName.l shouldBe List.fill(2)("kotlin.Pair")
        cpg.local
          .nameExact("grouped", "groupedValue", "associated", "associatedBy", "associatedByValue", "associatedWith")
          .typeFullName
          .l shouldBe List.fill(6)("java.util.Map")
        cpg.local.nameExact("valueGrouping").typeFullName.l shouldBe List("kotlin.collections.Grouping")
      }
    }

    "resolve collection and array set operation extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun setOps(
          |  values: List<String>,
          |  more: List<String>,
          |  set: Set<String>,
          |  strings: Array<String>,
          |  ints: IntArray
          |) {
          |  val valuesUnion = values.union(more)
          |  val valuesIntersect = values.intersect(more)
          |  val valuesSubtract = values.subtract(more)
          |  val setUnion = set.union(more)
          |  val setIntersect = set.intersect(more)
          |  val setSubtract = set.subtract(more)
          |  val stringsUnion = strings.union(more)
          |  val stringsIntersect = strings.intersect(more)
          |  val stringsSubtract = strings.subtract(more)
          |  val intsUnion = ints.union(listOf(1))
          |  val intsIntersect = ints.intersect(listOf(1))
          |  val intsSubtract = ints.subtract(listOf(1))
          |}
          |""".stripMargin) { cpg =>
        val iterableSetSignature = "java.util.Set(java.lang.Iterable,java.lang.Iterable)"
        val objectArraySignature = "java.util.Set(java.lang.Object[],java.lang.Iterable)"
        val intArraySignature    = "java.util.Set(int[],java.lang.Iterable)"

        List(
          ("values.union(more)", "union", iterableSetSignature),
          ("values.intersect(more)", "intersect", iterableSetSignature),
          ("values.subtract(more)", "subtract", iterableSetSignature),
          ("set.union(more)", "union", iterableSetSignature),
          ("set.intersect(more)", "intersect", iterableSetSignature),
          ("set.subtract(more)", "subtract", iterableSetSignature),
          ("strings.union(more)", "union", objectArraySignature),
          ("strings.intersect(more)", "intersect", objectArraySignature),
          ("strings.subtract(more)", "subtract", objectArraySignature),
          ("ints.union(listOf(1))", "union", intArraySignature),
          ("ints.intersect(listOf(1))", "intersect", intArraySignature),
          ("ints.subtract(listOf(1))", "subtract", intArraySignature)
        ).foreach { case (code, name, signature) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.util.Set"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.method
          .nameExact("setOps")
          .ast
          .isLocal
          .filterNot(_.name.startsWith("tmp_"))
          .typeFullName
          .l
          .distinct shouldBe List("java.util.Set")
      }
    }

    "resolve collection and array conversion and slice extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun conversions(
          |  values: List<String>,
          |  booleans: List<Boolean>,
          |  bytes: List<Byte>,
          |  shorts: List<Short>,
          |  chars: List<Char>,
          |  numbers: List<Int>,
          |  longs: List<Long>,
          |  floats: List<Float>,
          |  doubles: List<Double>,
          |  strings: Array<String>,
          |  ints: IntArray,
          |  comparator: java.util.Comparator<String>,
          |  intComparator: java.util.Comparator<Int>
          |) {
          |  val stringsAsList = strings.asList()
          |  val intsAsList = ints.asList()
          |  val stringsSortedSet = strings.toSortedSet()
          |  val intsSortedSet = ints.toSortedSet()
          |  val stringsSortedSetWithComparator = strings.toSortedSet(comparator)
          |  val intsSortedSetWithComparator = ints.toSortedSet(intComparator)
          |  val valuesTyped = values.toTypedArray()
          |  val numbersTyped = numbers.toTypedArray()
          |  val intsTyped = ints.toTypedArray()
          |  val booleansBooleanArray = booleans.toBooleanArray()
          |  val bytesByteArray = bytes.toByteArray()
          |  val shortsShortArray = shorts.toShortArray()
          |  val charsCharArray = chars.toCharArray()
          |  val numbersIntArray = numbers.toIntArray()
          |  val longsLongArray = longs.toLongArray()
          |  val floatsFloatArray = floats.toFloatArray()
          |  val doublesDoubleArray = doubles.toDoubleArray()
          |  val valuesSliceRange = values.slice(0..1)
          |  val valuesSliceIndices = values.slice(listOf(0, 1))
          |  val stringsSliceRange = strings.slice(0..1)
          |  val stringsSliceIndices = strings.slice(listOf(0, 1))
          |  val intsSliceRange = ints.slice(0..1)
          |  val intsSliceIndices = ints.slice(listOf(0, 1))
          |}
          |""".stripMargin) { cpg =>
        val objectArray = "java.lang.Object[]"

        List(
          ("strings.asList()", "asList", s"java.util.List($objectArray)", "java.util.List"),
          ("ints.asList()", "asList", "java.util.List(int[])", "java.util.List"),
          (
            "strings.toSortedSet()",
            "toSortedSet",
            "java.util.SortedSet(java.lang.Comparable[])",
            "java.util.SortedSet"
          ),
          ("ints.toSortedSet()", "toSortedSet", "java.util.SortedSet(int[])", "java.util.SortedSet"),
          (
            "strings.toSortedSet(comparator)",
            "toSortedSet",
            "java.util.SortedSet(java.lang.Object[],java.util.Comparator)",
            "java.util.SortedSet"
          ),
          ("ints.toSortedSet(intComparator)", "toSortedSet", "java.util.SortedSet(int[])", "java.util.SortedSet"),
          ("values.toTypedArray()", "toTypedArray", "java.lang.Object[](java.util.Collection)", "java.lang.String[]"),
          ("numbers.toTypedArray()", "toTypedArray", "java.lang.Object[](java.util.Collection)", "int[]"),
          ("ints.toTypedArray()", "toTypedArray", "int[](int[])", "int[]"),
          ("booleans.toBooleanArray()", "toBooleanArray", "boolean[](java.util.Collection)", "boolean[]"),
          ("bytes.toByteArray()", "toByteArray", "byte[](java.util.Collection)", "byte[]"),
          ("shorts.toShortArray()", "toShortArray", "short[](java.util.Collection)", "short[]"),
          ("chars.toCharArray()", "toCharArray", "char[](java.util.Collection)", "char[]"),
          ("numbers.toIntArray()", "toIntArray", "int[](java.util.Collection)", "int[]"),
          ("longs.toLongArray()", "toLongArray", "long[](java.util.Collection)", "long[]"),
          ("floats.toFloatArray()", "toFloatArray", "float[](java.util.Collection)", "float[]"),
          ("doubles.toDoubleArray()", "toDoubleArray", "double[](java.util.Collection)", "double[]"),
          ("values.slice(0..1)", "slice", "java.util.List(java.util.List,kotlin.ranges.IntRange)", "java.util.List"),
          (
            "values.slice(listOf(0, 1))",
            "slice",
            "java.util.List(java.util.List,java.lang.Iterable)",
            "java.util.List"
          ),
          ("strings.slice(0..1)", "slice", s"java.util.List($objectArray,kotlin.ranges.IntRange)", "java.util.List"),
          (
            "strings.slice(listOf(0, 1))",
            "slice",
            s"java.util.List($objectArray,java.lang.Iterable)",
            "java.util.List"
          ),
          ("ints.slice(0..1)", "slice", "java.util.List(int[],kotlin.ranges.IntRange)", "java.util.List"),
          ("ints.slice(listOf(0, 1))", "slice", "java.util.List(int[],java.lang.Iterable)", "java.util.List")
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes =
          cpg.method.nameExact("conversions").ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "stringsAsList"                  -> "java.util.List",
          "intsAsList"                     -> "java.util.List",
          "stringsSortedSet"               -> "java.util.SortedSet",
          "intsSortedSet"                  -> "java.util.SortedSet",
          "stringsSortedSetWithComparator" -> "java.util.SortedSet",
          "intsSortedSetWithComparator"    -> "java.util.SortedSet",
          "valuesTyped"                    -> "java.lang.String[]",
          "numbersTyped"                   -> "int[]",
          "intsTyped"                      -> "int[]",
          "booleansBooleanArray"           -> "boolean[]",
          "bytesByteArray"                 -> "byte[]",
          "shortsShortArray"               -> "short[]",
          "charsCharArray"                 -> "char[]",
          "numbersIntArray"                -> "int[]",
          "longsLongArray"                 -> "long[]",
          "floatsFloatArray"               -> "float[]",
          "doublesDoubleArray"             -> "double[]",
          "valuesSliceRange"               -> "java.util.List",
          "valuesSliceIndices"             -> "java.util.List",
          "stringsSliceRange"              -> "java.util.List",
          "stringsSliceIndices"            -> "java.util.List",
          "intsSliceRange"                 -> "java.util.List",
          "intsSliceIndices"               -> "java.util.List"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve list and array element default extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun defaults(values: List<String>, numbers: List<Int>, strings: Array<String>, ints: IntArray) {
          |  val valueOrNull = values.getOrNull(0)
          |  val valueOrElse = values.getOrElse(0) { "fallback" }
          |  val numberOrNull = numbers.getOrNull(0)
          |  val numberOrElse = numbers.getOrElse(0) { -1 }
          |  val stringOrNull = strings.getOrNull(0)
          |  val stringOrElse = strings.getOrElse(0) { "fallback" }
          |  val intOrNull = ints.getOrNull(0)
          |  val intOrElse = ints.getOrElse(0) { -1 }
          |}
          |""".stripMargin) { cpg =>
        val objectArray = "java.lang.Object[]"
        List(
          ("values.getOrNull(0)", "getOrNull", "java.lang.Object(java.util.List,int)", "java.lang.String"),
          (
            """values.getOrElse(0) { "fallback" }""",
            "getOrElse",
            "java.lang.Object(java.util.List,int,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("numbers.getOrNull(0)", "getOrNull", "java.lang.Object(java.util.List,int)", "int"),
          (
            "numbers.getOrElse(0) { -1 }",
            "getOrElse",
            "java.lang.Object(java.util.List,int,kotlin.jvm.functions.Function1)",
            "int"
          ),
          ("strings.getOrNull(0)", "getOrNull", s"java.lang.Object($objectArray,int)", "java.lang.String"),
          (
            """strings.getOrElse(0) { "fallback" }""",
            "getOrElse",
            s"java.lang.Object($objectArray,int,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("ints.getOrNull(0)", "getOrNull", "int(int[],int)", "int"),
          ("ints.getOrElse(0) { -1 }", "getOrElse", "int(int[],int,kotlin.jvm.functions.Function1)", "int")
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val localTypes =
          cpg.method.nameExact("defaults").ast.isLocal.map(local => local.name -> local.typeFullName).toMap
        Map(
          "valueOrNull"  -> "java.lang.String",
          "valueOrElse"  -> "java.lang.String",
          "numberOrNull" -> "int",
          "numberOrElse" -> "int",
          "stringOrNull" -> "java.lang.String",
          "stringOrElse" -> "java.lang.String",
          "intOrNull"    -> "int",
          "intOrElse"    -> "int"
        ).foreach { case (name, typeFullName) =>
          localTypes should contain(name -> typeFullName)
        }
      }
    }

    "resolve collection destination extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun destinations(values: List<String>, nullableValues: List<String?>) {
          |  val filtered = values.filterTo(mutableListOf<String>()) { it.isNotEmpty() }
          |  val filteredNot = values.filterNotTo(mutableListOf<String>()) { it.isEmpty() }
          |  val filteredIndexed = values.filterIndexedTo(mutableListOf<String>()) { index, item -> index > 0 && item.isNotEmpty() }
          |  val filteredNotNull = nullableValues.filterNotNullTo(mutableListOf<String>())
          |  val mapped = values.mapTo(mutableListOf<Int>()) { it.length }
          |  val mappedIndexed = values.mapIndexedTo(mutableListOf<Int>()) { index, item -> index + item.length }
          |  val mappedNotNull = values.mapNotNullTo(mutableListOf<Int>()) { it.length }
          |  val mappedIndexedNotNull = values.mapIndexedNotNullTo(mutableListOf<Int>()) { index, item -> index + item.length }
          |  val flatMapped = values.flatMapTo(mutableListOf<String>()) { listOf(it, it.uppercase()) }
          |  val flatMappedIndexed = values.flatMapIndexedTo(mutableListOf<String>()) { index, item -> listOf(item + index.toString()) }
          |  val grouped = values.groupByTo(mutableMapOf<Int, MutableList<String>>()) { it.length }
          |  val groupedValue = values.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it.length }, { it })
          |  val associated = values.associateTo(mutableMapOf<String, Int>()) { it to it.length }
          |  val associatedBy = values.associateByTo(mutableMapOf<Int, String>()) { it.length }
          |  val associatedByValue = values.associateByTo(mutableMapOf<Int, String>(), { it.length }, { it })
          |  val associatedWith = values.associateWithTo(mutableMapOf<String, Int>()) { it.length }
          |  println(filtered)
          |  println(filteredNot)
          |  println(filteredIndexed)
          |  println(filteredNotNull)
          |  println(mapped)
          |  println(mappedIndexed)
          |  println(mappedNotNull)
          |  println(mappedIndexedNotNull)
          |  println(flatMapped)
          |  println(flatMappedIndexed)
          |  println(grouped)
          |  println(groupedValue)
          |  println(associated)
          |  println(associatedBy)
          |  println(associatedByValue)
          |  println(associatedWith)
          |}
          |""".stripMargin) { cpg =>
        val collectionFunction1Signature =
          "java.util.Collection(java.lang.Iterable,java.util.Collection,kotlin.jvm.functions.Function1)"
        val collectionFunction2Signature =
          "java.util.Collection(java.lang.Iterable,java.util.Collection,kotlin.jvm.functions.Function2)"
        val mapFunction1Signature = "java.util.Map(java.lang.Iterable,java.util.Map,kotlin.jvm.functions.Function1)"
        val mapFunction2Signature =
          "java.util.Map(java.lang.Iterable,java.util.Map,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)"

        List(
          (
            "values.filterTo(mutableListOf<String>()) { it.isNotEmpty() }",
            "filterTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.filterNotTo(mutableListOf<String>()) { it.isEmpty() }",
            "filterNotTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.filterIndexedTo(mutableListOf<String>()) { index, item -> index > 0 && item.isNotEmpty() }",
            "filterIndexedTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "nullableValues.filterNotNullTo(mutableListOf<String>())",
            "filterNotNullTo",
            "java.util.Collection(java.lang.Iterable,java.util.Collection)",
            "java.util.List"
          ),
          ("values.mapTo(mutableListOf<Int>()) { it.length }", "mapTo", collectionFunction1Signature, "java.util.List"),
          (
            "values.mapIndexedTo(mutableListOf<Int>()) { index, item -> index + item.length }",
            "mapIndexedTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "values.mapNotNullTo(mutableListOf<Int>()) { it.length }",
            "mapNotNullTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.mapIndexedNotNullTo(mutableListOf<Int>()) { index, item -> index + item.length }",
            "mapIndexedNotNullTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "values.flatMapTo(mutableListOf<String>()) { listOf(it, it.uppercase()) }",
            "flatMapTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.flatMapIndexedTo(mutableListOf<String>()) { index, item -> listOf(item + index.toString()) }",
            "flatMapIndexedTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "values.groupByTo(mutableMapOf<Int, MutableList<String>>()) { it.length }",
            "groupByTo",
            mapFunction1Signature,
            "java.util.Map"
          ),
          (
            "values.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it.length }, { it })",
            "groupByTo",
            mapFunction2Signature,
            "java.util.Map"
          ),
          (
            "values.associateTo(mutableMapOf<String, Int>()) { it to it.length }",
            "associateTo",
            mapFunction1Signature,
            "java.util.Map"
          ),
          (
            "values.associateByTo(mutableMapOf<Int, String>()) { it.length }",
            "associateByTo",
            mapFunction1Signature,
            "java.util.Map"
          ),
          (
            "values.associateByTo(mutableMapOf<Int, String>(), { it.length }, { it })",
            "associateByTo",
            mapFunction2Signature,
            "java.util.Map"
          ),
          (
            "values.associateWithTo(mutableMapOf<String, Int>()) { it.length }",
            "associateWithTo",
            mapFunction1Signature,
            "java.util.Map"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "filtered",
            "filteredNot",
            "filteredIndexed",
            "filteredNotNull",
            "mapped",
            "mappedIndexed",
            "mappedNotNull",
            "mappedIndexedNotNull",
            "flatMapped",
            "flatMappedIndexed"
          )
          .typeFullName
          .l shouldBe List.fill(10)("java.util.List")
        cpg.local
          .nameExact("grouped", "groupedValue", "associated", "associatedBy", "associatedByValue", "associatedWith")
          .typeFullName
          .l shouldBe List.fill(6)("java.util.Map")
      }
    }

    "resolve collection selector and comparator extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun selectors(values: List<String>, comparator: Comparator<String>) {
          |  val minBy = values.minBy { it.length }
          |  val maxBy = values.maxBy { it.length }
          |  val minByOrNull = values.minByOrNull { it.length }
          |  val maxByOrNull = values.maxByOrNull { it.length }
          |  val minWith = values.minWith(comparator)
          |  val maxWith = values.maxWith(comparator)
          |  val minWithOrNull = values.minWithOrNull(comparator)
          |  val maxWithOrNull = values.maxWithOrNull(comparator)
          |  val sortedWith = values.sortedWith(comparator)
          |  println(minBy)
          |  println(maxBy)
          |  println(minByOrNull)
          |  println(maxByOrNull)
          |  println(minWith)
          |  println(maxWith)
          |  println(minWithOrNull)
          |  println(maxWithOrNull)
          |  println(sortedWith)
          |}
          |""".stripMargin) { cpg =>
        val selectorSignature   = "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)"
        val comparatorSignature = "java.lang.Object(java.lang.Iterable,java.util.Comparator)"

        List(
          ("values.minBy { it.length }", "minBy", selectorSignature, "java.lang.String"),
          ("values.maxBy { it.length }", "maxBy", selectorSignature, "java.lang.String"),
          ("values.minByOrNull { it.length }", "minByOrNull", selectorSignature, "java.lang.String"),
          ("values.maxByOrNull { it.length }", "maxByOrNull", selectorSignature, "java.lang.String"),
          ("values.minWith(comparator)", "minWith", comparatorSignature, "java.lang.String"),
          ("values.maxWith(comparator)", "maxWith", comparatorSignature, "java.lang.String"),
          ("values.minWithOrNull(comparator)", "minWithOrNull", comparatorSignature, "java.lang.String"),
          ("values.maxWithOrNull(comparator)", "maxWithOrNull", comparatorSignature, "java.lang.String"),
          (
            "values.sortedWith(comparator)",
            "sortedWith",
            "java.util.List(java.lang.Iterable,java.util.Comparator)",
            "java.util.List"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "minBy",
            "maxBy",
            "minByOrNull",
            "maxByOrNull",
            "minWith",
            "maxWith",
            "minWithOrNull",
            "maxWithOrNull"
          )
          .typeFullName
          .l shouldBe List.fill(8)("java.lang.String")
        cpg.local.nameExact("sortedWith").typeFullName.l shouldBe List("java.util.List")
      }
    }

    "resolve collection lambda-result extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun lambdaResults(values: List<String>) {
          |  val minLength = values.minOf { it.length }
          |  val maxLength = values.maxOf { it.length }
          |  val minLengthOrNull = values.minOfOrNull { it.length }
          |  val maxLengthOrNull = values.maxOfOrNull { it.length }
          |  val minText = values.minOf { it }
          |  val maxText = values.maxOf { it }
          |  val intTotal = values.sumOf { it.length }
          |  val longTotal = values.sumOf { it.length.toLong() }
          |  val doubleTotal = values.sumOf { it.length.toDouble() }
          |  println(minLength)
          |  println(maxLength)
          |  println(minLengthOrNull)
          |  println(maxLengthOrNull)
          |  println(minText)
          |  println(maxText)
          |  println(intTotal)
          |  println(longTotal)
          |  println(doubleTotal)
          |}
          |""".stripMargin) { cpg =>
        val comparableSignature = "java.lang.Comparable(java.lang.Iterable,kotlin.jvm.functions.Function1)"

        List(
          ("values.minOf { it.length }", "minOf", comparableSignature, "int"),
          ("values.maxOf { it.length }", "maxOf", comparableSignature, "int"),
          ("values.minOfOrNull { it.length }", "minOfOrNull", comparableSignature, "int"),
          ("values.maxOfOrNull { it.length }", "maxOfOrNull", comparableSignature, "int"),
          ("values.minOf { it }", "minOf", comparableSignature, "java.lang.String"),
          ("values.maxOf { it }", "maxOf", comparableSignature, "java.lang.String"),
          ("values.sumOf { it.length }", "sumOf", "int(java.lang.Iterable,kotlin.jvm.functions.Function1)", "int"),
          (
            "values.sumOf { it.length.toLong() }",
            "sumOf",
            "long(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "long"
          ),
          (
            "values.sumOf { it.length.toDouble() }",
            "sumOf",
            "double(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "double"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact("minLength", "maxLength", "minLengthOrNull", "maxLengthOrNull", "intTotal")
          .typeFullName
          .l shouldBe List.fill(5)("int")
        cpg.local.nameExact("minText", "maxText").typeFullName.l shouldBe List.fill(2)("java.lang.String")
        cpg.local.nameExact("longTotal").typeFullName.l shouldBe List("long")
        cpg.local.nameExact("doubleTotal").typeFullName.l shouldBe List("double")
      }
    }

    "resolve first-not-null lambda-result extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun firstNotNulls(values: List<String>, strings: Array<String>, ints: IntArray, seq: Sequence<String>) {
          |  val listFirst = values.firstNotNullOf { it.length }
          |  val listFirstOrNull = values.firstNotNullOfOrNull { it.length }
          |  val arrayFirst = strings.firstNotNullOf { it.length }
          |  val arrayFirstOrNull = strings.firstNotNullOfOrNull { it.length }
          |  val intArrayFirst = ints.firstNotNullOf { it.toString() }
          |  val intArrayFirstOrNull = ints.firstNotNullOfOrNull { it.toString() }
          |  val sequenceFirst = seq.firstNotNullOf { it.length }
          |  val sequenceFirstOrNull = seq.firstNotNullOfOrNull { it.length }
          |}
          |""".stripMargin) { cpg =>
        List(
          (
            "values.firstNotNullOf { it.length }",
            "firstNotNullOf",
            "kotlin.collections.firstNotNullOf:java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.firstNotNullOfOrNull { it.length }",
            "firstNotNullOfOrNull",
            "kotlin.collections.firstNotNullOfOrNull:java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Iterable,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.firstNotNullOf { it.length }",
            "firstNotNullOf",
            "kotlin.collections.firstNotNullOf:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "strings.firstNotNullOfOrNull { it.length }",
            "firstNotNullOfOrNull",
            "kotlin.collections.firstNotNullOfOrNull:java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "java.lang.Object(java.lang.Object[],kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "ints.firstNotNullOf { it.toString() }",
            "firstNotNullOf",
            "kotlin.collections.firstNotNullOf:java.lang.Object(int[],kotlin.jvm.functions.Function1)",
            "java.lang.Object(int[],kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "ints.firstNotNullOfOrNull { it.toString() }",
            "firstNotNullOfOrNull",
            "kotlin.collections.firstNotNullOfOrNull:java.lang.Object(int[],kotlin.jvm.functions.Function1)",
            "java.lang.Object(int[],kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.firstNotNullOf { it.length }",
            "firstNotNullOf",
            "kotlin.sequences.firstNotNullOf:java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "seq.firstNotNullOfOrNull { it.length }",
            "firstNotNullOfOrNull",
            "kotlin.sequences.firstNotNullOfOrNull:java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "int"
          )
        ).foreach { case (code, name, fullName, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe fullName
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "listFirst",
            "listFirstOrNull",
            "arrayFirst",
            "arrayFirstOrNull",
            "sequenceFirst",
            "sequenceFirstOrNull"
          )
          .typeFullName
          .l shouldBe List.fill(6)("int")
        cpg.local.nameExact("intArrayFirst", "intArrayFirstOrNull").typeFullName.l shouldBe
          List.fill(2)("java.lang.String")
      }
    }

    "resolve filter-is-instance extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun filterInstances(values: List<Any?>, strings: Array<Any?>, ints: IntArray, seq: Sequence<Any?>) {
          |  val listFiltered = values.filterIsInstance<String>()
          |  val listFilteredTo = values.filterIsInstanceTo(mutableListOf<String>())
          |  val arrayFiltered = strings.filterIsInstance<String>()
          |  val arrayFilteredTo = strings.filterIsInstanceTo(mutableListOf<String>())
          |  val intArrayFiltered = ints.filterIsInstance<String>()
          |  val intArrayFilteredTo = ints.filterIsInstanceTo(mutableListOf<String>())
          |  val sequenceFiltered = seq.filterIsInstance<String>()
          |  val sequenceFilteredTo = seq.filterIsInstanceTo(mutableListOf<String>())
          |}
          |""".stripMargin) { cpg =>
        List(
          (
            "values.filterIsInstance<String>()",
            "filterIsInstance",
            "kotlin.collections.filterIsInstance:java.util.List(java.lang.Iterable)",
            "java.util.List(java.lang.Iterable)",
            "java.util.List"
          ),
          (
            "values.filterIsInstanceTo(mutableListOf<String>())",
            "filterIsInstanceTo",
            "kotlin.collections.filterIsInstanceTo:java.util.Collection(java.lang.Iterable,java.util.Collection)",
            "java.util.Collection(java.lang.Iterable,java.util.Collection)",
            "java.util.List"
          ),
          (
            "strings.filterIsInstance<String>()",
            "filterIsInstance",
            "kotlin.collections.filterIsInstance:java.util.List(java.lang.Object[])",
            "java.util.List(java.lang.Object[])",
            "java.util.List"
          ),
          (
            "strings.filterIsInstanceTo(mutableListOf<String>())",
            "filterIsInstanceTo",
            "kotlin.collections.filterIsInstanceTo:java.util.Collection(java.lang.Object[],java.util.Collection)",
            "java.util.Collection(java.lang.Object[],java.util.Collection)",
            "java.util.List"
          ),
          (
            "seq.filterIsInstance<String>()",
            "filterIsInstance",
            "kotlin.sequences.filterIsInstance:kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          (
            "seq.filterIsInstanceTo(mutableListOf<String>())",
            "filterIsInstanceTo",
            "kotlin.sequences.filterIsInstanceTo:java.util.Collection(kotlin.sequences.Sequence,java.util.Collection)",
            "java.util.Collection(kotlin.sequences.Sequence,java.util.Collection)",
            "java.util.List"
          )
        ).foreach { case (code, name, fullName, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe fullName
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List(
          ("ints.filterIsInstance<String>()", "filterIsInstance", s"${Defines.UnresolvedSignature}(0)"),
          (
            "ints.filterIsInstanceTo(mutableListOf<String>())",
            "filterIsInstanceTo",
            s"${Defines.UnresolvedSignature}(1)"
          )
        ).foreach { case (code, name, signature) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"${Defines.UnresolvedNamespace}.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe TypeConstants.Any
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact("listFiltered", "listFilteredTo", "arrayFiltered", "arrayFilteredTo", "sequenceFilteredTo")
          .typeFullName
          .l shouldBe List.fill(5)("java.util.List")
        cpg.local.nameExact("sequenceFiltered").typeFullName.l shouldBe List("kotlin.sequences.Sequence")
        cpg.local.nameExact("intArrayFiltered", "intArrayFilteredTo").typeFullName.l shouldBe
          List.fill(2)(TypeConstants.Any)
      }
    }

    "resolve sequence extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun sequences(values: List<String>, comparator: java.util.Comparator<String>) {
          |  val seq = values.asSequence()
          |  val filtered = seq.filter { it.isNotEmpty() }
          |  val filteredNot = seq.filterNot { it.isEmpty() }
          |  val filteredIndexed = seq.filterIndexed { index, item -> index > 0 && item.isNotEmpty() }
          |  val mapped = seq.map { it.length }
          |  val mappedIndexed = seq.mapIndexed { index, item -> index + item.length }
          |  val mappedNotNull = seq.mapNotNull { it.length }
          |  val flatMapped = seq.flatMap { listOf(it, it.uppercase()) }
          |  val flatMappedIndexed = seq.flatMapIndexed { index, item -> listOf(item + index.toString()) }
          |  val taken = seq.take(2)
          |  val dropped = seq.drop(1)
          |  val takenWhile = seq.takeWhile { it.isNotEmpty() }
          |  val droppedWhile = seq.dropWhile { it.isEmpty() }
          |  val first = seq.first()
          |  val firstOrNull = seq.firstOrNull()
          |  val element = seq.elementAt(1)
          |  val elementOrNull = seq.elementAtOrNull(1)
          |  val elementOrElse = seq.elementAtOrElse(1) { "fallback" }
          |  val found = seq.find { it.isNotEmpty() }
          |  val firstMatch = seq.first { it.isNotEmpty() }
          |  val firstOrNullMatch = seq.firstOrNull { it.isNotEmpty() }
          |  val lastMatch = seq.last { it.isNotEmpty() }
          |  val lastOrNullMatch = seq.lastOrNull { it.isNotEmpty() }
          |  val singleMatch = seq.single { it.isNotEmpty() }
          |  val singleOrNullMatch = seq.singleOrNull { it.isNotEmpty() }
          |  val anyPlain = seq.any()
          |  val anyMatch = seq.any { it.isNotEmpty() }
          |  val hasValue = seq.contains("x")
          |  val indexFirst = seq.indexOfFirst { it.isNotEmpty() }
          |  val indexLast = seq.indexOfLast { it.isNotEmpty() }
          |  val countPlain = seq.count()
          |  val countMatch = seq.count { it.isNotEmpty() }
          |  val iterableView = seq.asIterable()
          |  val constrained = seq.constrainOnce()
          |  val listed = seq.toList()
          |  val mutableListed = seq.toMutableList()
          |  val set = seq.toSet()
          |  val mutableSet = seq.toMutableSet()
          |  val hashSet = seq.toHashSet()
          |  val sortedSet = seq.toSortedSet()
          |  val sortedSetWithComparator = seq.toSortedSet(comparator)
          |  val collected = seq.toCollection(mutableListOf<String>())
          |  val joined = seq.joinToString()
          |  val each = seq.onEach { println(it) }
          |  val eachIndexed = seq.onEachIndexed { index, item -> println(item + index.toString()) }
          |  val indexed = seq.withIndex()
          |  seq.forEach { println(it) }
          |  seq.forEachIndexed { index, item -> println(item + index.toString()) }
          |  println(filtered)
          |  println(filteredNot)
          |  println(filteredIndexed)
          |  println(mapped)
          |  println(mappedIndexed)
          |  println(mappedNotNull)
          |  println(flatMapped)
          |  println(flatMappedIndexed)
          |  println(taken)
          |  println(dropped)
          |  println(takenWhile)
          |  println(droppedWhile)
          |  println(first)
          |  println(firstOrNull)
          |  println(element)
          |  println(elementOrNull)
          |  println(elementOrElse)
          |  println(found)
          |  println(firstMatch)
          |  println(firstOrNullMatch)
          |  println(lastMatch)
          |  println(lastOrNullMatch)
          |  println(singleMatch)
          |  println(singleOrNullMatch)
          |  println(anyPlain)
          |  println(anyMatch)
          |  println(hasValue)
          |  println(indexFirst)
          |  println(indexLast)
          |  println(countPlain)
          |  println(countMatch)
          |  println(iterableView)
          |  println(constrained)
          |  println(listed)
          |  println(mutableListed)
          |  println(set)
          |  println(mutableSet)
          |  println(hashSet)
          |  println(sortedSet)
          |  println(sortedSetWithComparator)
          |  println(collected)
          |  println(joined)
          |  println(each)
          |  println(eachIndexed)
          |  println(indexed)
          |}
          |""".stripMargin) { cpg =>
        val sequenceFunction1Signature =
          "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)"
        val sequenceFunction2Signature =
          "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.jvm.functions.Function2)"
        val sequenceIntSignature       = "kotlin.sequences.Sequence(kotlin.sequences.Sequence,int)"
        val sequencePredicateSignature = "boolean(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)"

        List(
          ("seq.filter { it.isNotEmpty() }", "filter", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          ("seq.filterNot { it.isEmpty() }", "filterNot", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          (
            "seq.filterIndexed { index, item -> index > 0 && item.isNotEmpty() }",
            "filterIndexed",
            sequenceFunction2Signature,
            "kotlin.sequences.Sequence"
          ),
          ("seq.map { it.length }", "map", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          (
            "seq.mapIndexed { index, item -> index + item.length }",
            "mapIndexed",
            sequenceFunction2Signature,
            "kotlin.sequences.Sequence"
          ),
          ("seq.mapNotNull { it.length }", "mapNotNull", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          (
            "seq.flatMap { listOf(it, it.uppercase()) }",
            "flatMap",
            sequenceFunction1Signature,
            "kotlin.sequences.Sequence"
          ),
          (
            "seq.flatMapIndexed { index, item -> listOf(item + index.toString()) }",
            "flatMapIndexed",
            sequenceFunction2Signature,
            "kotlin.sequences.Sequence"
          ),
          ("seq.take(2)", "take", sequenceIntSignature, "kotlin.sequences.Sequence"),
          ("seq.drop(1)", "drop", sequenceIntSignature, "kotlin.sequences.Sequence"),
          ("seq.takeWhile { it.isNotEmpty() }", "takeWhile", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          ("seq.dropWhile { it.isEmpty() }", "dropWhile", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          ("seq.first()", "first", "java.lang.Object(kotlin.sequences.Sequence)", "java.lang.String"),
          ("seq.firstOrNull()", "firstOrNull", "java.lang.Object(kotlin.sequences.Sequence)", "java.lang.String"),
          ("seq.elementAt(1)", "elementAt", "java.lang.Object(kotlin.sequences.Sequence,int)", "java.lang.String"),
          (
            "seq.elementAtOrNull(1)",
            "elementAtOrNull",
            "java.lang.Object(kotlin.sequences.Sequence,int)",
            "java.lang.String"
          ),
          (
            """seq.elementAtOrElse(1) { "fallback" }""",
            "elementAtOrElse",
            "java.lang.Object(kotlin.sequences.Sequence,int,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.find { it.isNotEmpty() }",
            "find",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.first { it.isNotEmpty() }",
            "first",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.firstOrNull { it.isNotEmpty() }",
            "firstOrNull",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.last { it.isNotEmpty() }",
            "last",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.lastOrNull { it.isNotEmpty() }",
            "lastOrNull",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.single { it.isNotEmpty() }",
            "single",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          (
            "seq.singleOrNull { it.isNotEmpty() }",
            "singleOrNull",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("seq.any()", "any", "boolean(kotlin.sequences.Sequence)", "boolean"),
          ("seq.any { it.isNotEmpty() }", "any", sequencePredicateSignature, "boolean"),
          ("seq.contains(\"x\")", "contains", "boolean(kotlin.sequences.Sequence,java.lang.Object)", "boolean"),
          (
            "seq.indexOfFirst { it.isNotEmpty() }",
            "indexOfFirst",
            "int(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "seq.indexOfLast { it.isNotEmpty() }",
            "indexOfLast",
            "int(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "int"
          ),
          ("seq.count()", "count", "int(kotlin.sequences.Sequence)", "int"),
          (
            "seq.count { it.isNotEmpty() }",
            "count",
            "int(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "int"
          ),
          ("seq.asIterable()", "asIterable", "java.lang.Iterable(kotlin.sequences.Sequence)", "java.lang.Iterable"),
          (
            "seq.constrainOnce()",
            "constrainOnce",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          ("seq.toList()", "toList", "java.util.List(kotlin.sequences.Sequence)", "java.util.List"),
          ("seq.toMutableList()", "toMutableList", "java.util.List(kotlin.sequences.Sequence)", "java.util.List"),
          ("seq.toSet()", "toSet", "java.util.Set(kotlin.sequences.Sequence)", "java.util.Set"),
          ("seq.toMutableSet()", "toMutableSet", "java.util.Set(kotlin.sequences.Sequence)", "java.util.Set"),
          ("seq.toHashSet()", "toHashSet", "java.util.HashSet(kotlin.sequences.Sequence)", "java.util.HashSet"),
          ("seq.toSortedSet()", "toSortedSet", "java.util.SortedSet(kotlin.sequences.Sequence)", "java.util.SortedSet"),
          (
            "seq.toSortedSet(comparator)",
            "toSortedSet",
            "java.util.SortedSet(kotlin.sequences.Sequence,java.util.Comparator)",
            "java.util.SortedSet"
          ),
          (
            "seq.toCollection(mutableListOf<String>())",
            "toCollection",
            "java.util.Collection(kotlin.sequences.Sequence,java.util.Collection)",
            "java.util.List"
          ),
          (
            "seq.joinToString()",
            "joinToString",
            "java.lang.String(kotlin.sequences.Sequence,java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.String"
          ),
          ("seq.onEach { println(it) }", "onEach", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          (
            "seq.onEachIndexed { index, item -> println(item + index.toString()) }",
            "onEachIndexed",
            sequenceFunction2Signature,
            "kotlin.sequences.Sequence"
          ),
          (
            "seq.withIndex()",
            "withIndex",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          (
            "seq.forEach { println(it) }",
            "forEach",
            "void(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "void"
          ),
          (
            "seq.forEachIndexed { index, item -> println(item + index.toString()) }",
            "forEachIndexed",
            "void(kotlin.sequences.Sequence,kotlin.jvm.functions.Function2)",
            "void"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.sequences.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "seq",
            "filtered",
            "filteredNot",
            "filteredIndexed",
            "mapped",
            "mappedIndexed",
            "mappedNotNull",
            "flatMapped",
            "flatMappedIndexed",
            "taken",
            "dropped",
            "takenWhile",
            "droppedWhile",
            "constrained",
            "each",
            "eachIndexed",
            "indexed"
          )
          .typeFullName
          .l shouldBe List.fill(17)("kotlin.sequences.Sequence")
        cpg.local
          .nameExact(
            "first",
            "firstOrNull",
            "element",
            "elementOrNull",
            "elementOrElse",
            "found",
            "firstMatch",
            "firstOrNullMatch",
            "lastMatch",
            "lastOrNullMatch",
            "singleMatch",
            "singleOrNullMatch"
          )
          .typeFullName
          .l shouldBe List.fill(12)("java.lang.String")
        cpg.local.nameExact("anyPlain", "anyMatch", "hasValue").typeFullName.l shouldBe List.fill(3)("boolean")
        cpg.local.nameExact("indexFirst", "indexLast", "countPlain", "countMatch").typeFullName.l shouldBe
          List.fill(4)("int")
        cpg.local.nameExact("iterableView").typeFullName.l shouldBe List("java.lang.Iterable")
        cpg.local.nameExact("listed", "mutableListed", "collected").typeFullName.l shouldBe List.fill(3)(
          "java.util.List"
        )
        cpg.local.nameExact("set", "mutableSet").typeFullName.l shouldBe List.fill(2)("java.util.Set")
        cpg.local.nameExact("hashSet").typeFullName.l shouldBe List("java.util.HashSet")
        cpg.local.nameExact("sortedSet", "sortedSetWithComparator").typeFullName.l shouldBe
          List.fill(2)("java.util.SortedSet")
        cpg.local.nameExact("joined").typeFullName.l shouldBe List("java.lang.String")
      }
    }

    "resolve sequence lambda-result extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun sequenceLambdaResults(values: Sequence<String>) {
          |  val minLength = values.minOf { it.length }
          |  val maxLength = values.maxOf { it.length }
          |  val minLengthOrNull = values.minOfOrNull { it.length }
          |  val maxLengthOrNull = values.maxOfOrNull { it.length }
          |  val minText = values.minOf { it }
          |  val maxText = values.maxOf { it }
          |  val intTotal = values.sumOf { it.length }
          |  val longTotal = values.sumOf { it.length.toLong() }
          |  val doubleTotal = values.sumOf { it.length.toDouble() }
          |  println(minLength)
          |  println(maxLength)
          |  println(minLengthOrNull)
          |  println(maxLengthOrNull)
          |  println(minText)
          |  println(maxText)
          |  println(intTotal)
          |  println(longTotal)
          |  println(doubleTotal)
          |}
          |""".stripMargin) { cpg =>
        val comparableSignature = "java.lang.Comparable(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)"

        List(
          ("values.minOf { it.length }", "minOf", comparableSignature, "int"),
          ("values.maxOf { it.length }", "maxOf", comparableSignature, "int"),
          ("values.minOfOrNull { it.length }", "minOfOrNull", comparableSignature, "int"),
          ("values.maxOfOrNull { it.length }", "maxOfOrNull", comparableSignature, "int"),
          ("values.minOf { it }", "minOf", comparableSignature, "java.lang.String"),
          ("values.maxOf { it }", "maxOf", comparableSignature, "java.lang.String"),
          (
            "values.sumOf { it.length }",
            "sumOf",
            "int(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "int"
          ),
          (
            "values.sumOf { it.length.toLong() }",
            "sumOf",
            "long(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "long"
          ),
          (
            "values.sumOf { it.length.toDouble() }",
            "sumOf",
            "double(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "double"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.sequences.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact("minLength", "maxLength", "minLengthOrNull", "maxLengthOrNull", "intTotal")
          .typeFullName
          .l shouldBe List.fill(5)("int")
        cpg.local.nameExact("minText", "maxText").typeFullName.l shouldBe List.fill(2)("java.lang.String")
        cpg.local.nameExact("longTotal").typeFullName.l shouldBe List("long")
        cpg.local.nameExact("doubleTotal").typeFullName.l shouldBe List("double")
      }
    }

    "resolve sequence selector and ordering extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun sequenceSelectors(values: Sequence<String>, comparator: Comparator<String>) {
          |  val minValue = values.minOrNull()
          |  val maxValue = values.maxOrNull()
          |  val minBy = values.minBy { it.length }
          |  val maxBy = values.maxBy { it.length }
          |  val minByOrNull = values.minByOrNull { it.length }
          |  val maxByOrNull = values.maxByOrNull { it.length }
          |  val minWith = values.minWith(comparator)
          |  val maxWith = values.maxWith(comparator)
          |  val minWithOrNull = values.minWithOrNull(comparator)
          |  val maxWithOrNull = values.maxWithOrNull(comparator)
          |  val sorted = values.sorted()
          |  val sortedDescending = values.sortedDescending()
          |  val sortedBy = values.sortedBy { it.length }
          |  val sortedByDescending = values.sortedByDescending { it.length }
          |  val sortedWith = values.sortedWith(comparator)
          |  val distinct = values.distinct()
          |  val distinctBy = values.distinctBy { it.length }
          |  println(minValue)
          |  println(maxValue)
          |  println(minBy)
          |  println(maxBy)
          |  println(minByOrNull)
          |  println(maxByOrNull)
          |  println(minWith)
          |  println(maxWith)
          |  println(minWithOrNull)
          |  println(maxWithOrNull)
          |  println(sorted)
          |  println(sortedDescending)
          |  println(sortedBy)
          |  println(sortedByDescending)
          |  println(sortedWith)
          |  println(distinct)
          |  println(distinctBy)
          |}
          |""".stripMargin) { cpg =>
        val selectorSignature   = "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)"
        val comparatorSignature = "java.lang.Object(kotlin.sequences.Sequence,java.util.Comparator)"
        val sequenceFunction1Signature =
          "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)"

        List(
          ("values.minOrNull()", "minOrNull", "java.lang.Comparable(kotlin.sequences.Sequence)", "java.lang.String"),
          ("values.maxOrNull()", "maxOrNull", "java.lang.Comparable(kotlin.sequences.Sequence)", "java.lang.String"),
          ("values.minBy { it.length }", "minBy", selectorSignature, "java.lang.String"),
          ("values.maxBy { it.length }", "maxBy", selectorSignature, "java.lang.String"),
          ("values.minByOrNull { it.length }", "minByOrNull", selectorSignature, "java.lang.String"),
          ("values.maxByOrNull { it.length }", "maxByOrNull", selectorSignature, "java.lang.String"),
          ("values.minWith(comparator)", "minWith", comparatorSignature, "java.lang.String"),
          ("values.maxWith(comparator)", "maxWith", comparatorSignature, "java.lang.String"),
          ("values.minWithOrNull(comparator)", "minWithOrNull", comparatorSignature, "java.lang.String"),
          ("values.maxWithOrNull(comparator)", "maxWithOrNull", comparatorSignature, "java.lang.String"),
          (
            "values.sorted()",
            "sorted",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.sortedDescending()",
            "sortedDescending",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          ("values.sortedBy { it.length }", "sortedBy", sequenceFunction1Signature, "kotlin.sequences.Sequence"),
          (
            "values.sortedByDescending { it.length }",
            "sortedByDescending",
            sequenceFunction1Signature,
            "kotlin.sequences.Sequence"
          ),
          (
            "values.sortedWith(comparator)",
            "sortedWith",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.util.Comparator)",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.distinct()",
            "distinct",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          ("values.distinctBy { it.length }", "distinctBy", sequenceFunction1Signature, "kotlin.sequences.Sequence")
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.sequences.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "minValue",
            "maxValue",
            "minBy",
            "maxBy",
            "minByOrNull",
            "maxByOrNull",
            "minWith",
            "maxWith",
            "minWithOrNull",
            "maxWithOrNull"
          )
          .typeFullName
          .l shouldBe List.fill(10)("java.lang.String")
        cpg.local
          .nameExact(
            "sorted",
            "sortedDescending",
            "sortedBy",
            "sortedByDescending",
            "sortedWith",
            "distinct",
            "distinctBy"
          )
          .typeFullName
          .l shouldBe List.fill(7)("kotlin.sequences.Sequence")
      }
    }

    "resolve sequence pairing and grouping extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun sequenceGrouping(
          |  values: Sequence<String>,
          |  numbers: Sequence<Int>,
          |  pairs: Sequence<Pair<String, Int>>,
          |  nullableValues: Sequence<String?>,
          |  nested: Sequence<Sequence<String>>,
          |  moreValues: List<String>,
          |  moreArray: Array<String>,
          |  moreSequence: Sequence<String>
          |) {
          |  val zipped = values.zip(numbers)
          |  val zippedTransform = values.zip(numbers) { text, number -> text + number.toString() }
          |  val plusElement = values.plus("x")
          |  val plusList = values.plus(moreValues)
          |  val plusArray = values.plus(moreArray)
          |  val plusSequence = values.plus(moreSequence)
          |  val minusElement = values.minus("x")
          |  val minusList = values.minus(moreValues)
          |  val minusArray = values.minus(moreArray)
          |  val minusSequence = values.minus(moreSequence)
          |  val unzipped = pairs.unzip()
          |  val mapCopy = pairs.toMap()
          |  val mapCopyTo = pairs.toMap(mutableMapOf<String, Int>())
          |  val partitioned = values.partition { it.isNotEmpty() }
          |  val grouped = values.groupBy { it.length }
          |  val groupedValue = values.groupBy({ it.length }, { it })
          |  val valueGrouping = values.groupingBy { it.length }
          |  val associated = values.associate { it to it.length }
          |  val associatedBy = values.associateBy { it.length }
          |  val associatedByValue = values.associateBy({ it.length }, { it })
          |  val associatedWith = values.associateWith { it.length }
          |  val flattened = nested.flatten()
          |  val notNull = nullableValues.filterNotNull()
          |  val requiredNoNulls = nullableValues.requireNoNulls()
          |  println(zipped)
          |  println(zippedTransform)
          |  println(plusElement)
          |  println(plusList)
          |  println(plusArray)
          |  println(plusSequence)
          |  println(minusElement)
          |  println(minusList)
          |  println(minusArray)
          |  println(minusSequence)
          |  println(unzipped)
          |  println(mapCopy)
          |  println(mapCopyTo)
          |  println(partitioned)
          |  println(grouped)
          |  println(groupedValue)
          |  println(valueGrouping)
          |  println(associated)
          |  println(associatedBy)
          |  println(associatedByValue)
          |  println(associatedWith)
          |  println(flattened)
          |  println(notNull)
          |  println(requiredNoNulls)
          |}
          |""".stripMargin) { cpg =>
        List(
          (
            "values.zip(numbers)",
            "zip",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.zip(numbers) { text, number -> text + number.toString() }",
            "zip",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.sequences.Sequence,kotlin.jvm.functions.Function2)",
            "kotlin.sequences.Sequence"
          ),
          (
            """values.plus("x")""",
            "plus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object)",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.plus(moreValues)",
            "plus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Iterable)",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.plus(moreArray)",
            "plus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object[])",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.plus(moreSequence)",
            "plus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          (
            """values.minus("x")""",
            "minus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object)",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.minus(moreValues)",
            "minus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Iterable)",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.minus(moreArray)",
            "minus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object[])",
            "kotlin.sequences.Sequence"
          ),
          (
            "values.minus(moreSequence)",
            "minus",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          ("pairs.unzip()", "unzip", "kotlin.Pair(kotlin.sequences.Sequence)", "kotlin.Pair"),
          (
            "values.partition { it.isNotEmpty() }",
            "partition",
            "kotlin.Pair(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "kotlin.Pair"
          ),
          (
            "values.groupBy { it.length }",
            "groupBy",
            "java.util.Map(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.groupBy({ it.length }, { it })",
            "groupBy",
            "java.util.Map(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.groupingBy { it.length }",
            "groupingBy",
            "kotlin.collections.Grouping(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "kotlin.collections.Grouping"
          ),
          (
            "values.associate { it to it.length }",
            "associate",
            "java.util.Map(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateBy { it.length }",
            "associateBy",
            "java.util.Map(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateBy({ it.length }, { it })",
            "associateBy",
            "java.util.Map(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "values.associateWith { it.length }",
            "associateWith",
            "java.util.Map(kotlin.sequences.Sequence,kotlin.jvm.functions.Function1)",
            "java.util.Map"
          ),
          (
            "nested.flatten()",
            "flatten",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          (
            "nullableValues.filterNotNull()",
            "filterNotNull",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          ),
          (
            "nullableValues.requireNoNulls()",
            "requireNoNulls",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence)",
            "kotlin.sequences.Sequence"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.sequences.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        List(
          ("pairs.toMap()", "java.util.Map(kotlin.sequences.Sequence)"),
          ("pairs.toMap(mutableMapOf<String, Int>())", "java.util.Map(kotlin.sequences.Sequence,java.util.Map)")
        ).foreach { case (code, signature) =>
          val List(call) = cpg.call.nameExact("toMap").codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.collections.toMap:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.util.Map"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "zipped",
            "zippedTransform",
            "plusElement",
            "plusList",
            "plusArray",
            "plusSequence",
            "minusElement",
            "minusList",
            "minusArray",
            "minusSequence",
            "flattened",
            "notNull",
            "requiredNoNulls"
          )
          .typeFullName
          .l shouldBe List.fill(13)("kotlin.sequences.Sequence")
        cpg.local.nameExact("unzipped", "partitioned").typeFullName.l shouldBe List.fill(2)("kotlin.Pair")
        cpg.local
          .nameExact(
            "mapCopy",
            "mapCopyTo",
            "grouped",
            "groupedValue",
            "associated",
            "associatedBy",
            "associatedByValue",
            "associatedWith"
          )
          .typeFullName
          .l shouldBe List.fill(8)("java.util.Map")
        cpg.local.nameExact("valueGrouping").typeFullName.l shouldBe List("kotlin.collections.Grouping")
      }
    }

    "resolve sequence destination extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun sequenceDestinations(values: Sequence<String>, nullableValues: Sequence<String?>) {
          |  val filtered = values.filterTo(mutableListOf<String>()) { it.isNotEmpty() }
          |  val filteredNot = values.filterNotTo(mutableListOf<String>()) { it.isEmpty() }
          |  val filteredIndexed = values.filterIndexedTo(mutableListOf<String>()) { index, item -> index > 0 && item.isNotEmpty() }
          |  val filteredNotNull = nullableValues.filterNotNullTo(mutableListOf<String>())
          |  val mapped = values.mapTo(mutableListOf<Int>()) { it.length }
          |  val mappedIndexed = values.mapIndexedTo(mutableListOf<Int>()) { index, item -> index + item.length }
          |  val mappedNotNull = values.mapNotNullTo(mutableListOf<Int>()) { it.length }
          |  val mappedIndexedNotNull = values.mapIndexedNotNullTo(mutableListOf<Int>()) { index, item -> index + item.length }
          |  val flatMapped = values.flatMapTo(mutableListOf<String>()) { listOf(it, it.uppercase()) }
          |  val flatMappedIndexed = values.flatMapIndexedTo(mutableListOf<String>()) { index, item -> listOf(item + index.toString()) }
          |  val grouped = values.groupByTo(mutableMapOf<Int, MutableList<String>>()) { it.length }
          |  val groupedValue = values.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it.length }, { it })
          |  val associated = values.associateTo(mutableMapOf<String, Int>()) { it to it.length }
          |  val associatedBy = values.associateByTo(mutableMapOf<Int, String>()) { it.length }
          |  val associatedByValue = values.associateByTo(mutableMapOf<Int, String>(), { it.length }, { it })
          |  val associatedWith = values.associateWithTo(mutableMapOf<String, Int>()) { it.length }
          |  println(filtered)
          |  println(filteredNot)
          |  println(filteredIndexed)
          |  println(filteredNotNull)
          |  println(mapped)
          |  println(mappedIndexed)
          |  println(mappedNotNull)
          |  println(mappedIndexedNotNull)
          |  println(flatMapped)
          |  println(flatMappedIndexed)
          |  println(grouped)
          |  println(groupedValue)
          |  println(associated)
          |  println(associatedBy)
          |  println(associatedByValue)
          |  println(associatedWith)
          |}
          |""".stripMargin) { cpg =>
        val collectionFunction1Signature =
          "java.util.Collection(kotlin.sequences.Sequence,java.util.Collection,kotlin.jvm.functions.Function1)"
        val collectionFunction2Signature =
          "java.util.Collection(kotlin.sequences.Sequence,java.util.Collection,kotlin.jvm.functions.Function2)"
        val mapFunction1Signature =
          "java.util.Map(kotlin.sequences.Sequence,java.util.Map,kotlin.jvm.functions.Function1)"
        val mapFunction2Signature =
          "java.util.Map(kotlin.sequences.Sequence,java.util.Map,kotlin.jvm.functions.Function1,kotlin.jvm.functions.Function1)"

        List(
          (
            "values.filterTo(mutableListOf<String>()) { it.isNotEmpty() }",
            "filterTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.filterNotTo(mutableListOf<String>()) { it.isEmpty() }",
            "filterNotTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.filterIndexedTo(mutableListOf<String>()) { index, item -> index > 0 && item.isNotEmpty() }",
            "filterIndexedTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "nullableValues.filterNotNullTo(mutableListOf<String>())",
            "filterNotNullTo",
            "java.util.Collection(kotlin.sequences.Sequence,java.util.Collection)",
            "java.util.List"
          ),
          ("values.mapTo(mutableListOf<Int>()) { it.length }", "mapTo", collectionFunction1Signature, "java.util.List"),
          (
            "values.mapIndexedTo(mutableListOf<Int>()) { index, item -> index + item.length }",
            "mapIndexedTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "values.mapNotNullTo(mutableListOf<Int>()) { it.length }",
            "mapNotNullTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.mapIndexedNotNullTo(mutableListOf<Int>()) { index, item -> index + item.length }",
            "mapIndexedNotNullTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "values.flatMapTo(mutableListOf<String>()) { listOf(it, it.uppercase()) }",
            "flatMapTo",
            collectionFunction1Signature,
            "java.util.List"
          ),
          (
            "values.flatMapIndexedTo(mutableListOf<String>()) { index, item -> listOf(item + index.toString()) }",
            "flatMapIndexedTo",
            collectionFunction2Signature,
            "java.util.List"
          ),
          (
            "values.groupByTo(mutableMapOf<Int, MutableList<String>>()) { it.length }",
            "groupByTo",
            mapFunction1Signature,
            "java.util.Map"
          ),
          (
            "values.groupByTo(mutableMapOf<Int, MutableList<String>>(), { it.length }, { it })",
            "groupByTo",
            mapFunction2Signature,
            "java.util.Map"
          ),
          (
            "values.associateTo(mutableMapOf<String, Int>()) { it to it.length }",
            "associateTo",
            mapFunction1Signature,
            "java.util.Map"
          ),
          (
            "values.associateByTo(mutableMapOf<Int, String>()) { it.length }",
            "associateByTo",
            mapFunction1Signature,
            "java.util.Map"
          ),
          (
            "values.associateByTo(mutableMapOf<Int, String>(), { it.length }, { it })",
            "associateByTo",
            mapFunction2Signature,
            "java.util.Map"
          ),
          (
            "values.associateWithTo(mutableMapOf<String, Int>()) { it.length }",
            "associateWithTo",
            mapFunction1Signature,
            "java.util.Map"
          )
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.sequences.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "filtered",
            "filteredNot",
            "filteredIndexed",
            "filteredNotNull",
            "mapped",
            "mappedIndexed",
            "mappedNotNull",
            "mappedIndexedNotNull",
            "flatMapped",
            "flatMappedIndexed"
          )
          .typeFullName
          .l shouldBe List.fill(10)("java.util.List")
        cpg.local
          .nameExact("grouped", "groupedValue", "associated", "associatedBy", "associatedByValue", "associatedWith")
          .typeFullName
          .l shouldBe List.fill(6)("java.util.Map")
      }
    }

    "resolve sequence aggregation and accumulation extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun sequenceAggregations(values: Sequence<Int>) {
          |  val joined = values.joinToString()
          |  val joinedSep = values.joinToString(separator = "|")
          |  val joinedTo = values.joinTo(StringBuilder())
          |  val folded = values.fold(0) { acc, item -> acc + item }
          |  val foldedIndexed = values.foldIndexed(0) { index, acc, item -> acc + item + index }
          |  val reduced = values.reduce { acc, item -> acc + item }
          |  val reducedIndexed = values.reduceIndexed { index, acc, item -> acc + item + index }
          |  val reducedOrNull = values.reduceOrNull { acc, item -> acc + item }
          |  val reducedIndexedOrNull = values.reduceIndexedOrNull { index, acc, item -> acc + item + index }
          |  val total = values.sum()
          |  val average = values.average()
          |  println(joined)
          |  println(joinedSep)
          |  println(joinedTo)
          |  println(folded)
          |  println(foldedIndexed)
          |  println(reduced)
          |  println(reducedIndexed)
          |  println(reducedOrNull)
          |  println(reducedIndexedOrNull)
          |  println(total)
          |  println(average)
          |}
          |""".stripMargin) { cpg =>
        val joinToStringSignature =
          "java.lang.String(kotlin.sequences.Sequence,java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)"
        List(
          ("values.joinToString()", "joinToString", joinToStringSignature, "java.lang.String"),
          ("""values.joinToString(separator = "|")""", "joinToString", joinToStringSignature, "java.lang.String"),
          (
            "values.joinTo(StringBuilder())",
            "joinTo",
            "java.lang.Appendable(kotlin.sequences.Sequence,java.lang.Appendable,java.lang.CharSequence,java.lang.CharSequence,java.lang.CharSequence,int,java.lang.CharSequence,kotlin.jvm.functions.Function1)",
            "java.lang.StringBuilder"
          ),
          (
            "values.fold(0) { acc, item -> acc + item }",
            "fold",
            "java.lang.Object(kotlin.sequences.Sequence,java.lang.Object,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.foldIndexed(0) { index, acc, item -> acc + item + index }",
            "foldIndexed",
            "java.lang.Object(kotlin.sequences.Sequence,java.lang.Object,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.reduce { acc, item -> acc + item }",
            "reduce",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.reduceIndexed { index, acc, item -> acc + item + index }",
            "reduceIndexed",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function3)",
            "int"
          ),
          (
            "values.reduceOrNull { acc, item -> acc + item }",
            "reduceOrNull",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function2)",
            "int"
          ),
          (
            "values.reduceIndexedOrNull { index, acc, item -> acc + item + index }",
            "reduceIndexedOrNull",
            "java.lang.Object(kotlin.sequences.Sequence,kotlin.jvm.functions.Function3)",
            "int"
          ),
          ("values.sum()", "sum", "int(kotlin.sequences.Sequence)", "int"),
          ("values.average()", "average", "double(kotlin.sequences.Sequence)", "double")
        ).foreach { case (code, name, signature, typeFullName) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.sequences.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe typeFullName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local.nameExact("joined", "joinedSep").typeFullName.l shouldBe List.fill(2)("java.lang.String")
        cpg.local.nameExact("joinedTo").typeFullName.l shouldBe List("java.lang.StringBuilder")
        cpg.local
          .nameExact(
            "folded",
            "foldedIndexed",
            "reduced",
            "reducedIndexed",
            "reducedOrNull",
            "reducedIndexedOrNull",
            "total"
          )
          .typeFullName
          .l shouldBe List.fill(7)("int")
        cpg.local.nameExact("average").typeFullName.l shouldBe List("double")
      }
    }

    "resolve sequence windowing and running extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun sequenceWindowing(values: Sequence<String>, ints: Sequence<Int>) {
          |  val chunked = values.chunked(2)
          |  val chunkedTransform = values.chunked(2) { it.joinToString() }
          |  val windowed = values.windowed(2)
          |  val windowedStep = values.windowed(2, 1)
          |  val windowedPartial = values.windowed(2, 1, true)
          |  val windowedTransform = values.windowed(2, 1, true) { it.joinToString() }
          |  val zippedNext = values.zipWithNext()
          |  val zippedNextTransform = values.zipWithNext { left, right -> left + right }
          |  val runningFold = ints.runningFold(0) { acc, item -> acc + item }
          |  val runningFoldIndexed = ints.runningFoldIndexed(0) { index, acc, item -> acc + item + index }
          |  val runningReduce = ints.runningReduce { acc, item -> acc + item }
          |  val runningReduceIndexed = ints.runningReduceIndexed { index, acc, item -> acc + item + index }
          |  val scan = ints.scan(0) { acc, item -> acc + item }
          |  val scanIndexed = ints.scanIndexed(0) { index, acc, item -> acc + item + index }
          |}
          |""".stripMargin) { cpg =>
        val List(sequenceWindowing) = cpg.method.nameExact("sequenceWindowing").l: @unchecked

        List(
          ("values.chunked(2)", "chunked", "kotlin.sequences.Sequence(kotlin.sequences.Sequence,int)"),
          (
            "values.chunked(2) { it.joinToString() }",
            "chunked",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,int,kotlin.jvm.functions.Function1)"
          ),
          ("values.windowed(2)", "windowed", "kotlin.sequences.Sequence(kotlin.sequences.Sequence,int,int,boolean)"),
          ("values.windowed(2, 1)", "windowed", "kotlin.sequences.Sequence(kotlin.sequences.Sequence,int,int,boolean)"),
          (
            "values.windowed(2, 1, true)",
            "windowed",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,int,int,boolean)"
          ),
          (
            "values.windowed(2, 1, true) { it.joinToString() }",
            "windowed",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,int,int,boolean,kotlin.jvm.functions.Function1)"
          ),
          ("values.zipWithNext()", "zipWithNext", "kotlin.sequences.Sequence(kotlin.sequences.Sequence)"),
          (
            "values.zipWithNext { left, right -> left + right }",
            "zipWithNext",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.runningFold(0) { acc, item -> acc + item }",
            "runningFold",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.runningFoldIndexed(0) { index, acc, item -> acc + item + index }",
            "runningFoldIndexed",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object,kotlin.jvm.functions.Function3)"
          ),
          (
            "ints.runningReduce { acc, item -> acc + item }",
            "runningReduce",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.runningReduceIndexed { index, acc, item -> acc + item + index }",
            "runningReduceIndexed",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,kotlin.jvm.functions.Function3)"
          ),
          (
            "ints.scan(0) { acc, item -> acc + item }",
            "scan",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object,kotlin.jvm.functions.Function2)"
          ),
          (
            "ints.scanIndexed(0) { index, acc, item -> acc + item + index }",
            "scanIndexed",
            "kotlin.sequences.Sequence(kotlin.sequences.Sequence,java.lang.Object,kotlin.jvm.functions.Function3)"
          )
        ).foreach { case (code, name, signature) =>
          val List(call) = sequenceWindowing.ast.isCall.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.sequences.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "kotlin.sequences.Sequence"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        sequenceWindowing.ast.isLocal.filterNot(_.name.startsWith("tmp_")).typeFullName.l.distinct shouldBe
          List("kotlin.sequences.Sequence")
      }
    }

    "lower named call arguments" in {
      withOxidizedCpg("""package demo
          |
          |interface Producer {
          |  fun value(): String
          |}
          |
          |class Box(val value: String)
          |
          |fun target(one: Any, two: Any): Any = one
          |fun make(): String = "made"
          |
          |fun callers(flag: Boolean) {
          |  target(two = "literal", one = make())
          |  target(two = "ctor", one = Box("that").value)
          |  target(two = "if", one = if (flag) "that" else "other")
          |  target(two = "try", one = try { "that" } catch (e: Exception) { "other" })
          |  target(two = "when", one = when (flag) { true -> "that" else -> "other" })
          |  target(two = "paren", one = ("that"))
          |  target(two = "binary", one = 1 * 2)
          |  target(two = "cast", one = "that" as String)
          |  target(
          |    two = "object",
          |    one = object : Producer {
          |      override fun value(): String {
          |        return "that"
          |      }
          |    }
          |  )
          |  target(two = "lambda", one = { "that" })
          |  target(two = "anonymous", one = fun(): String { return "that" })
          |}
          |""".stripMargin) { cpg =>
        val targetCalls = cpg.call.nameExact("target").l
        targetCalls.size.shouldBe(11)
        targetCalls.foreach { call =>
          call.argument.argumentName.l.shouldBe(List("two", "one"))
        }
        targetCalls.flatMap(_.argument.argumentName("one").isCall.nameExact("make").l).size.shouldBe(1)
        targetCalls
          .flatMap(
            _.argument.argumentName("one").isCall.nameExact(Operators.fieldAccess).codeExact("Box(\"that\").value").l
          )
          .size
          .shouldBe(1)
        targetCalls
          .flatMap(_.argument.argumentName("one").isCall.nameExact(Operators.multiplication).l)
          .size
          .shouldBe(1)
        targetCalls.flatMap(_.argument.argumentName("one").isCall.nameExact(Operators.conditional).l).size.shouldBe(1)
        cpg.all.collectAll[Unknown].codeExact("""if (flag) "that" else "other"""").l shouldBe empty
        val List(whenCall) =
          targetCalls.flatMap(_.argument.argumentName("one").isCall.nameExact("<operator>.when").l): @unchecked
        whenCall.argument.isBlock.size shouldBe 3
        cpg.all.collectAll[Unknown].codeExact("""when (flag) { true -> "that" else -> "other" }""").l shouldBe empty
        targetCalls.flatMap(_.argument.argumentName("one").isCall.nameExact(Operators.cast).l).size.shouldBe(1)
        targetCalls.flatMap(_.argument.argumentName("one").isBlock.code("object.*").l).size.shouldBe(1)
        targetCalls.flatMap(_.argument.argumentName("one").isMethodRef.l).size.shouldBe(2)
      }
    }

    "lower when expressions used as navigation receivers" in {
      withOxidizedCpg("""package demo
          |
          |fun main(flag: Boolean) {
          |  val rendered = when {
          |    flag -> true
          |    else -> false
          |  }.toString()
          |  val numeric = 1.toString()
          |  println(rendered)
          |  println(numeric)
          |}
          |""".stripMargin) { cpg =>
        val List(toStringCall) = cpg.call.nameExact("toString").code("when.*\\.toString\\(\\)").l: @unchecked
        toStringCall.methodFullName shouldBe "kotlin.Boolean.toString:java.lang.String()"
        toStringCall.typeFullName shouldBe "java.lang.String"
        toStringCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        cpg.local.nameExact("rendered").typeFullName.l shouldBe List("java.lang.String")
        cpg.call.nameExact("toString").codeExact("1.toString()").methodFullName.l shouldBe List(
          "kotlin.Int.toString:java.lang.String()"
        )
        cpg.all.collectAll[Unknown].code("when.*\\.toString\\(\\)").l shouldBe empty
      }
    }

    "resolve parenthesized primitive receiver conversion calls" in {
      withOxidizedCpg("""package demo
          |
          |fun main() {
          |  val x = 100
          |  val y = 200
          |  val z = (if (x / 20 < 3) 3 else y / 20).toFloat()
          |  println(z)
          |}
          |""".stripMargin) { cpg =>
        val List(toFloatCall) = cpg.call.nameExact("toFloat").l: @unchecked
        toFloatCall.code shouldBe "(if (x / 20 < 3) 3 else y / 20).toFloat()"
        toFloatCall.methodFullName shouldBe "kotlin.Int.toFloat:float()"
        toFloatCall.signature shouldBe "float()"
        toFloatCall.typeFullName shouldBe "float"
        toFloatCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH

        cpg.local.nameExact("z").typeFullName.l shouldBe List("float")
        cpg.call.nameExact(Operators.division).typeFullName.l.distinct shouldBe List("int")
        cpg.all.collectAll[Unknown].code("\\(if .*\\.toFloat\\(\\)").l shouldBe empty
      }
    }

    "resolve parenthesized string receiver case conversion calls" in {
      withOxidizedCpg("""package demo
          |
          |fun main() {
          |  val lower = ("A" + "B").toLowerCase()
          |  val upper = ("a" + "b").uppercase()
          |  println(lower)
          |  println(upper)
          |}
          |""".stripMargin) { cpg =>
        val List(lowerCall) = cpg.call.nameExact("toLowerCase").l: @unchecked
        lowerCall.code shouldBe """("A" + "B").toLowerCase()"""
        lowerCall.methodFullName shouldBe "kotlin.text.toLowerCase:java.lang.String(java.lang.String)"
        lowerCall.signature shouldBe "java.lang.String(java.lang.String)"
        lowerCall.typeFullName shouldBe "java.lang.String"
        lowerCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        lowerCall.argument.isCall.nameExact(Operators.addition).codeExact(""""A" + "B"""").typeFullName.l shouldBe List(
          "java.lang.String"
        )

        val List(upperCall) = cpg.call.nameExact("uppercase").l: @unchecked
        upperCall.methodFullName shouldBe "kotlin.text.uppercase:java.lang.String(java.lang.String)"
        upperCall.signature shouldBe "java.lang.String(java.lang.String)"
        upperCall.typeFullName shouldBe "java.lang.String"
        upperCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        cpg.local.nameExact("lower").typeFullName.l shouldBe List("java.lang.String")
        cpg.local.nameExact("upper").typeFullName.l shouldBe List("java.lang.String")
      }
    }

    "resolve Kotlin text predicate and string transformation calls" in {
      withOxidizedCpg("""package demo
          |
          |fun textOps(name: String): String {
          |  val empty = name.isEmpty()
          |  val starts = (name + "_tail").startsWith("pre")
          |  val startsAt = (name + "_tail").startsWith("e", 1)
          |  val ends = ("prefix_" + name).endsWith(name)
          |  val blank = name.isBlank()
          |  val nonEmpty = name.isNotEmpty()
          |  val nonBlank = name.isNotBlank()
          |  val containsText = name.contains("needle", ignoreCase = true)
          |  val containsChar = name.contains('x')
          |  val suffix = (name + "_tail").substring(1)
          |  val segment = (name + "_tail").substring(1, 3)
          |  val replacedText = name.replace("old", "new", ignoreCase = true)
          |  val replacedChar = name.replace('a', 'b')
          |  println(empty)
          |  println(starts)
          |  println(startsAt)
          |  println(ends)
          |  println(blank)
          |  println(nonEmpty)
          |  println(nonBlank)
          |  println(containsText)
          |  println(containsChar)
          |  println(segment)
          |  println(replacedText)
          |  println(replacedChar)
          |  return suffix
          |}
          |""".stripMargin) { cpg =>
        val List(emptyCall) = cpg.call.nameExact("isEmpty").l: @unchecked
        emptyCall.methodFullName shouldBe "kotlin.text.isEmpty:boolean(java.lang.CharSequence)"
        emptyCall.signature shouldBe "boolean(java.lang.CharSequence)"
        emptyCall.typeFullName shouldBe "boolean"
        emptyCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(startsCall) =
          cpg.call.nameExact("startsWith").codeExact("""(name + "_tail").startsWith("pre")""").l: @unchecked
        startsCall.methodFullName shouldBe "kotlin.text.startsWith:boolean(java.lang.String,java.lang.String,boolean)"
        startsCall.signature shouldBe "boolean(java.lang.String,java.lang.String,boolean)"
        startsCall.typeFullName shouldBe "boolean"
        startsCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(startsAtCall) =
          cpg.call.nameExact("startsWith").codeExact("""(name + "_tail").startsWith("e", 1)""").l: @unchecked
        startsAtCall.methodFullName shouldBe "kotlin.text.startsWith:boolean(java.lang.String,java.lang.String,int,boolean)"
        startsAtCall.signature shouldBe "boolean(java.lang.String,java.lang.String,int,boolean)"
        startsAtCall.typeFullName shouldBe "boolean"
        startsAtCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(endsCall) =
          cpg.call.nameExact("endsWith").codeExact("""("prefix_" + name).endsWith(name)""").l: @unchecked
        endsCall.methodFullName shouldBe "kotlin.text.endsWith:boolean(java.lang.String,java.lang.String,boolean)"
        endsCall.signature shouldBe "boolean(java.lang.String,java.lang.String,boolean)"
        endsCall.typeFullName shouldBe "boolean"
        endsCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(suffixCall) =
          cpg.call.nameExact("substring").codeExact("""(name + "_tail").substring(1)""").l: @unchecked
        suffixCall.methodFullName shouldBe "kotlin.text.substring:java.lang.String(java.lang.String,int)"
        suffixCall.signature shouldBe "java.lang.String(java.lang.String,int)"
        suffixCall.typeFullName shouldBe "java.lang.String"
        suffixCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(segmentCall) =
          cpg.call.nameExact("substring").codeExact("""(name + "_tail").substring(1, 3)""").l: @unchecked
        segmentCall.methodFullName shouldBe "kotlin.text.substring:java.lang.String(java.lang.String,int,int)"
        segmentCall.signature shouldBe "java.lang.String(java.lang.String,int,int)"
        segmentCall.typeFullName shouldBe "java.lang.String"
        segmentCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(containsTextCall) =
          cpg.call.nameExact("contains").codeExact("""name.contains("needle", ignoreCase = true)""").l: @unchecked
        containsTextCall.methodFullName shouldBe "kotlin.text.contains:boolean(java.lang.CharSequence,java.lang.CharSequence,boolean)"
        containsTextCall.signature shouldBe "boolean(java.lang.CharSequence,java.lang.CharSequence,boolean)"
        containsTextCall.typeFullName shouldBe "boolean"
        containsTextCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(containsCharCall) = cpg.call.nameExact("contains").codeExact("name.contains('x')").l: @unchecked
        containsCharCall.methodFullName shouldBe "kotlin.text.contains:boolean(java.lang.CharSequence,char,boolean)"
        containsCharCall.signature shouldBe "boolean(java.lang.CharSequence,char,boolean)"
        containsCharCall.typeFullName shouldBe "boolean"
        containsCharCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(replacedTextCall) =
          cpg.call.nameExact("replace").codeExact("""name.replace("old", "new", ignoreCase = true)""").l: @unchecked
        replacedTextCall.methodFullName shouldBe "kotlin.text.replace:java.lang.String(java.lang.String,java.lang.String,java.lang.String,boolean)"
        replacedTextCall.signature shouldBe "java.lang.String(java.lang.String,java.lang.String,java.lang.String,boolean)"
        replacedTextCall.typeFullName shouldBe "java.lang.String"
        replacedTextCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(replacedCharCall) = cpg.call.nameExact("replace").codeExact("name.replace('a', 'b')").l: @unchecked
        replacedCharCall.methodFullName shouldBe "kotlin.text.replace:java.lang.String(java.lang.String,char,char,boolean)"
        replacedCharCall.signature shouldBe "java.lang.String(java.lang.String,char,char,boolean)"
        replacedCharCall.typeFullName shouldBe "java.lang.String"
        replacedCharCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val extensionCalls = List(
          "isBlank"    -> "kotlin.text.isBlank:boolean(java.lang.CharSequence)",
          "isNotEmpty" -> "kotlin.text.isNotEmpty:boolean(java.lang.CharSequence)",
          "isNotBlank" -> "kotlin.text.isNotBlank:boolean(java.lang.CharSequence)"
        )
        extensionCalls.foreach { case (name, fullName) =>
          val List(call) = cpg.call.nameExact(name).l: @unchecked
          call.methodFullName shouldBe fullName
          call.signature shouldBe "boolean(java.lang.CharSequence)"
          call.typeFullName shouldBe "boolean"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          call.argument.isIdentifier.nameExact("name").refsTo.l shouldBe cpg.method
            .nameExact("textOps")
            .parameter
            .nameExact("name")
            .l
        }

        cpg.local
          .nameExact(
            "empty",
            "starts",
            "startsAt",
            "ends",
            "blank",
            "nonEmpty",
            "nonBlank",
            "containsText",
            "containsChar"
          )
          .typeFullName
          .l shouldBe
          List.fill(9)("boolean")
        cpg.local.nameExact("suffix", "segment", "replacedText", "replacedChar").typeFullName.l shouldBe
          List.fill(4)("java.lang.String")
      }
    }

    "resolve Kotlin text defaulting removal and delimiter calls" in {
      withOxidizedCpg("""package demo
          |
          |fun textTransforms(name: String) {
          |  val blanked = name.ifBlank { "fallback" }
          |  val emptied = name.ifEmpty { "fallback" }
          |  val noPrefix = name.removePrefix("pre")
          |  val noSuffix = name.removeSuffix("tail")
          |  val noSurround = name.removeSurrounding("[", "]")
          |  val noSurroundOne = name.removeSurrounding("\"")
          |  val before = name.substringBefore(":")
          |  val after = name.substringAfter(":")
          |  val beforeLast = name.substringBeforeLast(":")
          |  val afterLast = name.substringAfterLast(":")
          |  val beforeChar = name.substringBefore(':')
          |  val afterChar = name.substringAfter(':')
          |  val beforeLastChar = name.substringBeforeLast(':')
          |  val afterLastChar = name.substringAfterLast(':')
          |  println(blanked)
          |  println(emptied)
          |  println(noPrefix)
          |  println(noSuffix)
          |  println(noSurround)
          |  println(noSurroundOne)
          |  println(before)
          |  println(after)
          |  println(beforeLast)
          |  println(afterLast)
          |  println(beforeChar)
          |  println(afterChar)
          |  println(beforeLastChar)
          |  println(afterLastChar)
          |}
          |""".stripMargin) { cpg =>
        val defaultingSignature =
          "java.lang.Object(java.lang.CharSequence&java.lang.Object,kotlin.jvm.functions.Function0)"
        List("ifBlank", "ifEmpty").foreach { name =>
          val List(call) = cpg.call.nameExact(name).l: @unchecked
          call.methodFullName shouldBe s"kotlin.text.$name:$defaultingSignature"
          call.signature shouldBe defaultingSignature
          call.typeFullName shouldBe "java.lang.String"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          call.argument.isIdentifier.nameExact("name").refsTo.l shouldBe cpg.method
            .nameExact("textTransforms")
            .parameter
            .nameExact("name")
            .l
          call.argument.isMethodRef.size shouldBe 1
        }

        List("removePrefix", "removeSuffix").foreach { name =>
          val List(call) = cpg.call.nameExact(name).l: @unchecked
          val signature  = "java.lang.String(java.lang.String,java.lang.CharSequence)"
          call.methodFullName shouldBe s"kotlin.text.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.lang.String"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val List(removeSurroundingPair) =
          cpg.call.nameExact("removeSurrounding").codeExact("""name.removeSurrounding("[", "]")""").l: @unchecked
        removeSurroundingPair.methodFullName shouldBe
          "kotlin.text.removeSurrounding:java.lang.String(java.lang.String,java.lang.CharSequence,java.lang.CharSequence)"
        removeSurroundingPair.signature shouldBe
          "java.lang.String(java.lang.String,java.lang.CharSequence,java.lang.CharSequence)"
        removeSurroundingPair.typeFullName shouldBe "java.lang.String"
        removeSurroundingPair.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(removeSurroundingSingle) =
          cpg.call.nameExact("removeSurrounding").codeExact("""name.removeSurrounding("\"")""").l: @unchecked
        removeSurroundingSingle.methodFullName shouldBe
          "kotlin.text.removeSurrounding:java.lang.String(java.lang.String,java.lang.CharSequence)"
        removeSurroundingSingle.signature shouldBe "java.lang.String(java.lang.String,java.lang.CharSequence)"
        removeSurroundingSingle.typeFullName shouldBe "java.lang.String"
        removeSurroundingSingle.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val substringAroundExpectations = List(
          (
            """name.substringBefore(":")""",
            "substringBefore",
            "java.lang.String(java.lang.String,java.lang.String,java.lang.String)"
          ),
          (
            """name.substringAfter(":")""",
            "substringAfter",
            "java.lang.String(java.lang.String,java.lang.String,java.lang.String)"
          ),
          (
            """name.substringBeforeLast(":")""",
            "substringBeforeLast",
            "java.lang.String(java.lang.String,java.lang.String,java.lang.String)"
          ),
          (
            """name.substringAfterLast(":")""",
            "substringAfterLast",
            "java.lang.String(java.lang.String,java.lang.String,java.lang.String)"
          ),
          ("name.substringBefore(':')", "substringBefore", "java.lang.String(java.lang.String,char,java.lang.String)"),
          ("name.substringAfter(':')", "substringAfter", "java.lang.String(java.lang.String,char,java.lang.String)"),
          (
            "name.substringBeforeLast(':')",
            "substringBeforeLast",
            "java.lang.String(java.lang.String,char,java.lang.String)"
          ),
          (
            "name.substringAfterLast(':')",
            "substringAfterLast",
            "java.lang.String(java.lang.String,char,java.lang.String)"
          )
        )
        substringAroundExpectations.foreach { case (code, name, signature) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.text.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "java.lang.String"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "blanked",
            "emptied",
            "noPrefix",
            "noSuffix",
            "noSurround",
            "noSurroundOne",
            "before",
            "after",
            "beforeLast",
            "afterLast",
            "beforeChar",
            "afterChar",
            "beforeLastChar",
            "afterLastChar"
          )
          .typeFullName
          .l shouldBe List.fill(14)("java.lang.String")
      }
    }

    "resolve Kotlin text utility and padding calls" in {
      withOxidizedCpg("""package demo
          |
          |fun textUtilities(name: String) {
          |  val trimStartPlain = name.trimStart()
          |  val trimEndPlain = name.trimEnd()
          |  val trimStartChar = name.trimStart('x')
          |  val trimEndChar = name.trimEnd('x')
          |  val reversed = name.reversed()
          |  val splitLines = name.lines()
          |  val splitLineSequence = name.lineSequence()
          |  val leftPadded = name.padStart(8)
          |  val leftPaddedChar = name.padStart(8, '0')
          |  val rightPadded = name.padEnd(8)
          |  val rightPaddedChar = name.padEnd(8, '0')
          |  val firstReplacedText = name.replaceFirst("old", "new", ignoreCase = true)
          |  val firstReplacedChar = name.replaceFirst('a', 'b')
          |  println(trimStartPlain)
          |  println(trimEndPlain)
          |  println(trimStartChar)
          |  println(trimEndChar)
          |  println(reversed)
          |  println(splitLines)
          |  println(splitLineSequence)
          |  println(leftPadded)
          |  println(leftPaddedChar)
          |  println(rightPadded)
          |  println(rightPaddedChar)
          |  println(firstReplacedText)
          |  println(firstReplacedChar)
          |}
          |""".stripMargin) { cpg =>
        List("trimStart", "trimEnd").foreach { name =>
          val List(plainCall) = cpg.call.nameExact(name).codeExact(s"name.$name()").l: @unchecked
          val plainSignature  = "java.lang.String(java.lang.String)"
          plainCall.methodFullName shouldBe s"kotlin.text.$name:$plainSignature"
          plainCall.signature shouldBe plainSignature
          plainCall.typeFullName shouldBe "java.lang.String"
          plainCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

          val List(charCall) = cpg.call.nameExact(name).codeExact(s"name.$name('x')").l: @unchecked
          val charSignature  = "java.lang.String(java.lang.String,char[])"
          charCall.methodFullName shouldBe s"kotlin.text.$name:$charSignature"
          charCall.signature shouldBe charSignature
          charCall.typeFullName shouldBe "java.lang.String"
          charCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val List(reversedCall) = cpg.call.nameExact("reversed").l: @unchecked
        reversedCall.methodFullName shouldBe "kotlin.text.reversed:java.lang.String(java.lang.String)"
        reversedCall.signature shouldBe "java.lang.String(java.lang.String)"
        reversedCall.typeFullName shouldBe "java.lang.String"
        reversedCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(linesCall) = cpg.call.nameExact("lines").l: @unchecked
        linesCall.methodFullName shouldBe "kotlin.text.lines:java.util.List(java.lang.CharSequence)"
        linesCall.signature shouldBe "java.util.List(java.lang.CharSequence)"
        linesCall.typeFullName shouldBe "java.util.List"
        linesCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(lineSequenceCall) = cpg.call.nameExact("lineSequence").l: @unchecked
        lineSequenceCall.methodFullName shouldBe
          "kotlin.text.lineSequence:kotlin.sequences.Sequence(java.lang.CharSequence)"
        lineSequenceCall.signature shouldBe "kotlin.sequences.Sequence(java.lang.CharSequence)"
        lineSequenceCall.typeFullName shouldBe "kotlin.sequences.Sequence"
        lineSequenceCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        List("padStart", "padEnd").foreach { name =>
          val List(defaultCall) = cpg.call.nameExact(name).codeExact(s"name.$name(8)").l: @unchecked
          val signature         = "java.lang.String(java.lang.String,int,char)"
          defaultCall.methodFullName shouldBe s"kotlin.text.$name:$signature"
          defaultCall.signature shouldBe signature
          defaultCall.typeFullName shouldBe "java.lang.String"
          defaultCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

          val List(charCall) = cpg.call.nameExact(name).codeExact(s"name.$name(8, '0')").l: @unchecked
          charCall.methodFullName shouldBe s"kotlin.text.$name:$signature"
          charCall.signature shouldBe signature
          charCall.typeFullName shouldBe "java.lang.String"
          charCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        val List(firstReplacedTextCall) =
          cpg.call
            .nameExact("replaceFirst")
            .codeExact("""name.replaceFirst("old", "new", ignoreCase = true)""")
            .l: @unchecked
        firstReplacedTextCall.methodFullName shouldBe
          "kotlin.text.replaceFirst:java.lang.String(java.lang.String,java.lang.String,java.lang.String,boolean)"
        firstReplacedTextCall.signature shouldBe
          "java.lang.String(java.lang.String,java.lang.String,java.lang.String,boolean)"
        firstReplacedTextCall.typeFullName shouldBe "java.lang.String"
        firstReplacedTextCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(firstReplacedCharCall) =
          cpg.call.nameExact("replaceFirst").codeExact("name.replaceFirst('a', 'b')").l: @unchecked
        firstReplacedCharCall.methodFullName shouldBe
          "kotlin.text.replaceFirst:java.lang.String(java.lang.String,char,char,boolean)"
        firstReplacedCharCall.signature shouldBe "java.lang.String(java.lang.String,char,char,boolean)"
        firstReplacedCharCall.typeFullName shouldBe "java.lang.String"
        firstReplacedCharCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        cpg.local
          .nameExact(
            "trimStartPlain",
            "trimEndPlain",
            "trimStartChar",
            "trimEndChar",
            "reversed",
            "leftPadded",
            "leftPaddedChar",
            "rightPadded",
            "rightPaddedChar",
            "firstReplacedText",
            "firstReplacedChar"
          )
          .typeFullName
          .l shouldBe List.fill(11)("java.lang.String")
        cpg.local.nameExact("splitLines").typeFullName.l shouldBe List("java.util.List")
        cpg.local.nameExact("splitLineSequence").typeFullName.l shouldBe List("kotlin.sequences.Sequence")
      }
    }

    "resolve Kotlin text search calls" in {
      withOxidizedCpg("""package demo
          |
          |fun searchText(name: String) {
          |  val indexText = name.indexOf("needle")
          |  val indexChar = name.indexOf('x')
          |  val indexTextFrom = name.indexOf("needle", startIndex = 2)
          |  val indexCharFrom = name.indexOf('x', startIndex = 2)
          |  val indexTextIgnore = name.indexOf("needle", startIndex = 2, ignoreCase = true)
          |  val indexCharIgnore = name.indexOf('x', startIndex = 2, ignoreCase = true)
          |  val lastText = name.lastIndexOf("needle")
          |  val lastChar = name.lastIndexOf('x')
          |  val lastTextFrom = name.lastIndexOf("needle", startIndex = 2)
          |  val lastCharFrom = name.lastIndexOf('x', startIndex = 2)
          |  val lastTextIgnore = name.lastIndexOf("needle", startIndex = 2, ignoreCase = true)
          |  val lastCharIgnore = name.lastIndexOf('x', startIndex = 2, ignoreCase = true)
          |  println(indexText)
          |  println(indexChar)
          |  println(indexTextFrom)
          |  println(indexCharFrom)
          |  println(indexTextIgnore)
          |  println(indexCharIgnore)
          |  println(lastText)
          |  println(lastChar)
          |  println(lastTextFrom)
          |  println(lastCharFrom)
          |  println(lastTextIgnore)
          |  println(lastCharIgnore)
          |}
          |""".stripMargin) { cpg =>
        List(
          ("""name.indexOf("needle")""", "indexOf", "int(java.lang.CharSequence,java.lang.String,int,boolean)"),
          ("name.indexOf('x')", "indexOf", "int(java.lang.CharSequence,char,int,boolean)"),
          (
            """name.indexOf("needle", startIndex = 2)""",
            "indexOf",
            "int(java.lang.CharSequence,java.lang.String,int,boolean)"
          ),
          ("name.indexOf('x', startIndex = 2)", "indexOf", "int(java.lang.CharSequence,char,int,boolean)"),
          (
            """name.indexOf("needle", startIndex = 2, ignoreCase = true)""",
            "indexOf",
            "int(java.lang.CharSequence,java.lang.String,int,boolean)"
          ),
          (
            "name.indexOf('x', startIndex = 2, ignoreCase = true)",
            "indexOf",
            "int(java.lang.CharSequence,char,int,boolean)"
          ),
          ("""name.lastIndexOf("needle")""", "lastIndexOf", "int(java.lang.CharSequence,java.lang.String,int,boolean)"),
          ("name.lastIndexOf('x')", "lastIndexOf", "int(java.lang.CharSequence,char,int,boolean)"),
          (
            """name.lastIndexOf("needle", startIndex = 2)""",
            "lastIndexOf",
            "int(java.lang.CharSequence,java.lang.String,int,boolean)"
          ),
          ("name.lastIndexOf('x', startIndex = 2)", "lastIndexOf", "int(java.lang.CharSequence,char,int,boolean)"),
          (
            """name.lastIndexOf("needle", startIndex = 2, ignoreCase = true)""",
            "lastIndexOf",
            "int(java.lang.CharSequence,java.lang.String,int,boolean)"
          ),
          (
            "name.lastIndexOf('x', startIndex = 2, ignoreCase = true)",
            "lastIndexOf",
            "int(java.lang.CharSequence,char,int,boolean)"
          )
        ).foreach { case (code, name, signature) =>
          val List(call) = cpg.call.nameExact(name).codeExact(code).l: @unchecked
          call.methodFullName shouldBe s"kotlin.text.$name:$signature"
          call.signature shouldBe signature
          call.typeFullName shouldBe "int"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        }

        cpg.local
          .nameExact(
            "indexText",
            "indexChar",
            "indexTextFrom",
            "indexCharFrom",
            "indexTextIgnore",
            "indexCharIgnore",
            "lastText",
            "lastChar",
            "lastTextFrom",
            "lastCharFrom",
            "lastTextIgnore",
            "lastCharIgnore"
          )
          .typeFullName
          .l shouldBe List.fill(12)("int")
      }
    }

    "lower stdlib extension calls on string receivers" in {
      withOxidizedCpg("""package demo
          |
          |fun trimParam(p: String): String {
          |  val y = p.trim()
          |  val parts = p.split(",")
          |  val filtered = p.split(",", ":", ignoreCase = false)
          |  return y
          |}
          |""".stripMargin) { cpg =>
        val List(trimCall) = cpg.call.nameExact("trim").codeExact("p.trim()").l: @unchecked
        trimCall.methodFullName shouldBe "kotlin.text.trim:java.lang.String(java.lang.String)"
        trimCall.signature shouldBe "java.lang.String(java.lang.String)"
        trimCall.typeFullName shouldBe "java.lang.String"
        trimCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

        val List(receiverArg) = trimCall.argument.isIdentifier.nameExact("p").l: @unchecked
        receiverArg.argumentIndex shouldBe 1
        receiverArg.typeFullName shouldBe "java.lang.String"
        receiverArg.refsTo.l shouldBe cpg.method.nameExact("trimParam").parameter.nameExact("p").l
        cpg.local.nameExact("y").typeFullName.l shouldBe List("java.lang.String")

        val splitCalls = cpg.call.nameExact("split").l
        splitCalls.map(_.methodFullName).distinct shouldBe
          List("kotlin.text.split:java.util.List(java.lang.CharSequence,java.lang.String[],boolean,int)")
        splitCalls.map(_.signature).distinct shouldBe
          List("java.util.List(java.lang.CharSequence,java.lang.String[],boolean,int)")
        splitCalls.map(_.typeFullName).distinct shouldBe List("java.util.List")
        splitCalls.map(_.dispatchType).distinct shouldBe List(DispatchTypes.STATIC_DISPATCH)
        splitCalls
          .flatMap(_.argument.isIdentifier.nameExact("p").l.filter(_.argumentIndex == 1).flatMap(_.refsTo.l))
          .distinct shouldBe
          cpg.method.nameExact("trimParam").parameter.nameExact("p").l
        cpg.local.nameExact("parts").typeFullName.l shouldBe List("java.util.List")
        cpg.local.nameExact("filtered").typeFullName.l shouldBe List("java.util.List")
      }
    }

    "lower stdlib scope extension calls" in {
      withOxidizedCpg("""package demo
          |
          |fun scope(p: String) {
          |  val letValue = p.let { it }
          |  val alsoValue = p.also { println(it) }
          |  val applyValue = p.apply { println(p) }
          |  val runValue = p.run { p }
          |  val kept = p.takeIf { it != "" }
          |  val dropped = p.takeUnless { it == "" }
          |  println(letValue)
          |  println(alsoValue)
          |  println(applyValue)
          |  println(runValue)
          |  println(kept)
          |  println(dropped)
          |}
          |""".stripMargin) { cpg =>
        val List(letCall)        = cpg.call.nameExact("let").code("p\\.let.*").l: @unchecked
        val List(alsoCall)       = cpg.call.nameExact("also").code("p\\.also.*").l: @unchecked
        val List(applyCall)      = cpg.call.nameExact("apply").code("p\\.apply.*").l: @unchecked
        val List(runCall)        = cpg.call.nameExact("run").code("p\\.run.*").l: @unchecked
        val List(takeIfCall)     = cpg.call.nameExact("takeIf").code("p\\.takeIf.*").l: @unchecked
        val List(takeUnlessCall) = cpg.call.nameExact("takeUnless").code("p\\.takeUnless.*").l: @unchecked
        val scopeCalls           = List(letCall, alsoCall, applyCall, runCall, takeIfCall, takeUnlessCall)
        scopeCalls.foreach { call =>
          call.signature shouldBe "java.lang.Object(java.lang.Object,kotlin.jvm.functions.Function1)"
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH

          val List(receiverArg) = call.argument.isIdentifier.nameExact("p").l: @unchecked
          receiverArg.argumentIndex shouldBe 1
          receiverArg.refsTo.l shouldBe cpg.method.nameExact("scope").parameter.nameExact("p").l

          val List(lambdaArg) = call.argument.isMethodRef.l: @unchecked
          lambdaArg.argumentIndex shouldBe 2
          lambdaArg.methodFullName should include("<lambda>")
        }
        letCall.methodFullName shouldBe "kotlin.let:java.lang.Object(java.lang.Object,kotlin.jvm.functions.Function1)"
        alsoCall.methodFullName shouldBe "kotlin.also:java.lang.Object(java.lang.Object,kotlin.jvm.functions.Function1)"
        applyCall.methodFullName shouldBe "kotlin.apply:java.lang.Object(java.lang.Object,kotlin.jvm.functions.Function1)"
        runCall.methodFullName shouldBe "kotlin.run:java.lang.Object(java.lang.Object,kotlin.jvm.functions.Function1)"
        takeIfCall.methodFullName shouldBe "kotlin.takeIf:java.lang.Object(java.lang.Object,kotlin.jvm.functions.Function1)"
        takeUnlessCall.methodFullName shouldBe "kotlin.takeUnless:java.lang.Object(java.lang.Object,kotlin.jvm.functions.Function1)"
        letCall.typeFullName shouldBe "java.lang.Object"
        runCall.typeFullName shouldBe "java.lang.Object"
        List(alsoCall, applyCall, takeIfCall, takeUnlessCall).foreach(_.typeFullName shouldBe "java.lang.String")
        cpg.local.nameExact("alsoValue").typeFullName.l shouldBe List("java.lang.String")
        cpg.local.nameExact("applyValue").typeFullName.l shouldBe List("java.lang.String")
        cpg.local.nameExact("kept").typeFullName.l shouldBe List("java.lang.String")
        cpg.local.nameExact("dropped").typeFullName.l shouldBe List("java.lang.String")
      }
    }

    "lower this and super receivers" in {
      withOxidizedCpg("""package demo
          |
          |open class BClass {
          |  open fun myfun() {
          |    println("B.myfun")
          |  }
          |}
          |
          |class AClass : BClass() {
          |  fun passSelf() {
          |    target(this)
          |  }
          |
          |  override fun myfun() {
          |    passSelf()
          |    this.passSelf()
          |    super.myfun()
          |  }
          |}
          |
          |fun target(x: Any) {
          |  println(x)
          |}
          |""".stripMargin) { cpg =>
        val List(passSelf) = cpg.method.fullNameExact("demo.AClass.passSelf:void()").l: @unchecked
        val List(thisArg)  = cpg.call.nameExact("target").argument.isIdentifier.nameExact("this").l: @unchecked
        thisArg.typeFullName shouldBe "demo.AClass"
        thisArg.refsTo.l shouldBe passSelf.parameter.nameExact("this").l

        val List(implicitCall) = cpg.call.codeExact("passSelf()").l: @unchecked
        implicitCall.methodFullName shouldBe "demo.AClass.passSelf:void()"

        val List(explicitThisCall) = cpg.call.codeExact("this.passSelf()").l: @unchecked
        explicitThisCall.methodFullName shouldBe "demo.AClass.passSelf:void()"
        explicitThisCall.argument.isIdentifier.nameExact("this").typeFullName.l.shouldBe(List("demo.AClass"))

        val List(superCall) = cpg.call.codeExact("super.myfun()").l: @unchecked
        superCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        superCall.methodFullName shouldBe "demo.BClass.myfun:void()"
        superCall.signature shouldBe "void()"
        superCall.argument.isIdentifier.nameExact("super").typeFullName.l.shouldBe(List("demo.BClass"))
      }
    }

    "resolve inherited receiver method calls" in {
      withOxidizedCpg("""package demo
          |
          |open class Base {
          |  fun baseName(): String = "base"
          |  open fun overridden(): String = "base"
          |}
          |
          |class Child : Base() {
          |  override fun overridden(): String = "child"
          |  private fun secret(): String = "secret"
          |
          |  fun callBase(): String {
          |    return baseName()
          |  }
          |
          |  fun callSecret(): String {
          |    return secret()
          |  }
          |}
          |
          |class Holder {
          |  val child: Child = Child()
          |  val children: Array<Child> = arrayOf(Child())
          |}
          |
          |fun top(): String = "top"
          |
          |fun main(args: Array<String>) {
          |  val child = Child()
          |  child.baseName()
          |  child.overridden()
          |  val holder = Holder()
          |  holder.child.baseName()
          |  holder.children[0].baseName()
          |  top()
          |}
          |""".stripMargin) { cpg =>
        cpg.call.codeExact("child.baseName()").methodFullName.l shouldBe List("demo.Base.baseName:java.lang.String()")
        cpg.call.codeExact("child.baseName()").typeFullName.l shouldBe List("java.lang.String")
        cpg.call.codeExact("child.baseName()").dispatchType.l shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        cpg.call.codeExact("child.overridden()").methodFullName.l shouldBe List(
          "demo.Child.overridden:java.lang.String()"
        )
        cpg.call.codeExact("child.overridden()").dispatchType.l shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        cpg.call.codeExact("holder.child.baseName()").methodFullName.l shouldBe List(
          "demo.Base.baseName:java.lang.String()"
        )
        cpg.call.codeExact("holder.child.baseName()").dispatchType.l shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        cpg.call.codeExact("holder.children[0].baseName()").methodFullName.l shouldBe List(
          "demo.Base.baseName:java.lang.String()"
        )
        cpg.call.codeExact("holder.children[0].baseName()").dispatchType.l shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        cpg.call.codeExact("Child()").dispatchType.l should contain only DispatchTypes.STATIC_DISPATCH
        cpg.call.codeExact("top()").dispatchType.l shouldBe List(DispatchTypes.STATIC_DISPATCH)

        val List(callBase) = cpg.method.fullNameExact("demo.Child.callBase:java.lang.String()").l: @unchecked
        callBase.ast.isCall.codeExact("baseName()").methodFullName.l shouldBe List(
          "demo.Base.baseName:java.lang.String()"
        )
        callBase.ast.isCall.codeExact("baseName()").dispatchType.l shouldBe List(DispatchTypes.DYNAMIC_DISPATCH)
        callBase.ast.isCall
          .codeExact("baseName()")
          .receiver
          .isIdentifier
          .nameExact(Constants.ThisName)
          .typeFullName
          .l shouldBe
          List("demo.Child")
        callBase.ast.isCall
          .codeExact("baseName()")
          .argument
          .isIdentifier
          .nameExact(Constants.ThisName)
          .argumentIndex
          .l shouldBe List(0)

        val List(callSecret) = cpg.method.fullNameExact("demo.Child.callSecret:java.lang.String()").l: @unchecked
        callSecret.ast.isCall.codeExact("secret()").methodFullName.l shouldBe List(
          "demo.Child.secret:java.lang.String()"
        )
        callSecret.ast.isCall.codeExact("secret()").dispatchType.l shouldBe List(DispatchTypes.STATIC_DISPATCH)
        callSecret.ast.isCall.codeExact("secret()").receiver.isEmpty shouldBe true
        callSecret.ast.isCall
          .codeExact("secret()")
          .argument
          .isIdentifier
          .nameExact(Constants.ThisName)
          .argumentIndex
          .l shouldBe List(0)
      }
    }

    "lower unary operators" in {
      withOxidizedCpg("""package demo
          |
          |fun main(args: Array<String>) {
          |  var x: Int = 5
          |  val y: Boolean = true
          |  println(+x)
          |  println(-x)
          |  println(!y)
          |  ++x
          |  --x
          |  x++
          |  x--
          |  val neg = -x
          |  val flag = !y
          |}
          |""".stripMargin) { cpg =>
        val List(main) = cpg.method.fullNameExact("demo.main:void(java.lang.String[])").l: @unchecked
        val xLocal     = main.ast.isLocal.nameExact("x").head
        val yLocal     = main.ast.isLocal.nameExact("y").head

        val unaryExpectations = List(
          (Operators.plus, "+x", "int", "x"),
          (Operators.minus, "-x", "int", "x"),
          (Operators.logicalNot, "!y", "boolean", "y"),
          (Operators.preIncrement, "++x", "int", "x"),
          (Operators.preDecrement, "--x", "int", "x"),
          (Operators.postIncrement, "x++", "int", "x"),
          (Operators.postDecrement, "x--", "int", "x")
        )
        unaryExpectations.foreach { case (operatorName, code, typeFullName, argumentName) =>
          val calls = main.ast.isCall.nameExact(operatorName).codeExact(code).l
          calls.nonEmpty.shouldBe(true)
          calls.foreach { call =>
            call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
            call.methodFullName shouldBe operatorName
            call.typeFullName shouldBe typeFullName
            call.argument.size shouldBe 1
            val List(argument) = call.argument.isIdentifier.nameExact(argumentName).l: @unchecked
            argument.refsTo.l shouldBe (if (argumentName == "x") List(xLocal) else List(yLocal))
          }
        }
        main.ast.isLocal.nameExact("neg").typeFullName.l shouldBe List("int")
        main.ast.isLocal.nameExact("flag").typeFullName.l shouldBe List("boolean")
      }
    }

    "lower assignment operators" in {
      withOxidizedCpg("""package demo
          |
          |fun main(args: Array<String>) {
          |  var x: Int = 5
          |  x = 2
          |  x += 1
          |  x -= 1
          |  x *= 1
          |  x /= 1
          |  x %= 1
          |}
          |""".stripMargin) { cpg =>
        val List(main) = cpg.method.fullNameExact("demo.main:void(java.lang.String[])").l: @unchecked
        val xLocal     = main.ast.isLocal.nameExact("x").head

        val assignmentExpectations = List(
          (Operators.assignment, "x = 2", "2"),
          (Operators.assignmentPlus, "x += 1", "1"),
          (Operators.assignmentMinus, "x -= 1", "1"),
          (Operators.assignmentMultiplication, "x *= 1", "1"),
          (Operators.assignmentDivision, "x /= 1", "1"),
          (Operators.assignmentModulo, "x %= 1", "1")
        )
        assignmentExpectations.foreach { case (operatorName, code, rhsCode) =>
          val List(call) = main.ast.isCall.nameExact(operatorName).codeExact(code).l: @unchecked
          call.methodFullName shouldBe operatorName
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          call.argument.size shouldBe 2
          call.argument.isIdentifier.nameExact("x").refsTo.l shouldBe List(xLocal)
          call.argument.isLiteral.codeExact(rhsCode).size shouldBe 1
        }
        main.ast.isCall.nameExact(Operators.assignment).codeExact("var x: Int = 5").size shouldBe 1
      }
    }

    "lower field access member types" in {
      withOxidizedCpg("""package demo
          |
          |class Box(val value: String)
          |
          |open class Base {
          |  val inherited: String = "base"
          |}
          |
          |class Child : Base()
          |
          |class AClass {
          |  var m = "PLACEHOLDER"
          |  val n: Int = 1
          |}
          |
          |fun main(args: Array<String>) {
          |  val a = AClass()
          |  a.m = "VALUE"
          |  a.m += "!"
          |  println(a.m)
          |  println(a.n)
          |  val xs = arrayOf(1, 2, 3)
          |  xs[0] = 2
          |  xs[1] += 3
          |  val box = Box("x")
          |  println(box.value)
          |  val child = Child()
          |  println(child.inherited)
          |}
          |""".stripMargin) { cpg =>
        val List(main) = cpg.method.fullNameExact("demo.main:void(java.lang.String[])").l: @unchecked
        val aLocal     = main.ast.isLocal.nameExact("a").head
        val xsLocal    = main.ast.isLocal.nameExact("xs").head
        val boxLocal   = main.ast.isLocal.nameExact("box").head
        val childLocal = main.ast.isLocal.nameExact("child").head

        val List(memberAssignment) =
          main.ast.isCall.nameExact(Operators.assignment).codeExact("a.m = \"VALUE\"").l: @unchecked
        val List(assignmentTarget) = memberAssignment.argument.isCall.nameExact(Operators.fieldAccess).l: @unchecked
        assignmentTarget.argumentIndex shouldBe 1
        assignmentTarget.typeFullName shouldBe "java.lang.String"
        assignmentTarget.argument.isIdentifier.nameExact("a").refsTo.l shouldBe List(aLocal)
        assignmentTarget.argument.isFieldIdentifier.canonicalName.l shouldBe List("m")

        val List(memberCompoundAssignment) =
          main.ast.isCall.nameExact(Operators.assignmentPlus).codeExact("a.m += \"!\"").l: @unchecked
        val List(memberCompoundTarget) =
          memberCompoundAssignment.argument.isCall.nameExact(Operators.fieldAccess).l: @unchecked
        memberCompoundTarget.typeFullName shouldBe "java.lang.String"
        memberCompoundTarget.argument.isIdentifier.nameExact("a").refsTo.l shouldBe List(aLocal)

        val List(indexAssignment) =
          main.ast.isCall.nameExact(Operators.assignment).codeExact("xs[0] = 2").l: @unchecked
        val List(indexTarget) = indexAssignment.argument.isCall.nameExact(Operators.indexAccess).l: @unchecked
        indexTarget.argument.isIdentifier.nameExact("xs").refsTo.l shouldBe List(xsLocal)
        indexTarget.argument.isLiteral.codeExact("0").size shouldBe 1

        val List(indexCompoundAssignment) =
          main.ast.isCall.nameExact(Operators.assignmentPlus).codeExact("xs[1] += 3").l: @unchecked
        val List(indexCompoundTarget) =
          indexCompoundAssignment.argument.isCall.nameExact(Operators.indexAccess).l: @unchecked
        indexCompoundTarget.argument.isIdentifier.nameExact("xs").refsTo.l shouldBe List(xsLocal)
        indexCompoundTarget.argument.isLiteral.codeExact("1").size shouldBe 1

        val expectedFields = List(
          ("a.m", "java.lang.String", "a", aLocal),
          ("a.n", "int", "a", aLocal),
          ("box.value", "java.lang.String", "box", boxLocal),
          ("child.inherited", "java.lang.String", "child", childLocal)
        )
        expectedFields.foreach { case (code, typeFullName, receiverName, receiverLocal) =>
          val fields = main.ast.isCall.nameExact(Operators.fieldAccess).codeExact(code).l
          fields.nonEmpty shouldBe true
          fields.foreach { field =>
            field.typeFullName shouldBe typeFullName
            field.argument.isIdentifier.nameExact(receiverName).refsTo.l shouldBe List(receiverLocal)
          }
        }
      }
    }

    "lower chained field and index assignment targets" in {
      withOxidizedCpg("""package demo
          |
          |class Leaf {
          |  var c: String = "leaf"
          |}
          |
          |class Holder {
          |  val leaves: Array<Leaf> = arrayOf(Leaf())
          |}
          |
          |class Root {
          |  val holder: Holder = Holder()
          |}
          |
          |fun main(args: Array<String>) {
          |  val root = Root()
          |  println(root.holder.leaves[0].c)
          |  root.holder.leaves[0].c = "VALUE"
          |  root.holder.leaves[0].c += "!"
          |}
          |""".stripMargin) { cpg =>
        val List(main) = cpg.method.fullNameExact("demo.main:void(java.lang.String[])").l: @unchecked
        val rootLocal  = main.ast.isLocal.nameExact("root").head

        val finalFieldCalls = main.ast.isCall.nameExact(Operators.fieldAccess).codeExact("root.holder.leaves[0].c").l
        finalFieldCalls.size shouldBe 3
        finalFieldCalls.foreach { finalField =>
          finalField.typeFullName shouldBe "java.lang.String"
          finalField.argument.isFieldIdentifier.canonicalName.l shouldBe List("c")
          val List(indexAccess) =
            finalField.argument.isCall.nameExact(Operators.indexAccess).codeExact("root.holder.leaves[0]").l: @unchecked
          indexAccess.typeFullName shouldBe "demo.Leaf"
          indexAccess.argument.isLiteral.codeExact("0").size shouldBe 1

          val List(leavesAccess) =
            indexAccess.argument.isCall.nameExact(Operators.fieldAccess).codeExact("root.holder.leaves").l: @unchecked
          leavesAccess.typeFullName shouldBe "demo.Leaf[]"
          leavesAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List("leaves")

          val List(holderAccess) =
            leavesAccess.argument.isCall.nameExact(Operators.fieldAccess).codeExact("root.holder").l: @unchecked
          holderAccess.typeFullName shouldBe "demo.Holder"
          holderAccess.argument.isIdentifier.nameExact("root").refsTo.l shouldBe List(rootLocal)
          holderAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List("holder")
        }

        val List(assignmentTarget) = main.ast.isCall
          .nameExact(Operators.assignment)
          .codeExact("root.holder.leaves[0].c = \"VALUE\"")
          .argument
          .isCall
          .nameExact(Operators.fieldAccess)
          .codeExact("root.holder.leaves[0].c")
          .l: @unchecked
        assignmentTarget.argumentIndex shouldBe 1
        assignmentTarget.typeFullName shouldBe "java.lang.String"

        val List(compoundTarget) = main.ast.isCall
          .nameExact(Operators.assignmentPlus)
          .codeExact("root.holder.leaves[0].c += \"!\"")
          .argument
          .isCall
          .nameExact(Operators.fieldAccess)
          .codeExact("root.holder.leaves[0].c")
          .l: @unchecked
        compoundTarget.argumentIndex shouldBe 1
        compoundTarget.typeFullName shouldBe "java.lang.String"
      }
    }

    "lower safe calls index access casts and membership operators" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun expr(args: Array<String>, any: Any, maybe: String?): Any {
          |    val safeLength = maybe?.length
          |    val safeCall = maybe?.trim()
          |    val indexed = args[1]
          |    val casted = any as String
          |    val checked = any is String
          |    val inside = 1 in 0..10
          |    val outside = 1 !in 0..10
          |    val product = 2 * 3 / 4 % 5
          |    val nested = args[1]?.length ?: -1
          |    val fallback = null ?: "fallback"
          |    val label = "nested: " + nested
          |    return casted
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(expr) = cpg.method.nameExact("expr").l: @unchecked

        expr.ast.isLocal.nameExact("casted").typeFullName.l shouldBe List("java.lang.String")
        expr.ast.isLocal.nameExact("checked").typeFullName.l shouldBe List("boolean")
        expr.ast.isLocal.nameExact("inside").typeFullName.l shouldBe List("boolean")
        expr.ast.isLocal.nameExact("outside").typeFullName.l shouldBe List("boolean")
        expr.ast.isLocal.nameExact("nested").typeFullName.l shouldBe List("int")
        expr.ast.isLocal.nameExact("fallback").typeFullName.l shouldBe List("java.lang.String")
        expr.ast.isLocal.nameExact("label").typeFullName.l shouldBe List("java.lang.String")

        val List(safeField) =
          expr.ast.isCall.nameExact(Operators.fieldAccess).codeExact("maybe?.length").l: @unchecked
        safeField.argument.isIdentifier.nameExact("maybe").refsTo.l shouldBe expr.parameter.nameExact("maybe").l
        safeField.argument.isFieldIdentifier.canonicalName.l shouldBe List("length")

        val List(safeCall) = expr.ast.isCall.nameExact("trim").codeExact("maybe?.trim()").l: @unchecked
        safeCall.argument.isIdentifier.nameExact("maybe").refsTo.l shouldBe expr.parameter.nameExact("maybe").l

        val indexCalls = expr.ast.isCall.nameExact(Operators.indexAccess).codeExact("args[1]").l
        indexCalls.size shouldBe 2
        indexCalls.flatMap(_.argument.isIdentifier.nameExact("args").refsTo.l).distinct shouldBe
          expr.parameter.nameExact("args").l
        indexCalls.flatMap(_.argument.isLiteral.codeExact("1").l).size shouldBe 2

        val List(castCall) = expr.ast.isCall.nameExact(Operators.cast).codeExact("any as String").l: @unchecked
        castCall.argument.isIdentifier.nameExact("any").refsTo.l shouldBe expr.parameter.nameExact("any").l
        castCall.argument.isTypeRef.codeExact("String").typeFullName.l shouldBe List("java.lang.String")

        val List(isCall) = expr.ast.isCall.nameExact(Operators.is).codeExact("any is String").l: @unchecked
        isCall.argument.isIdentifier.nameExact("any").refsTo.l shouldBe expr.parameter.nameExact("any").l
        isCall.argument.isTypeRef.codeExact("String").typeFullName.l shouldBe List("java.lang.String")

        val List(inCall) = expr.ast.isCall.nameExact(Operators.in).codeExact("1 in 0..10").l: @unchecked
        inCall.argument.isCall.nameExact(Operators.range).codeExact("0..10").size shouldBe 1
        val List(notInCall) = expr.ast.isCall.nameExact(Operators.notIn).codeExact("1 !in 0..10").l: @unchecked
        notInCall.argument.isCall.nameExact(Operators.range).codeExact("0..10").size shouldBe 1

        val List(moduloCall) =
          expr.ast.isCall.nameExact(Operators.modulo).codeExact("2 * 3 / 4 % 5").l: @unchecked
        moduloCall.argument.isCall.nameExact(Operators.division).codeExact("2 * 3 / 4").size shouldBe 1
        expr.ast.isCall.nameExact(Operators.multiplication).codeExact("2 * 3").size shouldBe 1

        val List(elvisCall) =
          expr.ast.isCall.nameExact(Operators.elvis).codeExact("args[1]?.length ?: -1").l: @unchecked
        elvisCall.typeFullName shouldBe "int"
        elvisCall.argument.isCall.nameExact(Operators.fieldAccess).codeExact("args[1]?.length").size shouldBe 1
        val List(fallbackElvisCall) =
          expr.ast.isCall.nameExact(Operators.elvis).codeExact("""null ?: "fallback"""").l: @unchecked
        fallbackElvisCall.typeFullName shouldBe "java.lang.String"
        val List(stringConcatCall) =
          expr.ast.isCall.nameExact(Operators.addition).codeExact(""""nested: " + nested""").l: @unchecked
        stringConcatCall.typeFullName shouldBe "java.lang.String"
        val List(nestedSafeField) =
          expr.ast.isCall.nameExact(Operators.fieldAccess).codeExact("args[1]?.length").l: @unchecked
        nestedSafeField.argument.isCall.nameExact(Operators.indexAccess).codeExact("args[1]").size shouldBe 1
      }
    }

    "lower string templates object declarations companion objects and enum entries" in {
      withOxidizedCpg("""package demo
          |
          |object TopObject {
          |  val bar: String = "x"
          |  var baz = "y"
          |  fun moo(): String {
          |    return bar
          |  }
          |}
          |
          |class AClass {
          |  companion object NamedCompanion {
          |    val m: String = "AVALUE"
          |    class Inner
          |  }
          |}
          |
          |enum class Direction {
          |  NORTH, SOUTH, WEST, EAST
          |}
          |
          |enum class Color(val rgb: Int) {
          |  RED(0xFF0000),
          |  GREEN(0x00FF00),
          |  BLUE(0x0000FF)
          |}
          |
          |fun render(name: String, age: Int): String {
          |  val msg = "$name is $age years old. The string length is ${name.length}"
          |  val out = AClass.m
          |  TopObject.moo()
          |  return msg
          |}
          |""".stripMargin) { cpg =>
        val List(topObject) = cpg.typeDecl.fullNameExact("demo.TopObject").l: @unchecked
        topObject.code shouldBe "TopObject"
        topObject.inheritsFromTypeFullName shouldBe List("java.lang.Object")
        topObject.member.nameExact("bar").typeFullName.l shouldBe List("java.lang.String")
        topObject.member.nameExact("baz").typeFullName.l shouldBe List("java.lang.String")
        cpg.method.fullNameExact("demo.TopObject.moo:java.lang.String()").parameter.name.l shouldBe List("this")

        val List(companion) = cpg.typeDecl.fullNameExact("demo.AClass$NamedCompanion").l: @unchecked
        companion.name shouldBe "NamedCompanion"
        companion.member.nameExact("m").typeFullName.l shouldBe List("java.lang.String")
        companion.member.nameExact(Constants.CompanionObjectMemberName).typeFullName.l shouldBe List("demo.AClass")
        cpg.typeDecl.fullNameExact("demo.AClass$NamedCompanion$Inner").size shouldBe 1

        val List(direction) = cpg.typeDecl.fullNameExact("demo.Direction").l: @unchecked
        direction.member.name.l should contain allOf ("NORTH", "SOUTH", "WEST", "EAST")
        val List(color) = cpg.typeDecl.fullNameExact("demo.Color").l: @unchecked
        color.member.name.l should contain allOf ("RED", "GREEN", "BLUE", "rgb")

        val List(render) = cpg.method.fullNameExact("demo.render:java.lang.String(java.lang.String,int)").l: @unchecked
        val formattedValues = render.ast.isCall.nameExact(Operators.formattedValue).l
        formattedValues.map(_.code) shouldBe List("name", "age", "name.length")
        formattedValues.map(_.typeFullName) shouldBe List("java.lang.String", "int", "int")
        val List(formatString) = render.ast.isCall.nameExact(Operators.formatString).l: @unchecked
        formatString.code shouldBe "\"$name is $age years old. The string length is ${name.length}\""
        formatString.argument.isCall.nameExact(Operators.formattedValue).size shouldBe 3
        formatString.argument.isCall
          .codeExact("name")
          .argument
          .isIdentifier
          .nameExact("name")
          .refsTo
          .l shouldBe render.parameter.nameExact("name").l
        formatString.argument.isCall
          .codeExact("name.length")
          .argument
          .isCall
          .nameExact(Operators.fieldAccess)
          .argument
          .isIdentifier
          .nameExact("name")
          .refsTo
          .l shouldBe render.parameter.nameExact("name").l

        val List(companionAccess) =
          render.ast.isCall.nameExact(Operators.fieldAccess).codeExact("AClass.m").l: @unchecked
        val List(companionReceiver) = companionAccess.argument.isCall.codeExact("AClass").l: @unchecked
        companionReceiver.typeFullName shouldBe "demo.AClass$NamedCompanion"
        companionReceiver.argument.isIdentifier.nameExact("AClass").typeFullName.l shouldBe List(
          "demo.AClass$NamedCompanion"
        )
        companionReceiver.argument.isFieldIdentifier.canonicalName.l shouldBe List(Constants.CompanionObjectMemberName)
        companionAccess.argument.isFieldIdentifier.canonicalName.l should contain("m")

        val List(objectCall) = render.ast.isCall.nameExact("moo").codeExact("TopObject.moo()").l: @unchecked
        objectCall.methodFullName shouldBe "demo.TopObject.moo:java.lang.String()"
        objectCall.typeFullName shouldBe "java.lang.String"
        objectCall.argument.isIdentifier.nameExact("TopObject").typeFullName.l shouldBe List("demo.TopObject")
      }
    }

    "lower init blocks and secondary constructors" in {
      withOxidizedCpg("""package demo
          |
          |class Foo(val seed: Int) {
          |  init {
          |    println(seed)
          |  }
          |  init {
          |    println("second")
          |  }
          |  constructor(seed: Int, name: String) : this(seed) {
          |    println(name)
          |  }
          |}
          |""".stripMargin) { cpg =>
        cpg.typeDecl
          .fullNameExact("demo.Foo")
          .method
          .fullName
          .l should contain allOf ("demo.Foo.<init>:void(int)", "demo.Foo.<init>:void(int,java.lang.String)")

        val List(primaryCtor) = cpg.method.fullNameExact("demo.Foo.<init>:void(int)").l: @unchecked
        primaryCtor.parameter.name.l shouldBe List("this", "seed")
        primaryCtor.ast.isCall
          .nameExact("println")
          .code
          .l should contain allOf ("println(seed)", """println("second")""")
        primaryCtor.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("seed").refsTo.l shouldBe
          primaryCtor.parameter.nameExact("seed").l

        val List(secondaryCtor) =
          cpg.method.fullNameExact("demo.Foo.<init>:void(int,java.lang.String)").l: @unchecked
        secondaryCtor.parameter.name.l shouldBe List("this", "seed", "name")
        secondaryCtor.block.astChildren.isCall.take(1).methodFullName.l shouldBe List("demo.Foo.<init>:void(int)")
        secondaryCtor.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("name").refsTo.l shouldBe
          secondaryCtor.parameter.nameExact("name").l
      }
    }

    "lower primary superclass constructor calls before initializer bodies" in {
      withOxidizedCpg("""package demo
          |
          |open class Base(val seed: Int)
          |
          |class Child(seed: Int) : Base(seed) {
          |  init {
          |    println(seed)
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(childCtor) = cpg.method.fullNameExact("demo.Child.<init>:void(int)").l: @unchecked
        childCtor.parameter.name.l shouldBe List("this", "seed")

        val List(superInit) = childCtor.block.astChildren.isCall.take(1).l: @unchecked
        superInit.code shouldBe "Base(seed)"
        superInit.methodFullName shouldBe "demo.Base.<init>:void(int)"
        superInit.signature shouldBe "void(int)"
        superInit.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        superInit.typeFullName shouldBe "void"
        superInit.argument.code.l shouldBe List("this", "seed")
        superInit.argument.isIdentifier.nameExact("this").typeFullName.l shouldBe List("demo.Base")
        superInit.argument.isIdentifier.nameExact("this").refsTo.l shouldBe childCtor.parameter.nameExact("this").l
        superInit.argument.isIdentifier.nameExact("seed").refsTo.l shouldBe childCtor.parameter.nameExact("seed").l

        val List(printlnCall) = childCtor.ast.isCall.nameExact("println").l: @unchecked
        printlnCall.argument.isIdentifier.nameExact("seed").refsTo.l shouldBe childCtor.parameter.nameExact("seed").l
      }
    }

    "lower visibility and abstract modifiers" in {
      withOxidizedCpg("""package demo
          |
          |fun topDefault(): Int {
          |  return 1
          |}
          |public fun topPublic(): Int = 2
          |private fun topPrivate(): Int = 3
          |internal fun topInternal(): Int = 4
          |
          |abstract class Base {
          |  abstract fun required(): Int
          |  fun visible(): Int = 5
          |  private fun hidden(): Int = 6
          |  internal fun inside(): Int = 7
          |  protected fun childOnly(): Int = 8
          |}
          |
          |interface Worker {
          |  fun work(): Int
          |}
          |""".stripMargin) { cpg =>
        def methodModifiers(fullName: String): List[String] =
          cpg.method.fullNameExact(fullName).modifier.modifierType.l

        methodModifiers("demo.topDefault:int()") shouldBe List(ModifierTypes.PUBLIC)
        methodModifiers("demo.topPublic:int()") shouldBe List(ModifierTypes.PUBLIC)
        methodModifiers("demo.topPrivate:int()") shouldBe List(ModifierTypes.PRIVATE)
        methodModifiers("demo.topInternal:int()") shouldBe List(ModifierTypes.INTERNAL)

        methodModifiers("demo.Base.required:int()") shouldBe List(
          ModifierTypes.PUBLIC,
          ModifierTypes.VIRTUAL,
          ModifierTypes.ABSTRACT
        )
        methodModifiers("demo.Base.visible:int()") shouldBe List(ModifierTypes.PUBLIC, ModifierTypes.VIRTUAL)
        methodModifiers("demo.Base.hidden:int()") shouldBe List(ModifierTypes.PRIVATE, ModifierTypes.VIRTUAL)
        methodModifiers("demo.Base.inside:int()") shouldBe List(ModifierTypes.INTERNAL, ModifierTypes.VIRTUAL)
        methodModifiers("demo.Base.childOnly:int()") shouldBe List(ModifierTypes.PROTECTED, ModifierTypes.VIRTUAL)

        cpg.typeDecl.fullNameExact("demo.Base").modifier.modifierType.l shouldBe List(ModifierTypes.ABSTRACT)
        cpg.typeDecl.fullNameExact("demo.Worker").modifier.modifierType.l shouldBe List(ModifierTypes.ABSTRACT)
        methodModifiers("demo.Worker.work:int()") shouldBe List(
          ModifierTypes.PUBLIC,
          ModifierTypes.VIRTUAL,
          ModifierTypes.ABSTRACT
        )

        cpg.method.nameExact("topDefault").sourceCode.l shouldBe List("""fun topDefault(): Int {
            |  return 1
            |}""".stripMargin)
      }
    }

    "lower declaration annotations" in {
      withOxidizedCpg("""package demo
          |
          |annotation class Fancy(val value: String)
          |
          |@Fancy("type")
          |class Annotated(@Fancy("ctorParam") val seed: Int) {
          |  @Fancy("field")
          |  val field: String = "x"
          |
          |  @Fancy("secondary")
          |  constructor(@Fancy("secondaryParam") seed: Int, name: String) : this(seed)
          |
          |  @Fancy("method")
          |  fun make(@Fancy("param") input: String): String {
          |    @Fancy("local") val local = input
          |    return local
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(typeDecl)                   = cpg.typeDecl.nameExact("Annotated").l: @unchecked
        val List(typeAnnotation: Annotation) = typeDecl.astChildren.collectAll[Annotation].l: @unchecked
        typeAnnotation.code shouldBe "@Fancy(\"type\")"
        typeAnnotation.name shouldBe "Fancy"
        typeAnnotation.fullName shouldBe "demo.Fancy"
        typeAnnotation.astChildren.collectAll[AnnotationLiteral].code.l shouldBe List("\"type\"")

        val List(primaryCtor) = cpg.method.fullNameExact("demo.Annotated.<init>:void(int)").l: @unchecked
        val List(primaryParamAnnotation: Annotation) =
          primaryCtor.parameter.nameExact("seed").astChildren.collectAll[Annotation].l: @unchecked
        primaryParamAnnotation.code shouldBe "@Fancy(\"ctorParam\")"
        primaryParamAnnotation.fullName shouldBe "demo.Fancy"
        typeDecl.member.nameExact("seed").astChildren.collectAll[Annotation].code.l shouldBe List(
          "@Fancy(\"ctorParam\")"
        )

        val List(fieldAnnotation: Annotation) =
          typeDecl.member.nameExact("field").astChildren.collectAll[Annotation].l: @unchecked
        fieldAnnotation.code shouldBe "@Fancy(\"field\")"
        fieldAnnotation.fullName shouldBe "demo.Fancy"

        val List(secondaryCtor) =
          cpg.method.fullNameExact("demo.Annotated.<init>:void(int,java.lang.String)").l: @unchecked
        secondaryCtor.astChildren.collectAll[Annotation].code.l shouldBe List("@Fancy(\"secondary\")")
        secondaryCtor.parameter.nameExact("seed").astChildren.collectAll[Annotation].code.l shouldBe List(
          "@Fancy(\"secondaryParam\")"
        )

        val List(make) =
          cpg.method.fullNameExact("demo.Annotated.make:java.lang.String(java.lang.String)").l: @unchecked
        make.astChildren.collectAll[Annotation].code.l shouldBe List("@Fancy(\"method\")")
        make.parameter.nameExact("input").astChildren.collectAll[Annotation].code.l shouldBe List("@Fancy(\"param\")")
        make.ast.isLocal.nameExact("local").astChildren.collectAll[Annotation].code.l shouldBe List("@Fancy(\"local\")")
      }
    }

    "lower type aliases" in {
      withOxidizedCpg("""package demo
          |
          |annotation class Marker
          |
          |@Marker
          |typealias MyInt = Int
          |typealias Names = List<String>
          |class AClass(val x: String)
          |typealias ATypeAlias = AClass
          |
          |class Foo {
          |  fun aliases(p: String, args: Array<String>): MyInt {
          |    val x: MyInt = 1
          |    val names: Names = listOf("a")
          |    val aClass: ATypeAlias = ATypeAlias(p)
          |    println(names)
          |    println(args)
          |    return x
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(myInt) = cpg.typeDecl.nameExact("MyInt").l: @unchecked
        myInt.code shouldBe "MyInt"
        myInt.fullName shouldBe "demo.MyInt"
        myInt.isExternal shouldBe false
        myInt.inheritsFromTypeFullName shouldBe List()
        myInt.aliasTypeFullName shouldBe Some("int")
        myInt.astChildren.collectAll[Annotation].code.l shouldBe List("@Marker")
        myInt.astChildren.collectAll[Annotation].fullName.l shouldBe List("demo.Marker")

        val List(names) = cpg.typeDecl.nameExact("Names").l: @unchecked
        names.fullName shouldBe "demo.Names"
        names.aliasTypeFullName shouldBe Some("java.util.List")

        val List(aTypeAlias) = cpg.typeDecl.nameExact("ATypeAlias").l: @unchecked
        aTypeAlias.fullName shouldBe "demo.ATypeAlias"
        aTypeAlias.aliasTypeFullName shouldBe Some("demo.AClass")

        val List(aliases) =
          cpg.method.fullNameExact("demo.Foo.aliases:int(java.lang.String,java.lang.String[])").l: @unchecked
        aliases.methodReturn.typeFullName shouldBe "int"
        aliases.parameter.nameExact("args").typeFullName.l shouldBe List("java.lang.String[]")
        aliases.ast.isLocal.nameExact("x").typeFullName.l shouldBe List("int")
        aliases.ast.isLocal.nameExact("names").typeFullName.l shouldBe List("java.util.List")
        aliases.ast.isLocal.nameExact("aClass").typeFullName.l shouldBe List("demo.AClass")

        val List(aliasCtorCall) = aliases.ast.isCall.methodFullNameExact("demo.AClass.<init>:void(java.lang.String)").l
        aliasCtorCall.name shouldBe "<init>"
        aliasCtorCall.code shouldBe "ATypeAlias(p)"
        aliasCtorCall.argument.isIdentifier.nameExact("p").refsTo.l shouldBe aliases.parameter.nameExact("p").l
      }
    }

    "lower generic bounds and erased generic method bindings" in {
      withOxidizedCpg("""package demo
          |
          |interface Transformer<T, R> {
          |  fun transform(p: T): R
          |}
          |
          |open class TestClass
          |
          |interface Bounded<T : TestClass, R> {
          |  fun run(p: T): R
          |}
          |
          |class TestClassImpl : TestClass()
          |
          |class TransformerImpl : Transformer<Int, Boolean> {
          |  override fun transform(p: Int): Boolean {
          |    return p == 42
          |  }
          |}
          |
          |class BoundedImpl : Bounded<TestClassImpl, Boolean> {
          |  override fun run(p: TestClassImpl): Boolean {
          |    return p is TestClass
          |  }
          |}
          |
          |fun <T> makeAny(name: String): T? {
          |  return TestClassImpl() as T
          |}
          |
          |fun <T : TestClass> makeBound(name: String): T? {
          |  return TestClassImpl() as T
          |}
          |
          |fun main() {
          |  val p1 = makeAny<TestClassImpl>("a")
          |  val p2 = makeBound<TestClassImpl>("b")
          |  println(p1)
          |  println(p2)
          |}
          |""".stripMargin) { cpg =>
        val List(transformerImpl) = cpg.typeDecl.nameExact("TransformerImpl").l: @unchecked
        transformerImpl.inheritsFromTypeFullName shouldBe List("demo.Transformer")
        val transformerBindings = transformerImpl.methodBinding.nameExact("transform").l.sortBy(_.signature)
        transformerBindings.map(_.signature) shouldBe List("boolean(int)", "java.lang.Object(java.lang.Object)")
        transformerBindings.map(_.methodFullName).distinct shouldBe List("demo.TransformerImpl.transform:boolean(int)")

        val List(boundedImpl) = cpg.typeDecl.nameExact("BoundedImpl").l: @unchecked
        boundedImpl.inheritsFromTypeFullName shouldBe List("demo.Bounded")
        val boundedBindings = boundedImpl.methodBinding.nameExact("run").l.sortBy(_.signature)
        boundedBindings.map(_.signature) shouldBe List(
          "boolean(demo.TestClassImpl)",
          "java.lang.Object(demo.TestClass)"
        )
        boundedBindings.map(_.methodFullName).distinct shouldBe List("demo.BoundedImpl.run:boolean(demo.TestClassImpl)")

        cpg.method.fullNameExact("demo.makeAny:java.lang.Object(java.lang.String)").size shouldBe 1
        cpg.method.fullNameExact("demo.makeBound:demo.TestClass(java.lang.String)").size shouldBe 1

        val List(main) = cpg.method.fullNameExact("demo.main:void()").l: @unchecked
        main.ast.isLocal.nameExact("p1").typeFullName.l shouldBe List("demo.TestClassImpl")
        main.ast.isLocal.nameExact("p2").typeFullName.l shouldBe List("demo.TestClassImpl")
        main.ast.isIdentifier.nameExact("p1").typeFullName.toSet shouldBe Set("demo.TestClassImpl")
        main.ast.isIdentifier.nameExact("p2").typeFullName.toSet shouldBe Set("demo.TestClassImpl")
        main.ast.isCall.codeExact("""makeAny<TestClassImpl>("a")""").methodFullName.l shouldBe List(
          "demo.makeAny:java.lang.Object(java.lang.String)"
        )
        main.ast.isCall.codeExact("""makeBound<TestClassImpl>("b")""").methodFullName.l shouldBe List(
          "demo.makeBound:demo.TestClass(java.lang.String)"
        )

        cpg.method
          .fullNameExact("demo.makeAny:java.lang.Object(java.lang.String)")
          .ast
          .isCall
          .nameExact(Operators.cast)
          .typeFullName
          .l shouldBe List("java.lang.Object")
        cpg.method
          .fullNameExact("demo.makeBound:demo.TestClass(java.lang.String)")
          .ast
          .isCall
          .nameExact(Operators.cast)
          .typeFullName
          .l shouldBe List("demo.TestClass")
      }
    }

    "lower local functions and classes" in {
      withOxidizedCpg("""package demo
          |
          |fun sink(x: String) = println(x)
          |
          |fun f1(p: String) {
          |  fun f2(q: String) {
          |    fun f3(r: String) {
          |      println(r)
          |    }
          |    f3(q)
          |  }
          |
          |  class AClass {
          |    fun doSomething(r: String) {
          |      class BClass {
          |        fun doSomethingElse(s: String) {
          |          sink(s)
          |        }
          |      }
          |      val bClass = BClass()
          |      bClass.doSomethingElse(r)
          |    }
          |  }
          |
          |  val aClass = AClass()
          |  f2(p)
          |  aClass.doSomething(p)
          |}
          |""".stripMargin) { cpg =>
        val List(f1) = cpg.method.fullNameExact("demo.f1:void(java.lang.String)").l: @unchecked
        val List(f2) = cpg.method.fullNameExact("demo.f1.f2:void(java.lang.String)").l: @unchecked
        val List(f3) = cpg.method.fullNameExact("demo.f1.f2.f3:void(java.lang.String)").l: @unchecked

        f2.parameter.name.l shouldBe List("q")
        f3.parameter.name.l shouldBe List("r")
        f2.astIn.isBlock.astIn.isMethod.fullName.l shouldBe List(f1.fullName)
        f3.astIn.isBlock.astIn.isMethod.fullName.l shouldBe List(f2.fullName)
        cpg.typeDecl.nameExact("f2").methodBinding.methodFullName.l should contain(f2.fullName)
        cpg.typeDecl.nameExact("f3").methodBinding.methodFullName.l should contain(f3.fullName)

        f1.ast.isCall.codeExact("f2(p)").methodFullName.l shouldBe List(f2.fullName)
        f2.ast.isCall.codeExact("f3(q)").methodFullName.l shouldBe List(f3.fullName)
        f3.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("r").refsTo.l shouldBe
          f3.parameter.nameExact("r").l

        val List(aClassType) = cpg.typeDecl.nameExact("AClass").l: @unchecked
        aClassType.fullName shouldBe "demo.f1.AClass"
        aClassType.inheritsFromTypeFullName shouldBe List("java.lang.Object")
        aClassType.astIn.isBlock.astIn.isMethod.fullName.l shouldBe List(f1.fullName)
        aClassType.method.fullName.l should contain allOf (
          "demo.f1.AClass.<init>:void()",
          "demo.f1.AClass.doSomething:void(java.lang.String)"
        )

        val List(doSomething) =
          cpg.method.fullNameExact("demo.f1.AClass.doSomething:void(java.lang.String)").l: @unchecked
        val List(bClassType) = cpg.typeDecl.nameExact("BClass").l: @unchecked
        bClassType.fullName shouldBe "demo.f1.AClass.doSomething.BClass"
        bClassType.inheritsFromTypeFullName shouldBe List("java.lang.Object")
        bClassType.astIn.isBlock.astIn.isMethod.fullName.l shouldBe List(doSomething.fullName)
        bClassType.method.fullName.l should contain allOf (
          "demo.f1.AClass.doSomething.BClass.<init>:void()",
          "demo.f1.AClass.doSomething.BClass.doSomethingElse:void(java.lang.String)"
        )

        val List(aClassLocal) = f1.ast.isLocal.nameExact("aClass").l: @unchecked
        aClassLocal.typeFullName shouldBe "demo.f1.AClass"
        f1.ast.isCall.codeExact("AClass()").methodFullName.l shouldBe List("demo.f1.AClass.<init>:void()")
        f1.ast.isCall.codeExact("aClass.doSomething(p)").methodFullName.l shouldBe List(doSomething.fullName)

        val List(bClassLocal) = doSomething.ast.isLocal.nameExact("bClass").l: @unchecked
        bClassLocal.typeFullName shouldBe "demo.f1.AClass.doSomething.BClass"
        doSomething.ast.isCall.codeExact("BClass()").methodFullName.l shouldBe List(
          "demo.f1.AClass.doSomething.BClass.<init>:void()"
        )
        doSomething.ast.isCall.codeExact("bClass.doSomethingElse(r)").methodFullName.l shouldBe List(
          "demo.f1.AClass.doSomething.BClass.doSomethingElse:void(java.lang.String)"
        )

        cpg.method
          .fullNameExact("demo.f1.AClass.doSomething.BClass.doSomethingElse:void(java.lang.String)")
          .ast
          .isCall
          .codeExact("sink(s)")
          .methodFullName
          .l shouldBe List("demo.sink:void(java.lang.String)")
      }
    }

    "lower class literals and delegated properties" in {
      withOxidizedCpg("""package demo
          |
          |@Target(AnnotationTarget.EXPRESSION)
          |@Retention(AnnotationRetention.SOURCE)
          |annotation class Fancy
          |
          |class Bar
          |class Baz
          |
          |class Holder {
          |  val myName: String by lazy { "one" + "two" }
          |}
          |
          |fun foo() {
          |  println(Bar::class)
          |  println(Baz::class.java)
          |  @Fancy Bar::class
          |}
          |""".stripMargin) { cpg =>
        val List(holder) = cpg.typeDecl.fullNameExact("demo.Holder").l: @unchecked
        holder.member.nameExact("myName").typeFullName.l shouldBe List("java.lang.String")

        val List(foo)         = cpg.method.fullNameExact("demo.foo:void()").l: @unchecked
        val classLiteralCalls = foo.ast.isCall.methodFullNameExact("<operator>.class").l
        classLiteralCalls.map(_.code) should contain allOf ("Bar::class", "Baz::class")
        classLiteralCalls.foreach { call =>
          call.argument.size shouldBe 0
          call.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          call.signature shouldBe "kotlin.reflect.KClass()"
          call.typeFullName shouldBe "kotlin.reflect.KClass"
        }

        cpg.annotation.codeExact("@Fancy").astParent.code.l should contain("Bar::class")

        val List(printlnBar) = foo.ast.isCall.nameExact("println").codeExact("println(Bar::class)").l: @unchecked
        printlnBar.argument.isCall.methodFullNameExact("<operator>.class").code.l shouldBe List("Bar::class")

        val List(javaAccess) =
          foo.ast.isCall.nameExact(Operators.fieldAccess).codeExact("Baz::class.java").l: @unchecked
        javaAccess.typeFullName shouldBe "java.lang.Class"
        javaAccess.argument.isCall.methodFullNameExact("<operator>.class").code.l shouldBe List("Baz::class")
        javaAccess.argument.isFieldIdentifier.canonicalName.l shouldBe List("java")
      }
    }

    "lower unbound top-level callable references" in {
      withOxidizedCpg("""package demo
          |
          |fun globalFunction(x: Int, y: Int): String {
          |  return "result"
          |}
          |
          |fun test() {
          |  val ref: (Int, Int) -> String = ::globalFunction
          |  println(ref)
          |}
          |""".stripMargin) { cpg =>
        val expectedMethodFullName = "demo.globalFunction:java.lang.String(int,int)"
        val expectedReferenceType =
          "demo.globalFunction$kotlin.jvm.functions.Function2Impl.invoke:java.lang.String(int,int)"

        val List(methodRef) = cpg.methodRef.codeExact("::globalFunction").l: @unchecked
        methodRef.methodFullName shouldBe expectedMethodFullName
        methodRef.typeFullName shouldBe expectedReferenceType
        methodRef.referencedMethod.fullName shouldBe expectedMethodFullName

        val List(assignment) =
          cpg.call
            .nameExact(Operators.assignment)
            .codeExact("val ref: (Int, Int) -> String = ::globalFunction")
            .l: @unchecked
        assignment.argument.isMethodRef.codeExact("::globalFunction").l shouldBe List(methodRef)
        cpg.local.nameExact("ref").typeFullName.l shouldBe List("(Int, Int) -> String")
      }
    }

    "lower bound callable references" in {
      withOxidizedCpg("""package demo
          |
          |class Handler {
          |  fun process(x: Int, y: String): Boolean {
          |    return true
          |  }
          |}
          |
          |class Utils {
          |  companion object {
          |    fun validate(x: Int): Boolean = x > 0
          |  }
          |}
          |
          |fun test() {
          |  val handler = Handler()
          |  val ref: (Int, String) -> Boolean = handler::process
          |  val staticRef: (Int) -> Boolean = Utils::validate
          |  println(ref)
          |  println(staticRef)
          |}
          |""".stripMargin) { cpg =>
        val handlerSamType =
          "demo.Handler.process$kotlin.jvm.functions.Function2Impl.invoke:boolean(int,java.lang.String)"
        val handlerInvokeFullName = s"$handlerSamType.invoke:boolean(int,java.lang.String)"
        val handlerCtorFullName   = s"$handlerSamType.<init>:void(demo.Handler)"
        val List(handlerTypeDecl) = cpg.typeDecl.fullNameExact(handlerSamType).l: @unchecked
        handlerTypeDecl.inheritsFromTypeFullName should contain allOf (
          "kotlin.jvm.functions.Function2",
          "kotlin.jvm.internal.CallableReference"
        )
        handlerTypeDecl.methodBinding.nameExact("invoke").signature.l.sorted shouldBe List(
          "boolean(int,java.lang.String)",
          "java.lang.Object(java.lang.Object,java.lang.Object)"
        )

        val List(handlerInvoke) = handlerTypeDecl.method.fullNameExact(handlerInvokeFullName).l: @unchecked
        handlerInvoke.signature shouldBe "boolean(int,java.lang.String)"
        handlerInvoke.parameter.typeFullName.l shouldBe List(handlerSamType, "int", "java.lang.String")
        val List(processCall) = handlerInvoke.ast.isCall.nameExact("process").l: @unchecked
        processCall.methodFullName shouldBe "demo.Handler.process:boolean(int,java.lang.String)"
        processCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        processCall.receiver.isCall.nameExact(Operators.fieldAccess).typeFullName.l shouldBe List("demo.Handler")
        processCall.argument.isIdentifier.nameExact("p1").typeFullName.l shouldBe List("int")
        processCall.argument.isIdentifier.nameExact("p2").typeFullName.l shouldBe List("java.lang.String")

        val List(handlerCtorCall) = cpg.call.methodFullNameExact(handlerCtorFullName).l: @unchecked
        handlerCtorCall.signature shouldBe "void(demo.Handler)"
        handlerCtorCall.argument.isIdentifier.nameExact("handler").typeFullName.l shouldBe List("demo.Handler")

        val utilsSamType =
          "demo.Utils$Companion.validate$kotlin.jvm.functions.Function1Impl.invoke:boolean(int)"
        val utilsInvokeFullName = s"$utilsSamType.invoke:boolean(int)"
        val utilsCtorFullName   = utilsSamType + ".<init>:void(demo.Utils$Companion)"
        val List(utilsTypeDecl) = cpg.typeDecl.fullNameExact(utilsSamType).l: @unchecked
        utilsTypeDecl.inheritsFromTypeFullName should contain allOf (
          "kotlin.jvm.functions.Function1",
          "kotlin.jvm.internal.CallableReference"
        )
        utilsTypeDecl.methodBinding.nameExact("invoke").signature.l.sorted shouldBe List(
          "boolean(int)",
          "java.lang.Object(java.lang.Object)"
        )

        val List(utilsInvoke)  = utilsTypeDecl.method.fullNameExact(utilsInvokeFullName).l: @unchecked
        val List(validateCall) = utilsInvoke.ast.isCall.nameExact("validate").l: @unchecked
        validateCall.methodFullName shouldBe "demo.Utils$Companion.validate:boolean(int)"
        validateCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        validateCall.receiver.isCall.nameExact(Operators.fieldAccess).typeFullName.l shouldBe List(
          "demo.Utils$Companion"
        )

        val List(utilsCtorCall) = cpg.call.methodFullNameExact(utilsCtorFullName).l: @unchecked
        utilsCtorCall.signature.shouldBe("void(demo.Utils$Companion)")
        utilsCtorCall.argument.isCall
          .nameExact(Operators.fieldAccess)
          .typeFullName
          .l
          .should(contain("demo.Utils$Companion"))
      }
    }

    "lower callable reference edge cases" in {
      withOxidizedCpg("""package demo
          |
          |class Counter {
          |  fun increment(): Int = 1
          |}
          |
          |class MyClass {
          |  fun method(x: Int) {}
          |
          |  fun setup() {
          |    val ref: (Int) -> Unit = this::method
          |    println(ref)
          |  }
          |}
          |
          |class Calculator {
          |  fun add(a: Int, b: Int): Int = a + b
          |}
          |
          |fun test() {
          |  val counter = Counter()
          |  val zeroRef: () -> Int = counter::increment
          |  val calc1 = Calculator()
          |  val calc2 = Calculator()
          |  val ref1: (Int, Int) -> Int = calc1::add
          |  val ref2: (Int, Int) -> Int = calc2::add
          |  val ref3: (Int, Int) -> Int = calc1::add
          |  println(zeroRef)
          |  println(ref1)
          |  println(ref2)
          |  println(ref3)
          |}
          |""".stripMargin) { cpg =>
        val counterSamType        = "demo.Counter.increment$kotlin.jvm.functions.Function0Impl.invoke:int()"
        val counterCtor           = s"$counterSamType.<init>:void(demo.Counter)"
        val List(counterTypeDecl) = cpg.typeDecl.fullNameExact(counterSamType).l: @unchecked
        counterTypeDecl.method.nameExact("invoke").signature.l shouldBe List("int()")
        counterTypeDecl.method.nameExact("invoke").parameter.typeFullName.l shouldBe List(counterSamType)
        counterTypeDecl.method.nameExact("<init>").signature.l shouldBe List("void(demo.Counter)")
        val List(counterCtorCall) = cpg.call.methodFullNameExact(counterCtor).l: @unchecked
        counterCtorCall.argument.isIdentifier.nameExact("counter").typeFullName.l shouldBe List("demo.Counter")
        val List(incrementCall) =
          counterTypeDecl.method.nameExact("invoke").ast.isCall.nameExact("increment").l: @unchecked
        incrementCall.methodFullName shouldBe "demo.Counter.increment:int()"
        incrementCall.signature shouldBe "int()"
        incrementCall.argument.isIdentifier.size shouldBe 0

        val thisSamType        = "demo.MyClass.method$kotlin.jvm.functions.Function1Impl.invoke:void(int)"
        val thisCtor           = s"$thisSamType.<init>:void(demo.MyClass)"
        val List(thisTypeDecl) = cpg.typeDecl.fullNameExact(thisSamType).l: @unchecked
        thisTypeDecl.method.nameExact("invoke").signature.l shouldBe List("void(int)")
        thisTypeDecl.method.nameExact("<init>").signature.l shouldBe List("void(demo.MyClass)")
        val List(thisCtorCall) = cpg.call.methodFullNameExact(thisCtor).l: @unchecked
        thisCtorCall.argument.isIdentifier.nameExact(Constants.ThisName).typeFullName.l shouldBe List("demo.MyClass")
        val List(methodCall) = thisTypeDecl.method.nameExact("invoke").ast.isCall.nameExact("method").l: @unchecked
        methodCall.methodFullName shouldBe "demo.MyClass.method:void(int)"
        methodCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        methodCall.argument.isIdentifier.nameExact("p1").typeFullName.l shouldBe List("int")

        val calculatorSamType = "demo.Calculator.add$kotlin.jvm.functions.Function2Impl.invoke:int(int,int)"
        val calculatorCtor    = s"$calculatorSamType.<init>:void(demo.Calculator)"
        cpg.typeDecl.fullNameExact(calculatorSamType).size shouldBe 1
        cpg.typeDecl.fullNameExact(calculatorSamType).method.nameExact("invoke").signature.l shouldBe List(
          "int(int,int)"
        )
        cpg.call.methodFullNameExact(calculatorCtor).size shouldBe 3
      }
    }

    "lower expression annotations" in {
      withOxidizedCpg("""package demo
          |
          |@Target(AnnotationTarget.EXPRESSION)
          |@Retention(AnnotationRetention.SOURCE)
          |annotation class Fancy(val value: String = "x")
          |
          |class Foo {
          |  fun annotated(seed: Int) {
          |    @Fancy println("call")
          |    @Fancy 1 + 1
          |    val choice = @Fancy("when") when (seed) {
          |      1 -> "one"
          |      else -> "other"
          |    }
          |    @Fancy if (seed > 0) println("positive") else println("zero")
          |    @Fancy "literal"
          |    @Fancy { println("lambda") }
          |    @Fancy object {
          |      fun value(): Int {
          |        return seed
          |      }
          |    }
          |  }
          |}
          |""".stripMargin) { cpg =>
        cpg.annotation.codeExact("@Fancy").size shouldBe 6
        cpg.annotation.codeExact("@Fancy(\"when\")").size shouldBe 1
        cpg.annotation.fullNameExact("demo.Fancy").size shouldBe 7

        cpg.method
          .nameExact("annotated")
          .ast
          .isCall
          .nameExact("println")
          .codeExact("""println("call")""")
          .astChildren
          .collectAll[Annotation]
          .code
          .l
          .shouldBe(List("@Fancy"))
        cpg.method
          .nameExact("annotated")
          .ast
          .isCall
          .nameExact("<operator>.when")
          .astChildren
          .collectAll[Annotation]
          .code
          .l
          .shouldBe(List("@Fancy(\"when\")"))
        cpg.method
          .nameExact("annotated")
          .ast
          .isCall
          .nameExact(Operators.addition)
          .codeExact("1 + 1")
          .astChildren
          .collectAll[Annotation]
          .code
          .l
          .shouldBe(List("@Fancy"))
        cpg.annotation
          .codeExact("@Fancy")
          .astParent
          .code
          .l should contain("""if (seed > 0) println("positive") else println("zero")""")
        cpg.method
          .nameExact("annotated")
          .ast
          .isLiteral
          .codeExact("\"literal\"")
          .astChildren
          .collectAll[Annotation]
          .code
          .l
          .shouldBe(List("@Fancy"))
        cpg.methodRef
          .methodFullName(".*<lambda>.*")
          .astChildren
          .collectAll[Annotation]
          .code
          .l should contain("@Fancy")
        cpg.method
          .nameExact("annotated")
          .ast
          .isBlock
          .code("object.*")
          .astChildren
          .collectAll[Annotation]
          .code
          .l should contain("@Fancy")
      }
    }

    "lower object literals assigned to locals" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun objects(p: String) {
          |    val o = object {
          |      val m = "meow"
          |      fun printWithSuffix(suffix: String) {
          |        println(suffix + m)
          |      }
          |    }
          |    o.printWithSuffix(p)
          |  }
          |}
          |""".stripMargin) { cpg =>
        val objectFullName   = "demo.Foo.objects.object$0"
        val List(objectType) = cpg.typeDecl.fullNameExact(objectFullName).l: @unchecked
        objectType.name shouldBe "anonymous_obj"
        objectType.inheritsFromTypeFullName shouldBe List("java.lang.Object")
        objectType.member.nameExact("m").typeFullName.l shouldBe List("java.lang.String")
        objectType.boundMethod.fullName.l should contain allOf (
          s"$objectFullName.<init>:void()",
          s"$objectFullName.printWithSuffix:void(java.lang.String)"
        )

        val List(objects) = cpg.method.fullNameExact("demo.Foo.objects:void(java.lang.String)").l: @unchecked
        val List(oLocal)  = objects.ast.isLocal.nameExact("o").l: @unchecked
        oLocal.typeFullName shouldBe objectFullName
        val List(assignment) = objects.ast.isCall.nameExact(Operators.assignment).code("val o = object.*").l: @unchecked
        assignment.argument.isIdentifier.nameExact("o").refsTo.l shouldBe List(oLocal)
        assignment.argument.isCall.nameExact(Operators.alloc).typeFullName.l shouldBe List(objectFullName)

        val List(initCall) =
          objects.ast.isCall.nameExact("<init>").methodFullNameExact(s"$objectFullName.<init>:void()").l
        initCall.argument.isIdentifier.nameExact("o").refsTo.l shouldBe List(oLocal)

        val List(objectMethodCall) = objects.ast.isCall.nameExact("printWithSuffix").l: @unchecked
        objectMethodCall.methodFullName shouldBe s"$objectFullName.printWithSuffix:void(java.lang.String)"
        objectMethodCall.argument.isIdentifier.nameExact("o").refsTo.l shouldBe List(oLocal)
        objectMethodCall.argument.isIdentifier.nameExact("p").refsTo.l shouldBe objects.parameter.nameExact("p").l
      }
    }

    "lower object literals assigned to class properties" in {
      withOxidizedCpg("""package demo
          |
          |open class Callback
          |
          |class Foo constructor(val a: String, b: String) {
          |  private val callback = object : Callback() {
          |    inner class Bar(val a: Long, val b: String, val c: String)
          |  }
          |}
          |""".stripMargin) { cpg =>
        val objectFullName   = "demo.Foo.callback.object$0"
        val List(objectType) = cpg.typeDecl.nameExact("anonymous_obj").l: @unchecked
        objectType.fullName shouldBe objectFullName
        objectType.inheritsFromTypeFullName shouldBe List("demo.Callback")
        objectType.astParentType shouldBe "TYPE_DECL"
        objectType.astParentFullName shouldBe "demo.Foo"

        val List(callbackMember) = cpg.typeDecl.nameExact("Foo").member.nameExact("callback").l: @unchecked
        callbackMember.typeFullName shouldBe objectFullName

        val List(objectCtor) = objectType.astChildren.isMethod.nameExact("<init>").l: @unchecked
        objectCtor.fullName shouldBe s"$objectFullName.<init>:void()"
        objectCtor.parameter.nameExact("this").typeFullName.l shouldBe List(objectFullName)
        objectCtor.methodReturn.typeFullName shouldBe "void"

        val List(innerClass) = cpg.typeDecl.nameExact("Bar").l: @unchecked
        innerClass.fullName shouldBe s"${objectFullName}$$Bar"
        innerClass.astParentType shouldBe "TYPE_DECL"
        innerClass.astParentFullName shouldBe objectFullName

        val List(innerCtor) = innerClass.astChildren.isMethod.nameExact("<init>").l: @unchecked
        innerCtor.fullName shouldBe s"${objectFullName}$$Bar.<init>:void(long,java.lang.String,java.lang.String)"
        innerCtor.parameter.name.l shouldBe List("this", "a", "b", "c")
        innerCtor.methodReturn.typeFullName shouldBe "void"
      }
    }

    "lower inline object literals as expression blocks" in {
      withOxidizedCpg("""package demo
          |
          |interface AnInterface {
          |  fun doSomething(x: String)
          |}
          |
          |fun does(x: AnInterface, p: String) {
          |  x.doSomething(p)
          |}
          |
          |class Foo {
          |  fun inlineObject(p: String) {
          |    does(object : AnInterface {
          |      override fun doSomething(x: String) {
          |        println(x)
          |      }
          |    }, p)
          |  }
          |}
          |""".stripMargin) { cpg =>
        val objectFullName   = "demo.Foo.inlineObject.object$0"
        val List(objectType) = cpg.typeDecl.fullNameExact(objectFullName).l: @unchecked
        objectType.name shouldBe "anonymous_obj"
        objectType.inheritsFromTypeFullName shouldBe List("demo.AnInterface")
        objectType.boundMethod.fullName.l should contain allOf (
          s"$objectFullName.<init>:void()",
          s"$objectFullName.doSomething:void(java.lang.String)"
        )

        val List(inlineObject) = cpg.method.fullNameExact("demo.Foo.inlineObject:void(java.lang.String)").l: @unchecked
        val List(doesCall)     = inlineObject.ast.isCall.nameExact("does").l: @unchecked
        val List(objectBlock)  = doesCall.argument.isBlock.l: @unchecked

        objectBlock.astChildren.isTypeDecl.fullName.l shouldBe List(objectFullName)
        val List(tmpLocal) = objectBlock.astChildren.isLocal.nameExact("tmp_obj_1").l: @unchecked
        tmpLocal.typeFullName shouldBe objectFullName
        objectBlock.astChildren.isCall.nameExact(Operators.assignment).code.l shouldBe List("tmp_obj_1 = <alloc>")
        objectBlock.astChildren.isCall
          .nameExact(Operators.assignment)
          .argument
          .isIdentifier
          .nameExact("tmp_obj_1")
          .refsTo
          .l shouldBe
          List(tmpLocal)
        objectBlock.astChildren.isCall.nameExact("<init>").methodFullName.l shouldBe List(
          s"$objectFullName.<init>:void()"
        )
        objectBlock.astChildren.isIdentifier.nameExact("tmp_obj_1").refsTo.l shouldBe List(tmpLocal)
        doesCall.argument.isIdentifier.nameExact("p").refsTo.l shouldBe inlineObject.parameter.nameExact("p").l
      }
    }

    "lower top level properties and captured references" in {
      withOxidizedCpg("""package demo
          |
          |val AGLOBAL = "A_GLOBAL"
          |val COUNT = 42
          |
          |fun f1() {
          |  println(AGLOBAL)
          |  println(COUNT)
          |}
          |""".stripMargin) { cpg =>
        val List(globalMethod) = cpg.method.nameExact("<global>").l: @unchecked
        val List(aGlobal)      = globalMethod.ast.isLocal.nameExact("AGLOBAL").l: @unchecked
        val List(countGlobal)  = globalMethod.ast.isLocal.nameExact("COUNT").l: @unchecked
        aGlobal.code shouldBe "AGLOBAL"
        aGlobal.typeFullName shouldBe "java.lang.String"
        aGlobal.closureBindingId shouldBe None
        countGlobal.typeFullName shouldBe "int"
        countGlobal.closureBindingId shouldBe None

        val List(f1)         = cpg.method.fullNameExact("demo.f1:void()").l: @unchecked
        val List(aCaptured)  = f1.ast.isLocal.nameExact("AGLOBAL").l: @unchecked
        val List(countLocal) = f1.ast.isLocal.nameExact("COUNT").l: @unchecked
        aCaptured.typeFullName shouldBe "java.lang.String"
        aCaptured.closureBindingId shouldBe Some("demo.f1:void():AGLOBAL")
        countLocal.typeFullName shouldBe "int"
        countLocal.closureBindingId shouldBe Some("demo.f1:void():COUNT")

        aGlobal.closureBinding.closureBindingId.l should contain("demo.f1:void():AGLOBAL")
        countGlobal.closureBinding.closureBindingId.l should contain("demo.f1:void():COUNT")
        cpg.methodRef.methodFullNameExact("demo.f1:void()").outE.collectAll[Capture].size shouldBe 2

        f1.ast.isIdentifier.nameExact("AGLOBAL").refsTo.l shouldBe List(aCaptured)
        f1.ast.isIdentifier.nameExact("COUNT").refsTo.l shouldBe List(countLocal)
      }
    }

    "lower top level object literal properties" in {
      withOxidizedCpg("""package demo
          |
          |interface SomeInterface {
          |  fun doSomething()
          |}
          |
          |val AN_OBJ = object : SomeInterface {
          |  override fun doSomething() {
          |    println("something")
          |  }
          |}
          |
          |fun useIt() {
          |  AN_OBJ.doSomething()
          |}
          |""".stripMargin) { cpg =>
        val objectFullName   = "demo.AN_OBJ.object$0"
        val List(objectType) = cpg.typeDecl.fullNameExact(objectFullName).l: @unchecked
        objectType.name shouldBe "anonymous_obj"
        objectType.inheritsFromTypeFullName shouldBe List("demo.SomeInterface")
        objectType.boundMethod.fullName.l should contain allOf (
          s"$objectFullName.<init>:void()",
          s"$objectFullName.doSomething:void()"
        )

        val List(globalMethod) = cpg.method.nameExact("<global>").l: @unchecked
        val List(globalLocal)  = globalMethod.ast.isLocal.nameExact("AN_OBJ").l: @unchecked
        globalLocal.typeFullName shouldBe objectFullName
        globalMethod.ast.isCall.nameExact(Operators.assignment).code("val AN_OBJ = object.*").size shouldBe 1
        globalMethod.ast.isCall
          .nameExact("<init>")
          .methodFullNameExact(s"$objectFullName.<init>:void()")
          .size shouldBe 1

        val List(useIt)    = cpg.method.fullNameExact("demo.useIt:void()").l: @unchecked
        val List(captured) = useIt.ast.isLocal.nameExact("AN_OBJ").l: @unchecked
        captured.typeFullName shouldBe objectFullName
        captured.closureBindingId shouldBe Some("demo.useIt:void():AN_OBJ")

        val List(objectCall) = useIt.ast.isCall.nameExact("doSomething").l: @unchecked
        objectCall.methodFullName shouldBe s"$objectFullName.doSomething:void()"
        objectCall.argument.isIdentifier.nameExact("AN_OBJ").refsTo.l shouldBe List(captured)
      }
    }

    "lower lambda and anonymous function arguments" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun transform(seed: Int): Int {
          |    val numbers = listOf(1, 2)
          |    val chosen = numbers.map { value ->
          |      val shifted: Int = value + seed
          |      shifted
          |    }
          |    val implicit = numbers.filter { it > seed }
          |    val direct = numbers.map({ n -> n + 1 })
          |    val anon = numbers.filter(fun(item: Int): Boolean { return item > 0 })
          |    return seed
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(transform) = cpg.method.fullNameExact("demo.Foo.transform:int(int)").l: @unchecked
        val lambdaMethods   = cpg.method.name(".*<lambda>.*").l.sortBy(_.fullName)
        val lambdaRefs      = cpg.methodRef.methodFullName(".*<lambda>.*").l.sortBy(_.methodFullName)

        lambdaMethods.size shouldBe 4
        lambdaRefs.map(_.methodFullName) shouldBe lambdaMethods.map(_.fullName)
        lambdaRefs.map(_.referencedMethod.fullName) should contain theSameElementsAs lambdaMethods.map(_.fullName)
        cpg.typeDecl.nameExact(Constants.LambdaTypeDeclName).size shouldBe 4
        lambdaMethods.foreach { method =>
          method.modifier.modifierType.l should contain allOf (ModifierTypes.VIRTUAL, ModifierTypes.LAMBDA)
        }

        val List(explicitRef)     = cpg.methodRef.methodFullName(".*<lambda>0.*").l: @unchecked
        val List(explicitCapture) = explicitRef._closureBindingViaCaptureOut.l: @unchecked
        explicitCapture.evaluationStrategy shouldBe EvaluationStrategies.BY_REFERENCE
        explicitCapture.closureBindingId shouldBe Some("demo.Foo.transform.<lambda>0.seed")
        explicitCapture._methodParameterInViaRefOut.name.l shouldBe List("seed")

        val List(explicitLambda) = cpg.method.fullName(".*<lambda>0.*").l: @unchecked
        explicitLambda.parameter.name.l shouldBe List("value")
        explicitLambda.ast.isLocal.nameExact("shifted").typeFullName.l shouldBe List("int")
        explicitLambda.ast.isCall
          .nameExact(Operators.addition)
          .argument
          .isIdentifier
          .nameExact("value")
          .refsTo
          .l shouldBe
          explicitLambda.parameter.nameExact("value").l
        explicitLambda.ast.isCall
          .nameExact(Operators.addition)
          .argument
          .isIdentifier
          .nameExact("seed")
          .refsTo
          .l shouldBe
          transform.parameter.nameExact("seed").l
        explicitLambda.ast.isReturn.astChildren.isIdentifier.nameExact("shifted").refsTo.l shouldBe
          explicitLambda.ast.isLocal.nameExact("shifted").l

        val List(implicitLambda) = cpg.method.fullName(".*<lambda>1.*").l: @unchecked
        implicitLambda.parameter.name.l shouldBe List("it")
        implicitLambda.ast.isCall
          .nameExact(Operators.greaterThan)
          .argument
          .isIdentifier
          .nameExact("it")
          .refsTo
          .l shouldBe
          implicitLambda.parameter.nameExact("it").l
        implicitLambda.ast.isCall
          .nameExact(Operators.greaterThan)
          .argument
          .isIdentifier
          .nameExact("seed")
          .refsTo
          .l shouldBe
          transform.parameter.nameExact("seed").l

        val List(implicitRef)     = cpg.methodRef.methodFullName(".*<lambda>1.*").l: @unchecked
        val List(implicitCapture) = implicitRef._closureBindingViaCaptureOut.l: @unchecked
        implicitCapture.evaluationStrategy shouldBe EvaluationStrategies.BY_REFERENCE
        implicitCapture.closureBindingId shouldBe Some("demo.Foo.transform.<lambda>1.seed")
        implicitCapture._methodParameterInViaRefOut.name.l shouldBe List("seed")

        val List(argumentLambda) = cpg.method.fullName(".*<lambda>2.*").l: @unchecked
        cpg.methodRef.methodFullName(".*<lambda>2.*")._closureBindingViaCaptureOut.size shouldBe 0
        argumentLambda.parameter.name.l shouldBe List("n")
        argumentLambda.ast.isReturn.astChildren.isCall
          .nameExact(Operators.addition)
          .argument
          .isIdentifier
          .nameExact("n")
          .refsTo
          .l shouldBe
          argumentLambda.parameter.nameExact("n").l

        val List(anonymousFunction) = cpg.method.fullName(".*<lambda>3.*").l: @unchecked
        cpg.methodRef.methodFullName(".*<lambda>3.*")._closureBindingViaCaptureOut.size shouldBe 0
        anonymousFunction.signature shouldBe "boolean(int)"
        anonymousFunction.parameter.name.l shouldBe List("item")
        anonymousFunction.ast.isReturn.astChildren.isCall
          .nameExact(Operators.greaterThan)
          .argument
          .isIdentifier
          .nameExact("item")
          .refsTo
          .l shouldBe
          anonymousFunction.parameter.nameExact("item").l
      }
    }

    "lower local destructuring declarations" in {
      withOxidizedCpg("""package demo
          |
          |data class PairBox(val name: String, val count: Int)
          |
          |fun makeAny(seed: Int): Any {
          |  return seed
          |}
          |
          |class Foo {
          |  fun destruct(pair: Any, seed: Int, box: PairBox): Any {
          |    val (first, second) = pair
          |    val (boxName, boxCount) = box
          |    val (kept, _) = pair
          |    val (callA, callB) = makeAny(seed)
          |    println(first)
          |    println(boxName)
          |    println(callA)
          |    return second
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(destruct)      = cpg.method.nameExact("destruct").l: @unchecked
        val List(firstLocal)    = destruct.ast.isLocal.nameExact("first").l: @unchecked
        val List(secondLocal)   = destruct.ast.isLocal.nameExact("second").l: @unchecked
        val List(boxNameLocal)  = destruct.ast.isLocal.nameExact("boxName").l: @unchecked
        val List(boxCountLocal) = destruct.ast.isLocal.nameExact("boxCount").l: @unchecked
        val List(keptLocal)     = destruct.ast.isLocal.nameExact("kept").l: @unchecked
        val List(callALocal)    = destruct.ast.isLocal.nameExact("callA").l: @unchecked
        val List(callBLocal)    = destruct.ast.isLocal.nameExact("callB").l: @unchecked
        val List(tmpLocal)      = destruct.ast.isLocal.nameExact("tmp_1").l: @unchecked

        destruct.ast.isLocal.nameExact("_").size shouldBe 0
        destruct.ast.isCall.nameExact(Operators.assignment).code.l should contain allOf (
          "first = pair.component1()",
          "second = pair.component2()",
          "boxName = box.component1()",
          "boxCount = box.component2()",
          "kept = pair.component1()",
          "tmp_1 = makeAny(seed)",
          "callA = tmp_1.component1()",
          "callB = tmp_1.component2()"
        )
        destruct.ast.isCall.codeExact("kept = pair.component2()").size shouldBe 0
        cpg.typeDecl.nameExact("PairBox").method.name("component.*").fullName.l shouldBe
          List("demo.PairBox.component1:java.lang.String()", "demo.PairBox.component2:int()")
        boxNameLocal.typeFullName shouldBe "java.lang.String"
        boxCountLocal.typeFullName shouldBe "int"

        destruct.ast.isCall
          .codeExact("first = pair.component1()")
          .argument
          .isIdentifier
          .nameExact("first")
          .refsTo
          .l shouldBe
          List(firstLocal)
        destruct.ast.isCall
          .codeExact("second = pair.component2()")
          .argument
          .isIdentifier
          .nameExact("second")
          .refsTo
          .l shouldBe
          List(secondLocal)
        destruct.ast.isCall
          .codeExact("boxName = box.component1()")
          .argument
          .isIdentifier
          .nameExact("boxName")
          .refsTo
          .l shouldBe
          List(boxNameLocal)
        destruct.ast.isCall
          .codeExact("boxCount = box.component2()")
          .argument
          .isIdentifier
          .nameExact("boxCount")
          .refsTo
          .l shouldBe
          List(boxCountLocal)
        destruct.ast.isCall
          .codeExact("kept = pair.component1()")
          .argument
          .isIdentifier
          .nameExact("kept")
          .refsTo
          .l shouldBe
          List(keptLocal)
        destruct.ast.isCall
          .codeExact("callA = tmp_1.component1()")
          .argument
          .isIdentifier
          .nameExact("callA")
          .refsTo
          .l shouldBe
          List(callALocal)
        destruct.ast.isCall
          .codeExact("callB = tmp_1.component2()")
          .argument
          .isIdentifier
          .nameExact("callB")
          .refsTo
          .l shouldBe
          List(callBLocal)

        destruct.ast.isCall
          .codeExact("first = pair.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .argument
          .isIdentifier
          .nameExact("pair")
          .refsTo
          .l shouldBe
          destruct.parameter.nameExact("pair").l
        destruct.ast.isCall
          .codeExact("boxName = box.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .methodFullName
          .l shouldBe
          List("demo.PairBox.component1:java.lang.String()")
        destruct.ast.isCall
          .codeExact("boxCount = box.component2()")
          .argument
          .isCall
          .nameExact("component2")
          .signature
          .l shouldBe
          List("int()")
        destruct.ast.isCall
          .codeExact("boxName = box.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .argument
          .isIdentifier
          .nameExact("box")
          .refsTo
          .l shouldBe
          destruct.parameter.nameExact("box").l
        destruct.ast.isCall
          .codeExact("tmp_1 = makeAny(seed)")
          .argument
          .isIdentifier
          .nameExact("tmp_1")
          .refsTo
          .l shouldBe
          List(tmpLocal)
        destruct.ast.isCall
          .codeExact("callA = tmp_1.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .argument
          .isIdentifier
          .nameExact("tmp_1")
          .refsTo
          .l shouldBe
          List(tmpLocal)
        destruct.ast.isReturn.astChildren.isIdentifier.nameExact("second").refsTo.l shouldBe List(secondLocal)
      }
    }

    "lower lambda destructuring parameters" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun consume(entries: Any): Any {
          |    entries.forEach { (key, value) ->
          |      println(key)
          |      println(value)
          |    }
          |    return entries
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(lambda)       = cpg.method.fullName(".*<lambda>0.*").l: @unchecked
        val syntheticParamName = s"${Constants.DestructedParamNamePrefix}1"

        lambda.parameter.name.l shouldBe List(syntheticParamName)
        lambda.ast.isLocal.name.l should contain allOf ("it", "tmp_1", "key", "value")
        lambda.ast.isCall.nameExact(Operators.assignment).code.l should contain allOf (
          "tmp_1 = it",
          "key = tmp_1.component1()",
          "value = tmp_1.component2()"
        )

        val List(tmpLocal)   = lambda.ast.isLocal.nameExact("tmp_1").l: @unchecked
        val List(keyLocal)   = lambda.ast.isLocal.nameExact("key").l: @unchecked
        val List(valueLocal) = lambda.ast.isLocal.nameExact("value").l: @unchecked
        lambda.ast.isCall.codeExact("tmp_1 = it").argument.isIdentifier.nameExact("it").refsTo.l shouldBe
          lambda.parameter.nameExact(syntheticParamName).l
        lambda.ast.isCall.codeExact("key = tmp_1.component1()").argument.isIdentifier.nameExact("key").refsTo.l shouldBe
          List(keyLocal)
        lambda.ast.isCall
          .codeExact("key = tmp_1.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .argument
          .isIdentifier
          .nameExact("tmp_1")
          .refsTo
          .l shouldBe
          List(tmpLocal)
        lambda.ast.isCall
          .codeExact("value = tmp_1.component2()")
          .argument
          .isIdentifier
          .nameExact("value")
          .refsTo
          .l shouldBe
          List(valueLocal)
        lambda.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("key").refsTo.l shouldBe List(keyLocal)
        lambda.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("value").refsTo.l shouldBe List(
          valueLocal
        )
      }
    }

    "lower for loop destructuring parameters" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun loop(entries: Any): Any {
          |    for ((key, value) in entries) {
          |      println(key)
          |      println(value)
          |    }
          |    for ((kept, _) in entries) {
          |      println(kept)
          |    }
          |    for ((_, tail) in entries) {
          |      println(tail)
          |    }
          |    return entries
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(loop) = cpg.method.nameExact("loop").l: @unchecked
        val List(firstFor, secondFor, thirdFor) =
          loop.controlStructure
            .controlStructureTypeExact(ControlStructureTypes.WHILE)
            .l
            .filter(_.code.startsWith("for")): @unchecked

        val List(firstIterator) = loop.ast.isLocal.nameExact("iterator_1").l: @unchecked
        firstFor.condition.isCall.nameExact("hasNext").methodFullName.l shouldBe
          List("kotlin.collections.Iterator.hasNext:boolean()")
        firstFor.condition.isCall.nameExact("hasNext").argument.isIdentifier.nameExact("iterator_1").refsTo.l shouldBe
          List(firstIterator)
        loop.ast.isCall
          .codeExact("iterator_1 = entries.iterator()")
          .argument
          .isCall
          .nameExact("iterator")
          .argument
          .isIdentifier
          .nameExact("entries")
          .refsTo
          .l shouldBe
          loop.parameter.nameExact("entries").l

        val List(firstTmp)   = firstFor.ast.isLocal.nameExact("tmp_1").l: @unchecked
        val List(keyLocal)   = firstFor.ast.isLocal.nameExact("key").l: @unchecked
        val List(valueLocal) = firstFor.ast.isLocal.nameExact("value").l: @unchecked
        firstFor.ast.isCall.nameExact(Operators.assignment).code.l should contain allOf (
          "tmp_1 = iterator_1.next()",
          "key = tmp_1.component1()",
          "value = tmp_1.component2()"
        )
        firstFor.ast.isCall
          .codeExact("tmp_1 = iterator_1.next()")
          .argument
          .isCall
          .nameExact("next")
          .methodFullName
          .l shouldBe
          List("kotlin.collections.Iterator.next:java.lang.Object()")
        firstFor.ast.isCall
          .codeExact("key = tmp_1.component1()")
          .argument
          .isIdentifier
          .nameExact("key")
          .refsTo
          .l shouldBe
          List(keyLocal)
        firstFor.ast.isCall
          .codeExact("value = tmp_1.component2()")
          .argument
          .isIdentifier
          .nameExact("value")
          .refsTo
          .l shouldBe
          List(valueLocal)
        firstFor.ast.isCall
          .codeExact("key = tmp_1.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .argument
          .isIdentifier
          .nameExact("tmp_1")
          .refsTo
          .l shouldBe
          List(firstTmp)
        firstFor.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("key").refsTo.l shouldBe
          List(keyLocal)
        firstFor.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("value").refsTo.l shouldBe
          List(valueLocal)

        val List(secondTmp) = secondFor.ast.isLocal.nameExact("tmp_2").l: @unchecked
        val List(keptLocal) = secondFor.ast.isLocal.nameExact("kept").l: @unchecked
        val List(thirdTmp)  = thirdFor.ast.isLocal.nameExact("tmp_3").l: @unchecked
        val List(tailLocal) = thirdFor.ast.isLocal.nameExact("tail").l: @unchecked
        secondFor.ast.isLocal.nameExact("_").size shouldBe 0
        thirdFor.ast.isLocal.nameExact("_").size shouldBe 0
        secondFor.ast.isCall
          .codeExact("kept = tmp_2.component1()")
          .argument
          .isIdentifier
          .nameExact("kept")
          .refsTo
          .l shouldBe
          List(keptLocal)
        secondFor.ast.isCall.codeExact("kept = tmp_2.component2()").size shouldBe 0
        secondFor.ast.isCall
          .codeExact("kept = tmp_2.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .argument
          .isIdentifier
          .nameExact("tmp_2")
          .refsTo
          .l shouldBe
          List(secondTmp)
        thirdFor.ast.isCall
          .codeExact("tail = tmp_3.component2()")
          .argument
          .isIdentifier
          .nameExact("tail")
          .refsTo
          .l shouldBe
          List(tailLocal)
        thirdFor.ast.isCall.codeExact("tail = tmp_3.component1()").size shouldBe 0
        thirdFor.ast.isCall
          .codeExact("tail = tmp_3.component2()")
          .argument
          .isCall
          .nameExact("component2")
          .argument
          .isIdentifier
          .nameExact("tmp_3")
          .refsTo
          .l shouldBe
          List(thirdTmp)
        secondFor.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("kept").refsTo.l shouldBe
          List(keptLocal)
        thirdFor.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("tail").refsTo.l shouldBe
          List(tailLocal)
      }
    }

    "resolve data class component calls in for loop destructuring" in {
      withOxidizedCpg("""package demo
          |
          |data class Entry(val key: String, val value: Int)
          |
          |class Foo {
          |  fun loop(): Any {
          |    val first = Entry("a", 1)
          |    val second = Entry("b", 2)
          |    val entries = listOf(first, second)
          |    for ((key, value) in entries) {
          |      println(key)
          |      println(value)
          |    }
          |    return entries
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(loop) = cpg.method.nameExact("loop").l: @unchecked
        val List(forLoop) =
          loop.controlStructure
            .controlStructureTypeExact(ControlStructureTypes.WHILE)
            .l
            .filter(_.code.startsWith("for")): @unchecked
        val List(tmpLocal)   = forLoop.ast.isLocal.nameExact("tmp_1").l: @unchecked
        val List(keyLocal)   = forLoop.ast.isLocal.nameExact("key").l: @unchecked
        val List(valueLocal) = forLoop.ast.isLocal.nameExact("value").l: @unchecked

        tmpLocal.typeFullName shouldBe "demo.Entry"
        keyLocal.typeFullName shouldBe "java.lang.String"
        valueLocal.typeFullName shouldBe "int"
        cpg.typeDecl.nameExact("Entry").method.name("component.*").fullName.l shouldBe
          List("demo.Entry.component1:java.lang.String()", "demo.Entry.component2:int()")

        forLoop.ast.isCall
          .codeExact("key = tmp_1.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .methodFullName
          .l shouldBe
          List("demo.Entry.component1:java.lang.String()")
        forLoop.ast.isCall
          .codeExact("key = tmp_1.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .signature
          .l shouldBe
          List("java.lang.String()")
        forLoop.ast.isCall
          .codeExact("value = tmp_1.component2()")
          .argument
          .isCall
          .nameExact("component2")
          .methodFullName
          .l shouldBe
          List("demo.Entry.component2:int()")
        forLoop.ast.isCall
          .codeExact("value = tmp_1.component2()")
          .argument
          .isCall
          .nameExact("component2")
          .signature
          .l shouldBe
          List("int()")
        forLoop.ast.isCall
          .codeExact("key = tmp_1.component1()")
          .argument
          .isCall
          .nameExact("component1")
          .argument
          .isIdentifier
          .nameExact("tmp_1")
          .refsTo
          .l shouldBe
          List(tmpLocal)
      }
    }

    "lower try catch finally and jump control structures" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun control(x: Int): Int {
          |    loop@ for (i in 0..x) {
          |      try {
          |        if (i > 10) break@loop
          |        if (i == 3) continue
          |        throw RuntimeException("boom")
          |      } catch (e: Exception) {
          |        println(e)
          |      } finally {
          |        println("done")
          |      }
          |    }
          |    val out = try {
          |      x
          |    } catch (e: Exception) {
          |      0
          |    }
          |    listOf(1).forEach { return@forEach }
          |    return out
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(control) = cpg.method.nameExact("control").l: @unchecked

        val List(labelTarget) = control.ast.collectAll[JumpTarget].nameExact("loop").l: @unchecked
        labelTarget.code shouldBe "loop@"

        val List(tryNode) = control.controlStructure.controlStructureTypeExact(ControlStructureTypes.TRY).l: @unchecked
        tryNode.tryBodyOut.ast.isCall.nameExact("<init>").code.l shouldBe List("""RuntimeException("boom")""")
        tryNode.catchBodyOut.code.l shouldBe List("catch (e: Exception) {\n        println(e)\n      }")
        tryNode.catchBodyOut.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("e").size shouldBe 1
        tryNode.finallyBodyOut.ast.isCall.nameExact("println").argument.isLiteral.codeExact("\"done\"").size shouldBe 1

        val List(breakNode) =
          control.controlStructure.controlStructureTypeExact(ControlStructureTypes.BREAK).l: @unchecked
        breakNode.code shouldBe "break@loop"
        breakNode._jumpArgumentOut.collectAll[JumpLabel].code.l shouldBe List("loop")

        val List(continueNode) =
          control.controlStructure.controlStructureTypeExact(ControlStructureTypes.CONTINUE).l: @unchecked
        continueNode.code shouldBe "continue"
        continueNode._jumpArgumentOut.size shouldBe 0

        val List(throwNode) =
          control.controlStructure.controlStructureTypeExact(ControlStructureTypes.THROW).l: @unchecked
        throwNode.code shouldBe """throw RuntimeException("boom")"""
        val List(throwBlock) = throwNode.argumentOut.isBlock.l: @unchecked
        throwBlock.astChildren.isCall.sortBy(_.name).name.l shouldBe List("<init>", Operators.assignment)

        val List(allocAssignment) = throwBlock.astChildren.isCall.nameExact(Operators.assignment).l: @unchecked
        allocAssignment.code shouldBe "tmp_1 = <alloc>"
        allocAssignment.argument.isCall.nameExact(Operators.alloc).typeFullName.l shouldBe List(
          "java.lang.RuntimeException"
        )

        val List(initCall) = throwBlock.astChildren.isCall.nameExact("<init>").l: @unchecked
        initCall.code shouldBe """RuntimeException("boom")"""
        initCall.methodFullName shouldBe "java.lang.RuntimeException.<init>:void(java.lang.String)"
        initCall.signature shouldBe "void(java.lang.String)"
        initCall.argument.isIdentifier.nameExact("tmp_1").refsTo.l shouldBe throwBlock.astChildren.isLocal
          .nameExact("tmp_1")
          .l

        val List(tryCall) = control.ast.isCall.nameExact(Operators.tryCatch).l: @unchecked
        tryCall.code should include("try")
        tryCall.argument.isBlock.size shouldBe 2
        tryCall.argument.isBlock.argumentIndex(1).astChildren.isIdentifier.nameExact("x").refsTo.l shouldBe
          control.parameter.nameExact("x").l
        tryCall.argument.isBlock.argumentIndex(2).astChildren.isLiteral.codeExact("0").size shouldBe 1

        val List(lambdaReturn) =
          cpg.method.fullName(".*<lambda>.*").ast.isReturn.codeExact("return@forEach").l: @unchecked
        lambdaReturn._jumpArgumentOut.collectAll[JumpLabel].code.l shouldBe List("forEach")
      }
    }

    "resolve string conversion calls inside try expressions" in {
      withOxidizedCpg("""package demo
          |
          |fun doSomething(x: Int): Int {
          |  val r = "41414141"
          |  val out = try {
          |    x
          |  } catch (e: Exception) {
          |    r.toInt()
          |  }
          |  return out
          |}
          |""".stripMargin) { cpg =>
        val List(doSomething) = cpg.method.fullNameExact("demo.doSomething:int(int)").l: @unchecked
        val List(outLocal)    = doSomething.ast.isLocal.nameExact("out").l: @unchecked
        outLocal.typeFullName shouldBe "int"

        val List(tryCall) = doSomething.ast.isCall.nameExact(Operators.tryCatch).l: @unchecked
        tryCall.typeFullName shouldBe "int"
        tryCall.argument.isBlock.argumentIndex(1).astChildren.isIdentifier.nameExact("x").refsTo.l shouldBe
          doSomething.parameter.nameExact("x").l

        val List(toIntCall) = tryCall.argument.isBlock.argumentIndex(2).astChildren.isCall.nameExact("toInt").l
        toIntCall.code shouldBe "r.toInt()"
        toIntCall.methodFullName shouldBe "kotlin.text.toInt:int(java.lang.String)"
        toIntCall.signature shouldBe "int(java.lang.String)"
        toIntCall.typeFullName shouldBe "int"
        toIntCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        toIntCall.argument.isIdentifier.nameExact("r").refsTo.l shouldBe doSomething.ast.isLocal.nameExact("r").l
      }
    }

    "lower if while and for control structures" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun control(a: Int, b: Int): Int {
          |    var total: Int = 0
          |    if (a > b) {
          |      total = a
          |    } else {
          |      total = b
          |    }
          |    while (total < 10) {
          |      total = total + 1
          |    }
          |    for (i in 0..total) {
          |      println(i)
          |    }
          |    return total
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(control)    = cpg.method.fullNameExact("demo.Foo.control:int(int,int)").l: @unchecked
        val List(totalLocal) = control.ast.isLocal.nameExact("total").l: @unchecked

        val List(ifNode) =
          control.controlStructure.controlStructureTypeExact(ControlStructureTypes.IF).l: @unchecked
        ifNode.condition.isCall.nameExact(Operators.greaterThan).argument.isIdentifier.nameExact("a").refsTo.l shouldBe
          control.parameter.nameExact("a").l
        ifNode.condition.isCall.nameExact(Operators.greaterThan).argument.isIdentifier.nameExact("b").refsTo.l shouldBe
          control.parameter.nameExact("b").l
        ifNode.ast.isCall.nameExact(Operators.assignment).code.l should contain allOf ("total = a", "total = b")
        ifNode.ast.isCall
          .nameExact(Operators.assignment)
          .argument
          .isIdentifier
          .nameExact("total")
          .refsTo
          .l
          .distinct shouldBe
          List(totalLocal)

        val List(whileNode) =
          control.controlStructure
            .controlStructureTypeExact(ControlStructureTypes.WHILE)
            .l
            .filter(_.code.startsWith("while")): @unchecked
        whileNode.condition.isCall
          .nameExact(Operators.lessThan)
          .argument
          .isIdentifier
          .nameExact("total")
          .refsTo
          .l shouldBe
          List(totalLocal)
        whileNode.ast.isCall.nameExact(Operators.addition).argument.isIdentifier.nameExact("total").refsTo.l shouldBe
          List(totalLocal)

        val List(loweredFor) =
          control.controlStructure
            .controlStructureTypeExact(ControlStructureTypes.WHILE)
            .l
            .filter(_.code.startsWith("for")): @unchecked
        val List(loopLocal)     = loweredFor.ast.isLocal.nameExact("i").l: @unchecked
        val List(iteratorLocal) = control.ast.isLocal.nameExact("iterator_1").l: @unchecked
        loweredFor.condition.isCall.nameExact("hasNext").argument.isIdentifier.nameExact("iterator_1").refsTo.l shouldBe
          List(iteratorLocal)
        control.ast.isCall
          .codeExact("iterator_1 = 0..total.iterator()")
          .argument
          .isCall
          .nameExact("iterator")
          .argument
          .isCall
          .nameExact(Operators.range)
          .argument
          .isIdentifier
          .nameExact("total")
          .refsTo
          .l shouldBe
          List(totalLocal)
        loweredFor.ast.isCall.codeExact("i = iterator_1.next()").size shouldBe 1
        loweredFor.ast.isCall.nameExact("println").argument.isIdentifier.nameExact("i").refsTo.l shouldBe List(
          loopLocal
        )
      }
    }

    "lower when expressions and boolean null operators" in {
      withOxidizedCpg("""package demo
          |
          |class Foo {
          |  fun classify(x: Int, name: String?): String {
          |    var out: String = ""
          |    when (x) {
          |      0 -> out = "zero"
          |      1, 2 -> out = "small"
          |      else -> out = name ?: "other"
          |    }
          |    if (x > 0 && name != null || !out.isEmpty()) {
          |      out = name!!
          |    }
          |    return out
          |  }
          |}
          |""".stripMargin) { cpg =>
        val List(classify) =
          cpg.method.fullNameExact("demo.Foo.classify:java.lang.String(int,java.lang.String)").l: @unchecked
        val List(outLocal) = classify.ast.isLocal.nameExact("out").l: @unchecked

        val List(switchNode) =
          classify.controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).l: @unchecked
        switchNode.code shouldBe "when(x)"
        switchNode.condition.isIdentifier.nameExact("x").refsTo.l shouldBe classify.parameter.nameExact("x").l
        switchNode.ast.isLiteral.code.l should contain allElementsOf List("0", "1", "2")
        switchNode.ast.collectAll[Unknown].l shouldBe empty
        switchNode.ast.isCall.nameExact(Operators.assignment).code.l should contain allOf (
          "out = \"zero\"",
          "out = \"small\"",
          "out = name ?: \"other\""
        )
        switchNode.ast.isCall
          .nameExact(Operators.assignment)
          .argument
          .isIdentifier
          .nameExact("out")
          .refsTo
          .l
          .distinct shouldBe
          List(outLocal)

        val List(elvisCall) = switchNode.ast.isCall.nameExact(Operators.elvis).l: @unchecked
        elvisCall.argument.isIdentifier.nameExact("name").refsTo.l shouldBe classify.parameter.nameExact("name").l
        elvisCall.argument.isLiteral.code.l should contain("\"other\"")

        val List(logicalOrCall)  = classify.ast.isCall.nameExact(Operators.logicalOr).l: @unchecked
        val List(logicalAndCall) = logicalOrCall.argument.isCall.nameExact(Operators.logicalAnd).l: @unchecked
        val List(logicalNotCall) = logicalOrCall.argument.isCall.nameExact(Operators.logicalNot).l: @unchecked
        logicalAndCall.argument.isCall
          .nameExact(Operators.greaterThan)
          .argument
          .isIdentifier
          .nameExact("x")
          .refsTo
          .l shouldBe
          classify.parameter.nameExact("x").l

        val List(notEqualsCall) = logicalAndCall.argument.isCall.nameExact(Operators.notEquals).l: @unchecked
        notEqualsCall.argument.isIdentifier.nameExact("name").refsTo.l shouldBe classify.parameter.nameExact("name").l
        notEqualsCall.argument.isLiteral.code.l shouldBe List("null")
        notEqualsCall.argument.isLiteral.typeFullName.l shouldBe List("null")

        logicalNotCall.argument.isCall.nameExact("isEmpty").size shouldBe 1
        val List(notNullAssertCall) = classify.ast.isCall.nameExact(Operators.notNullAssert).l: @unchecked
        notNullAssertCall.argument.isIdentifier.nameExact("name").refsTo.l shouldBe classify.parameter
          .nameExact("name")
          .l
      }
    }
  }

  private def withOxidizedCpg(code: String)(test: io.shiftleft.codepropertygraph.generated.Cpg => Unit): Unit = {
    FileUtil.usingTemporaryDirectory("oxidizedKotlinInput") { inputDir =>
      writeFile(inputDir / "demo" / "Sample.kt", code)

      FileUtil.usingTemporaryDirectory("oxidizedKotlinOut") { outputDir =>
        val config = Config(parserBackend = KotlinParserBackend.Oxidized)
          .withInputPath(inputDir.toString)
          .withOutputPath(outputDir.toString)
          .withDisableFileContent(false)
        val cpg = new Kotlin2Cpg().createCpg(config).get
        try test(cpg)
        finally cpg.close()
      }
    }
  }

  private def writeFile(path: Path, content: String): Unit = {
    path.createWithParentsIfNotExists(createParents = true)
    Files.writeString(path, content)
  }
}
