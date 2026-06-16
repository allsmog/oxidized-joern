package io.joern.c2cpg.compat

import io.joern.c2cpg.{C2Cpg, Config}
import io.joern.c2cpg.astcreation.Defines
import io.joern.c2cpg.parser.ParserBackend
import io.joern.c2cpg.testfixtures.C2CpgSuite
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, ModifierTypes, Operators}
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

    "capture object-like macros and configured defines as inlined zero-argument calls" in {
      val cpg = code("""
          |#define SIZE 4
          |
          |int configured() {
          |  return FROM_DB;
          |}
          |int source_macro() {
          |  return SIZE + 1;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized, defines = Set("FROM_DB=7")))

      cpg.method.nameExact("SIZE").signature.l shouldBe List("int(0)")
      cpg.method.nameExact("SIZE").fullName.l shouldBe List("Test0.c:SIZE:int(0)")
      cpg.method.nameExact("FROM_DB").signature.l shouldBe List("int(0)")
      cpg.method.nameExact("FROM_DB").fullName.l shouldBe List("Test0.c:FROM_DB:int(0)")

      inside(cpg.call.nameExact("SIZE").l) { case List(sizeCall) =>
        sizeCall.code shouldBe "SIZE"
        sizeCall.methodFullName shouldBe "Test0.c:SIZE:int(0)"
        sizeCall.signature shouldBe "int(0)"
        sizeCall.dispatchType shouldBe DispatchTypes.INLINED
        sizeCall.argument.l shouldBe Nil
      }
      inside(cpg.call.nameExact("FROM_DB").l) { case List(fromDbCall) =>
        fromDbCall.code shouldBe "FROM_DB"
        fromDbCall.methodFullName shouldBe "Test0.c:FROM_DB:int(0)"
        fromDbCall.signature shouldBe "int(0)"
        fromDbCall.dispatchType shouldBe DispatchTypes.INLINED
        fromDbCall.argument.l shouldBe Nil
      }
    }

    "capture basic C++ namespaces, classes, and methods" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(int& seed) : value(seed) {}
          |  ~Widget();
          |  int value;
          |  int get() { return normalize(); }
          |  int normalize() { return pick(value); }
          |  int pick(int seed) { return seed; }
          |  int pick(Widget& other) { return other.value; }
          |  int size();
          |};
          |int normalize() { return 99; }
          |int convert(int value) { return value; }
          |int convert(Widget widget) { return widget.get(); }
          |int make() { return 1; }
          |}
          |Core::Widget::Widget() : value(1) {}
          |Core::Widget::~Widget() {}
          |int Core::Widget::outside() { return 2; }
          |int use() {
          |  Core::Widget widget(7);
          |  return Core::make() + convert(1) + convert(widget) + widget.get() + widget.outside();
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.namespaceBlock.nameExact("Core").fullName.l shouldBe List("Test0.cpp:Core")
      cpg.typeDecl.fullNameExact("Core.Widget").fullName.l shouldBe List("Core.Widget")
      cpg.typeDecl.fullNameExact("Core.Widget").filename.l shouldBe List("Test0.cpp")
      cpg.typeDecl.fullNameExact("Core.Widget").member.name.l shouldBe List("value")
      cpg.typeDecl.fullNameExact("Core.Widget").member.typeFullName.l shouldBe List("int")
      cpg.typeDecl.fullNameExact("Core.Widget").method.name.l.sorted shouldBe
        List("Widget", "Widget", "get", "normalize", "outside", "pick", "pick", "size", "~Widget")
      cpg.method.nameExact("Widget").internal.fullName.l.sorted shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void(int&)")
      cpg.method.nameExact("Widget").external.l shouldBe Nil
      cpg.method.nameExact("Widget").isConstructor.fullName.l.sorted shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void(int&)")
      cpg.method.fullNameExact("Core.Widget.Widget:void(int&)").call.nameExact(Operators.assignment).code.l shouldBe
        List("this->value = seed")
      cpg.method.fullNameExact("Core.Widget.Widget:void(int&)").call.nameExact(Operators.assignment).argument.code.l shouldBe
        List("this->value", "seed")
      cpg.method.fullNameExact("Core.Widget.Widget:void()").call.nameExact(Operators.assignment).code.l shouldBe
        List("this->value = 1")
      cpg.method.fullNameExact("Core.Widget.Widget:void()").call.nameExact(Operators.assignment).argument.code.l shouldBe
        List("this->value", "1")
      cpg.method.nameExact("~Widget").internal.fullName.l shouldBe List("Core.Widget.~Widget:void()")
      cpg.method.nameExact("get").internal.fullName.l shouldBe List("Core.Widget.get:int()")
      cpg.method.nameExact("normalize").fullName.l.sorted shouldBe List("Core.Widget.normalize:int()", "Core.normalize:int()")
      cpg.method.nameExact("size").external.fullName.l shouldBe List("Core.Widget.size:int()")
      cpg.method.nameExact("outside").internal.fullName.l shouldBe List("Core.Widget.outside:int()")
      cpg.method.nameExact("make").fullName.l shouldBe List("Core.make:int()")
      cpg.method.nameExact("use").local.nameExact("widget").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.fullNameExact("Core.Widget.get:int()").parameter.name.l shouldBe List("this")
      cpg.method.fullNameExact("Core.Widget.get:int()").parameter.index.l shouldBe List(0)
      cpg.method.fullNameExact("Core.Widget.get:int()").parameter.typeFullName.l shouldBe List("Core.Widget*")
      cpg.method.fullNameExact("Core.Widget.get:int()").ast.isReturn.code.l shouldBe List("return normalize()")
      cpg.method.fullNameExact("Core.Widget.get:int()").call.nameExact("normalize").methodFullName.l shouldBe
        List("Core.Widget.normalize:int()")
      cpg.method.fullNameExact("Core.Widget.normalize:int()").call.nameExact("pick").methodFullName.l shouldBe
        List("Core.Widget.pick:int(int)")
      inside(cpg.method.fullNameExact("Core.Widget.normalize:int()").call.nameExact(Operators.indirectFieldAccess).l) {
        case List(fieldAccess) =>
        fieldAccess.code shouldBe "this->value"
        fieldAccess.typeFullName shouldBe "int"
        fieldAccess.argument.code.l shouldBe List("this", "value")
      }
      cpg.method.nameExact("use").call.nameExact("Widget").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(int&)")
      cpg.method.nameExact("use").call.nameExact("make").methodFullName.l shouldBe List("Core.make:int()")
      cpg.method.nameExact("use").call.codeExact("convert(1)").methodFullName.l shouldBe List("Core.convert:int(int)")
      cpg.method.nameExact("use").call.codeExact("convert(widget)").methodFullName.l shouldBe
        List("Core.convert:int(Widget)")
      cpg.method.nameExact("use").call.nameExact("get").methodFullName.l shouldBe List("Core.Widget.get:int()")
      cpg.method.nameExact("use").call.nameExact("outside").methodFullName.l shouldBe List("Core.Widget.outside:int()")
    }

    "capture C++ new and delete expressions" in {
      val cpg = code(
        """
          |int *allocate(int n) {
          |  int *arr = new int[n];
          |  delete[] arr;
          |  return arr;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("allocate").local.nameExact("arr").typeFullName.l shouldBe List("int*")
      inside(cpg.method.nameExact("allocate").call.nameExact(Operators.alloc).l) { case List(alloc) =>
        alloc.code shouldBe "new int[n]"
        alloc.methodFullName shouldBe Operators.alloc
        alloc.argument.code.l shouldBe List("int", "n")
      }
      inside(cpg.method.nameExact("allocate").call.nameExact(Operators.delete).l) { case List(delete) =>
        delete.code shouldBe "delete[] arr"
        delete.methodFullName shouldBe Operators.delete
        delete.argument.code.l shouldBe List("arr")
      }
    }

    "capture enum variant initializers through a static initializer" in {
      val cpg = code("""
          |enum Mode { MODE_A = 1, MODE_B = 2, MODE_C };
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val mode = cpg.typeDecl.nameExact("Mode").head
      mode.member.name.l shouldBe List("MODE_A", "MODE_B", "MODE_C")
      mode.member.typeFullName.l shouldBe List("int", "int", "int")

      inside(mode.astChildren.isMethod.nameExact(io.joern.x2cpg.Defines.StaticInitMethodName).l) { case List(clinit) =>
        clinit.fullName shouldBe s"Mode.${io.joern.x2cpg.Defines.StaticInitMethodName}:Mode()"
        clinit.modifier.modifierType.l.sorted shouldBe List(ModifierTypes.CONSTRUCTOR, ModifierTypes.STATIC).sorted
        clinit.local.name.l shouldBe List("MODE_A", "MODE_B")
        clinit.call.nameExact(Operators.assignment).code.l.sorted shouldBe List("MODE_A = 1", "MODE_B = 2")
        clinit.call.nameExact(Operators.assignment).argument.code.l shouldBe List("MODE_A", "1", "MODE_B", "2")
      }
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

    "capture include imports and dependencies" in {
      val cpg = code("""
          |#include "./folder/sub/foo.h"
          |#include <io.h>
          |int value;
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.dependency.l) { case List(fooDep, ioDep) =>
        fooDep.name shouldBe "./folder/sub/foo.h"
        fooDep.version shouldBe "include"
        fooDep.dependencyGroupId shouldBe Option("./folder/sub/foo.h")
        ioDep.name shouldBe "io.h"
        ioDep.version shouldBe "include"
        ioDep.dependencyGroupId shouldBe Option("io.h")
      }
      inside(cpg.imports.l) { case List(fooImport, ioImport) =>
        fooImport.code shouldBe "#include \"./folder/sub/foo.h\""
        fooImport.importedEntity shouldBe Option("./folder/sub/foo.h")
        fooImport.importedAs shouldBe Option("./folder/sub/foo.h")
        fooImport._dependencyViaImportsOut.name.l shouldBe List("./folder/sub/foo.h")
        ioImport.code shouldBe "#include <io.h>"
        ioImport.importedEntity shouldBe Option("io.h")
        ioImport.importedAs shouldBe Option("io.h")
        ioImport._dependencyViaImportsOut.name.l shouldBe List("io.h")
      }
    }

    "preserve function pointer type names from declarators" in {
      val cpg = code("""
          |struct Ops {
          |  int (*open)(int);
          |};
          |typedef int (*Callback)(int);
          |int (*foo)(int, int) = { 0 };
          |int (*bar[])(int, int) = { 0 };
          |int invoke(int (*cb)(int), int value) {
          |  struct Ops ops;
          |  int (*local)(int) = cb;
          |  local(value);
          |  ops.open(value);
          |  return cb(value);
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.typeDecl.nameExact("Ops").member.nameExact("open").typeFullName.l shouldBe List("int(*)(int)")
      cpg.typeDecl.nameExact("Callback").aliasTypeFullName.l shouldBe List("int(*)(int)")
      cpg.local.nameExact("foo").typeFullName.l shouldBe List("int(*)(int,int)")
      cpg.local.nameExact("bar").typeFullName.l shouldBe List("int(*[])(int,int)")
      cpg.method.nameExact("invoke").signature.l shouldBe List("int(int(*)(int),int)")
      cpg.method.nameExact("invoke").parameter.nameExact("cb").typeFullName.l shouldBe List("int(*)(int)")
      cpg.method.nameExact("invoke").local.nameExact("local").typeFullName.l shouldBe List("int(*)(int)")

      val pointerCalls = cpg.method.nameExact("invoke").call.nameExact(Defines.OperatorPointerCall).l
      pointerCalls.map(_.code).sorted shouldBe List("cb(value)", "local(value)", "ops.open(value)")
      pointerCalls.foreach { call =>
        call.methodFullName shouldBe Defines.OperatorPointerCall
        call.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        call.typeFullName shouldBe "int"
        call.argument.code.l shouldBe List("value")
        call.receiver.argumentIndex.l shouldBe List(-1)
      }
      val cbPointerCall    = pointerCalls.collectFirst { case call if call.code == "cb(value)" => call }.get
      val localPointerCall = pointerCalls.collectFirst { case call if call.code == "local(value)" => call }.get
      val fieldPointerCall = pointerCalls.collectFirst { case call if call.code == "ops.open(value)" => call }.get
      cbPointerCall.receiver.code.l shouldBe List("cb")
      localPointerCall.receiver.code.l shouldBe List("local")
      fieldPointerCall.receiver.code.l shouldBe List("ops.open")
    }

    "capture nested aggregate declarations, unions, and bitfields" in {
      val cpg = code("""
          |struct Outer {
          |  int flags:3;
          |  struct Inner {
          |    int a;
          |    union Choice {
          |      int i;
          |      char c;
          |    };
          |  };
          |  union Storage {
          |    int x;
          |    char y;
          |  };
          |  union {
          |    long promoted;
          |  };
          |  struct {
          |    int inline_x;
          |  } inline_field;
          |};
          |union Top {
          |  int i;
          |  char c;
          |};
          |int use_union() {
          |  union Top top;
          |  return top.i;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val outer = cpg.typeDecl.nameExact("Outer").head
      outer.member.nameExact("flags").typeFullName.l shouldBe List("int")
      outer.member.nameExact("flags").code.l shouldBe List("int flags:3")
      outer.member.nameExact("inline_field").typeFullName.l shouldBe List("inline_field")
      val inner   = outer.astChildren.isTypeDecl.nameExact("Inner").head
      val storage = outer.astChildren.isTypeDecl.nameExact("Storage").head
      val embeddedAnonymous = outer.astChildren.isTypeDecl.nameExact("<type>0").head
      val inlineFieldType   = outer.astChildren.isTypeDecl.nameExact("inline_field").head
      inner.member.nameExact("a").typeFullName.l shouldBe List("int")
      inner.astChildren.isTypeDecl.nameExact("Choice").head.member.name.l shouldBe List("i", "c")
      storage.member.name.l shouldBe List("x", "y")
      embeddedAnonymous.member.name.l shouldBe List("promoted")
      inlineFieldType.member.name.l shouldBe List("inline_x")
      cpg.typeDecl.nameExact("Top").member.name.l shouldBe List("i", "c")
      cpg.method.nameExact("use_union").local.nameExact("top").typeFullName.l shouldBe List("Top")
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
        val includeDir = dir / "include"
        Files.createDirectories(includeDir)
        Files.writeString(
          includeDir / "feature.h",
          """#define FEATURE_VALUE 5
            |struct FeatureBox { int value; };
            |typedef struct FeatureBox FeatureAlias;
            |int feature_add(int x, int y);
            |""".stripMargin
        )
        val selected = dir / "selected.c"
        val ignored  = dir / "ignored.c"
        Files.writeString(
          selected,
          """
            |#include "feature.h"
            |int selected() {
            |#if FEATURE == 1 && FEATURE_VALUE == 5
            |  FeatureAlias box;
            |  box.value = feature_add(FROM_DB, FEATURE_VALUE);
            |  return box.value;
            |#else
            |  return 0;
            |#endif
            |}
            |int disabled_by_zero() {
            |#if DISABLED
            |  return 1;
            |#else
            |  return 0;
            |#endif
            |}
            |#define LOCAL_MACRO 1
            |#undef LOCAL_MACRO
            |int dropped_after_undef() {
            |#ifdef LOCAL_MACRO
            |  return 1;
            |#else
            |  return 0;
            |#endif
            |}
            |int unresolved_after_undef() {
            |  return LOCAL_MACRO;
            |}
            |""".stripMargin
        )
        Files.writeString(ignored, "int ignored() { return 0; }\n")

        val compileCommands = dir / "compile_commands.json"
        Files.writeString(
          compileCommands,
          s"""
             |[
             |  {
             |    "directory": "${dir.toString}",
             |    "arguments": ["clang", "-I${includeDir.toString}", "-DFEATURE=1", "-DDISABLED=0", "-DFROM_DB=7", "-c", "selected.c"],
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
          cpg.method.nameExact("FROM_DB").signature.l shouldBe List("int(0)")
          cpg.method.nameExact("FEATURE_VALUE").fullName.l shouldBe List("include/feature.h:FEATURE_VALUE:int(0)")
          cpg.method.nameExact("LOCAL_MACRO").signature.l shouldBe List("int(0)")
          cpg.typeDecl.nameExact("FeatureBox").filename.l shouldBe List("include/feature.h")
          cpg.typeDecl.nameExact("FeatureBox").lineNumber.l shouldBe List(2)
          cpg.typeDecl.nameExact("FeatureAlias").filename.l shouldBe List("include/feature.h")
          cpg.typeDecl.nameExact("FeatureAlias").aliasTypeFullName.l shouldBe List("FeatureBox")
          cpg.method.nameExact("feature_add").external.filename.l shouldBe List("include/feature.h")
          cpg.method.nameExact("feature_add").external.signature.l shouldBe List("int(int,int)")
          cpg.method.nameExact("selected").local.nameExact("box").typeFullName.l shouldBe List("FeatureAlias")
          inside(cpg.call.nameExact("FROM_DB").l) { case List(fromDbCall) =>
            fromDbCall.code shouldBe "FROM_DB"
            fromDbCall.signature shouldBe "int(0)"
            fromDbCall.dispatchType shouldBe DispatchTypes.INLINED
          }
          inside(cpg.call.nameExact("FEATURE_VALUE").l) { case List(featureCall) =>
            featureCall.code shouldBe "FEATURE_VALUE"
            featureCall.methodFullName shouldBe "include/feature.h:FEATURE_VALUE:int(0)"
            featureCall.signature shouldBe "int(0)"
            featureCall.dispatchType shouldBe DispatchTypes.INLINED
          }
          inside(cpg.call.nameExact("feature_add").l) { case List(featureAddCall) =>
            featureAddCall.code shouldBe "feature_add(FROM_DB, FEATURE_VALUE)"
            featureAddCall.methodFullName shouldBe "feature_add"
            featureAddCall.signature shouldBe "int(int,int)"
            featureAddCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          }
          cpg.method.nameExact("selected").ast.isReturn.code.l shouldBe List("return box.value")
          cpg.method.nameExact("disabled_by_zero").ast.isReturn.code.l shouldBe List("return 0")
          cpg.method.nameExact("dropped_after_undef").ast.isReturn.code.l shouldBe List("return 0")
          cpg.call.nameExact("LOCAL_MACRO").l shouldBe Nil
          cpg.method.nameExact("unresolved_after_undef").ast.isIdentifier.nameExact("LOCAL_MACRO").code.l shouldBe List(
            "LOCAL_MACRO"
          )
        } finally {
          cpg.close()
        }
      }
    }

  }

}
