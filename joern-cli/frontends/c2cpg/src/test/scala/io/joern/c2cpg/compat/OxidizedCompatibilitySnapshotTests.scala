package io.joern.c2cpg.compat

import io.joern.c2cpg.{C2Cpg, Config}
import io.joern.c2cpg.astcreation.Defines
import io.joern.c2cpg.parser.ParserBackend
import io.joern.c2cpg.testfixtures.C2CpgSuite
import io.shiftleft.codepropertygraph.generated.{
  ControlStructureTypes,
  DispatchTypes,
  EvaluationStrategies,
  ModifierTypes,
  Operators
}
import io.shiftleft.codepropertygraph.generated.nodes.JumpTarget
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
          |  Widget(const Widget& other) : value(other.value) {}
          |  ~Widget();
          |  int value;
          |  static int instances;
          |  int get() { return normalize(); }
          |  int stable() const { return value; }
          |  virtual int render(int scale) { return scale; }
          |  virtual int declared(int scale);
          |  int normalize() { return pick(value) + identity(1); }
          |  int pick(int seed) { return seed; }
          |  int pick(Widget& other) { return other.value; }
          |  static int identity(int x);
          |  int size() const;
          |  int outside() const;
          |  int operator+(const Widget& other) const { return value + other.value; }
          |  Widget& operator=(const Widget& other) { value = other.value; return *this; }
          |  int operator[](int index) const { return value + index; }
          |};
          |class Fancy : public Widget {
          |public:
          |  int render(int scale) override { return scale + 1; }
          |  int inheritedValue() { return value + get(); }
          |  int explicitThis() { return this->value + this->get(); }
          |};
          |class Invoker {
          |public:
          |  int operator()(int delta) const { return delta + 1; }
          |};
          |int normalize() { return 99; }
          |int convert(int value) { return value; }
          |int convert(Widget widget) { return widget.get(); }
          |int make() { return 1; }
          |}
          |Core::Widget::Widget() : value(1) {}
          |Core::Widget::~Widget() {}
          |int Core::Widget::identity(int x) { return instances + x; }
          |int Core::Widget::outside() const { return stable(); }
          |int Core::Widget::declared(int scale) { return scale; }
          |int use() {
          |  Core::Widget widget(7);
          |  Core::Widget direct(widget);
          |  Core::Widget copied = widget;
          |  if (1) {
          |    Core::Widget scoped(widget);
          |  }
          |  if (widget.get()) {
          |    Core::Widget early(widget);
          |    return early.get();
          |  }
          |  Core::Widget *ptr = &widget;
          |  Core::Fancy fancy;
          |  Core::Invoker invoker;
          |  ptr->~Widget();
          |  widget = fancy;
          |  return Core::make() + Core::Widget::identity(2) + Core::Widget::instances + convert(1) + convert(widget) + widget.get() + widget.stable() + widget.outside() + widget.render(3) + widget.declared(4) + fancy.render(5) + fancy.get() + fancy.value + fancy.declared(6) + fancy.inheritedValue() + fancy.explicitThis() + (widget + fancy) + widget[2] + invoker(3);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.namespaceBlock.nameExact("Core").fullName.l shouldBe List("Test0.cpp:Core")
      cpg.typeDecl.fullNameExact("Core.Widget").fullName.l shouldBe List("Core.Widget")
      cpg.typeDecl.fullNameExact("Core.Widget").filename.l shouldBe List("Test0.cpp")
      cpg.typeDecl.fullNameExact("Core.Fancy").fullName.l shouldBe List("Core.Fancy")
      cpg.typeDecl.fullNameExact("Core.Fancy").inheritsFromTypeFullName.l shouldBe List("Core.Widget")
      cpg.typeDecl.fullNameExact("Core.Fancy").inheritsFromOut.fullName.l shouldBe List("Core.Widget")
      cpg.typeDecl.fullNameExact("Core.Invoker").fullName.l shouldBe List("Core.Invoker")
      cpg.typeDecl.fullNameExact("Core.Widget").member.name.l shouldBe List("value", "instances")
      cpg.typeDecl.fullNameExact("Core.Widget").member.typeFullName.l shouldBe List("int", "int")
      cpg.typeDecl.fullNameExact("Core.Widget").member.nameExact("instances").modifier.modifierType.l shouldBe
        List(ModifierTypes.STATIC)
      cpg.typeDecl.fullNameExact("Core.Widget").method.name.l.sorted shouldBe
        List(
          "Widget",
          "Widget",
          "Widget",
          "declared",
          "get",
          "identity",
          "normalize",
          "operator+",
          "operator=",
          "operator[]",
          "outside",
          "pick",
          "pick",
          "render",
          "size",
          "stable",
          "~Widget"
        )
      cpg.method.nameExact("Widget").internal.fullName.l.sorted shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void(Widget&)", "Core.Widget.Widget:void(int&)")
      cpg.method.nameExact("Widget").external.l shouldBe Nil
      cpg.method.nameExact("Widget").isConstructor.fullName.l.sorted shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void(Widget&)", "Core.Widget.Widget:void(int&)")
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
      cpg.method.nameExact("stable").internal.fullName.l shouldBe List("Core.Widget.stable:int()<const>")
      cpg.method.fullNameExact("Core.Widget.stable:int()<const>").signature.l shouldBe List("int()<const>")
      cpg.method.fullNameExact("Core.Widget.stable:int()<const>").parameter.name.l shouldBe List("this")
      cpg.method.fullNameExact("Core.Widget.stable:int()<const>").parameter.index.l shouldBe List(0)
      inside(cpg.method.fullNameExact("Core.Widget.stable:int()<const>").call.nameExact(Operators.indirectFieldAccess).l) {
        case List(fieldAccess) =>
          fieldAccess.code shouldBe "this->value"
          fieldAccess.typeFullName shouldBe "int"
          fieldAccess.argument.code.l shouldBe List("this", "value")
      }
      cpg.method.fullNameExact("Core.Widget.render:int(int)").modifier.modifierType.l shouldBe
        List(ModifierTypes.VIRTUAL)
      cpg.method.fullNameExact("Core.Widget.declared:int(int)").modifier.modifierType.l shouldBe
        List(ModifierTypes.VIRTUAL)
      cpg.method.fullNameExact("Core.Widget.declared:int(int)").external.l shouldBe Nil
      cpg.method.fullNameExact("Core.Widget.operator+:int(Widget&)<const>").parameter.name.l shouldBe
        List("this", "other")
      cpg.method.fullNameExact("Core.Widget.operator+:int(Widget&)<const>").parameter.typeFullName.l shouldBe
        List("Core.Widget*", "Widget&")
      cpg.method.fullNameExact("Core.Widget.operator=:Widget&(Widget&)").parameter.name.l shouldBe
        List("this", "other")
      cpg.method.fullNameExact("Core.Widget.operator=:Widget&(Widget&)").parameter.typeFullName.l shouldBe
        List("Core.Widget*", "Widget&")
      cpg.method.fullNameExact("Core.Widget.operator[]:int(int)<const>").parameter.name.l shouldBe
        List("this", "index")
      cpg.method.fullNameExact("Core.Invoker.operator():int(int)<const>").parameter.name.l shouldBe
        List("this", "delta")
      cpg.method.fullNameExact("Core.Fancy.render:int(int)").modifier.modifierType.l shouldBe
        List(ModifierTypes.VIRTUAL)
      cpg.method.fullNameExact("Core.Fancy.inheritedValue:int()").call.nameExact("get").methodFullName.l shouldBe
        List("Core.Widget.get:int()")
      inside(cpg.method.fullNameExact("Core.Fancy.inheritedValue:int()").call.nameExact(Operators.indirectFieldAccess).l) {
        case List(fieldAccess) =>
          fieldAccess.code shouldBe "this->value"
          fieldAccess.typeFullName shouldBe "int"
          fieldAccess.argument.code.l shouldBe List("this", "value")
      }
      cpg.method.fullNameExact("Core.Fancy.explicitThis:int()").call.codeExact("this->get()").methodFullName.l shouldBe
        List("Core.Widget.get:int()")
      cpg.method.fullNameExact("Core.Fancy.explicitThis:int()").call.codeExact("this->get()").argument.code.l shouldBe
        List("this")
      inside(cpg.method.fullNameExact("Core.Fancy.explicitThis:int()").call.codeExact("this->value").l) {
        case List(fieldAccess) =>
          fieldAccess.name shouldBe Operators.indirectFieldAccess
          fieldAccess.typeFullName shouldBe "int"
          fieldAccess.argument.code.l shouldBe List("this", "value")
      }
      cpg.method.fullNameExact("Core.Widget.identity:int(int)").modifier.modifierType.l shouldBe List(ModifierTypes.STATIC)
      cpg.method.fullNameExact("Core.Widget.identity:int(int)").parameter.name.l shouldBe List("x")
      cpg.method.fullNameExact("Core.Widget.identity:int(int)").parameter.index.l shouldBe List(1)
      inside(cpg.method.fullNameExact("Core.Widget.identity:int(int)").call.nameExact(Operators.fieldAccess).l) {
        case List(fieldAccess) =>
          fieldAccess.code shouldBe "Core.Widget.instances"
          fieldAccess.typeFullName shouldBe "int"
          fieldAccess.argument.code.l shouldBe List("Core.Widget", "instances")
      }
      cpg.method.nameExact("normalize").fullName.l.sorted shouldBe List("Core.Widget.normalize:int()", "Core.normalize:int()")
      cpg.method.nameExact("size").external.fullName.l shouldBe List("Core.Widget.size:int()<const>")
      cpg.method.nameExact("outside").internal.fullName.l shouldBe List("Core.Widget.outside:int()<const>")
      cpg.method.fullNameExact("Core.Widget.outside:int()<const>").call.nameExact("stable").methodFullName.l shouldBe
        List("Core.Widget.stable:int()<const>")
      cpg.method.nameExact("make").fullName.l shouldBe List("Core.make:int()")
      cpg.method.nameExact("use").local.nameExact("widget").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("use").local.nameExact("direct").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("use").local.nameExact("copied").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("use").local.nameExact("scoped").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("use").local.nameExact("early").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("use").local.nameExact("ptr").typeFullName.l shouldBe List("Core.Widget*")
      cpg.method.nameExact("use").local.nameExact("fancy").typeFullName.l shouldBe List("Core.Fancy")
      cpg.method.nameExact("use").local.nameExact("invoker").typeFullName.l shouldBe List("Core.Invoker")
      cpg.method.fullNameExact("Core.Widget.get:int()").parameter.name.l shouldBe List("this")
      cpg.method.fullNameExact("Core.Widget.get:int()").parameter.index.l shouldBe List(0)
      cpg.method.fullNameExact("Core.Widget.get:int()").parameter.typeFullName.l shouldBe List("Core.Widget*")
      cpg.method.fullNameExact("Core.Widget.get:int()").ast.isReturn.code.l shouldBe List("return normalize()")
      cpg.method.fullNameExact("Core.Widget.get:int()").call.nameExact("normalize").methodFullName.l shouldBe
        List("Core.Widget.normalize:int()")
      cpg.method.fullNameExact("Core.Widget.normalize:int()").call.nameExact("pick").methodFullName.l shouldBe
        List("Core.Widget.pick:int(int)")
      cpg.method.fullNameExact("Core.Widget.normalize:int()").call.nameExact("identity").methodFullName.l shouldBe
        List("Core.Widget.identity:int(int)")
      inside(cpg.method.fullNameExact("Core.Widget.normalize:int()").call.nameExact(Operators.indirectFieldAccess).l) {
        case List(fieldAccess) =>
        fieldAccess.code shouldBe "this->value"
        fieldAccess.typeFullName shouldBe "int"
        fieldAccess.argument.code.l shouldBe List("this", "value")
      }
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core.Widget.Widget(7)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(int&)")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core.Widget.Widget(widget)").methodFullName.l shouldBe
        List(
          "Core.Widget.Widget:void(Widget&)",
          "Core.Widget.Widget:void(Widget&)",
          "Core.Widget.Widget:void(Widget&)",
          "Core.Widget.Widget:void(Widget&)"
        )
      cpg.method.nameExact("use").call.nameExact(Operators.assignment).codeExact("direct = Core.Widget.Widget(widget)").argument.code.l shouldBe
        List("direct", "Core.Widget.Widget(widget)")
      cpg.method.nameExact("use").call.nameExact(Operators.assignment).codeExact("copied = Core.Widget.Widget(widget)").argument.code.l shouldBe
        List("copied", "Core.Widget.Widget(widget)")
      cpg.method.nameExact("use").call.nameExact(Operators.assignment).codeExact("*ptr = &widget").argument.code.l shouldBe
        List("*ptr", "&widget")
      inside(cpg.method.nameExact("use").call.nameExact("~Widget").codeExact("ptr->~Widget()").l) {
        case List(destructorCall) =>
          destructorCall.methodFullName shouldBe "Core.Widget.~Widget:void()"
          destructorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          destructorCall.argument.code.l shouldBe List("ptr")
      }
      cpg.method.nameExact("use").call.nameExact("~Widget").code.l.sorted shouldBe
        List(
          "copied.~Widget()",
          "copied.~Widget()",
          "direct.~Widget()",
          "direct.~Widget()",
          "early.~Widget()",
          "ptr->~Widget()",
          "scoped.~Widget()",
          "widget.~Widget()",
          "widget.~Widget()"
        )
      inside(cpg.method.nameExact("use").controlStructure.controlStructureType(ControlStructureTypes.IF).l) {
        case List(scopedIf, earlyIf) =>
          scopedIf.ast.isCall.nameExact("~Widget").code.l shouldBe List("scoped.~Widget()")
          earlyIf.ast.isCall.nameExact("~Widget").code.l shouldBe
            List("early.~Widget()", "copied.~Widget()", "direct.~Widget()", "widget.~Widget()")
      }
      cpg.method.nameExact("use").call.codeExact("Core::Widget::identity(2)").methodFullName.l shouldBe
        List("Core.Widget.identity:int(int)")
      inside(cpg.method.nameExact("use").call.nameExact(Operators.fieldAccess).codeExact("Core::Widget::instances").l) {
        case List(fieldAccess) =>
          fieldAccess.typeFullName shouldBe "int"
          fieldAccess.argument.code.l shouldBe List("Core::Widget", "instances")
      }
      cpg.method.nameExact("use").call.nameExact("make").methodFullName.l shouldBe List("Core.make:int()")
      cpg.method.nameExact("use").call.codeExact("convert(1)").methodFullName.l shouldBe List("Core.convert:int(int)")
      cpg.method.nameExact("use").call.codeExact("convert(widget)").methodFullName.l shouldBe
        List("Core.convert:int(Widget)")
      cpg.method.nameExact("use").call.codeExact("widget.get()").methodFullName.l shouldBe
        List("Core.Widget.get:int()", "Core.Widget.get:int()")
      cpg.method.nameExact("use").call.codeExact("fancy.get()").methodFullName.l shouldBe List("Core.Widget.get:int()")
      cpg.method.nameExact("use").call.nameExact("stable").methodFullName.l shouldBe List("Core.Widget.stable:int()<const>")
      cpg.method.nameExact("use").call.nameExact("outside").methodFullName.l shouldBe
        List("Core.Widget.outside:int()<const>")
      inside(cpg.method.nameExact("use").call.nameExact(Operators.fieldAccess).codeExact("fancy.value").l) {
        case List(fieldAccess) =>
          fieldAccess.typeFullName shouldBe "int"
          fieldAccess.argument.code.l shouldBe List("fancy", "value")
      }
      inside(cpg.method.nameExact("use").call.nameExact("render").codeExact("widget.render(3)").l) { case List(renderCall) =>
        renderCall.methodFullName shouldBe "Core.Widget.render:int(int)"
        renderCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        renderCall.argument.code.l shouldBe List("widget", "3")
        renderCall.receiver.code.l shouldBe List("widget")
      }
      inside(cpg.method.nameExact("use").call.nameExact("render").codeExact("fancy.render(5)").l) { case List(renderCall) =>
        renderCall.methodFullName shouldBe "Core.Fancy.render:int(int)"
        renderCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        renderCall.argument.code.l shouldBe List("fancy", "5")
        renderCall.receiver.code.l shouldBe List("fancy")
      }
      inside(cpg.method.nameExact("use").call.nameExact("declared").codeExact("widget.declared(4)").l) {
        case List(declaredCall) =>
          declaredCall.methodFullName shouldBe "Core.Widget.declared:int(int)"
          declaredCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
          declaredCall.argument.code.l shouldBe List("widget", "4")
          declaredCall.receiver.code.l shouldBe List("widget")
      }
      inside(cpg.method.nameExact("use").call.nameExact("declared").codeExact("fancy.declared(6)").l) {
        case List(declaredCall) =>
        declaredCall.methodFullName shouldBe "Core.Widget.declared:int(int)"
        declaredCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        declaredCall.argument.code.l shouldBe List("fancy", "6")
        declaredCall.receiver.code.l shouldBe List("fancy")
      }
      cpg.method.nameExact("use").call.codeExact("fancy.inheritedValue()").methodFullName.l shouldBe
        List("Core.Fancy.inheritedValue:int()")
      cpg.method.nameExact("use").call.codeExact("fancy.explicitThis()").methodFullName.l shouldBe
        List("Core.Fancy.explicitThis:int()")
      inside(cpg.method.nameExact("use").call.nameExact("operator=").codeExact("widget = fancy").l) {
        case List(operatorCall) =>
          operatorCall.methodFullName shouldBe "Core.Widget.operator=:Widget&(Widget&)"
          operatorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          operatorCall.typeFullName shouldBe "Widget&"
          operatorCall.argument.code.l shouldBe List("widget", "fancy")
      }
      cpg.method.nameExact("use").call.nameExact(Operators.assignment).codeExact("widget = fancy").l shouldBe Nil
      inside(cpg.method.nameExact("use").call.nameExact("operator+").codeExact("widget + fancy").l) {
        case List(operatorCall) =>
          operatorCall.methodFullName shouldBe "Core.Widget.operator+:int(Widget&)<const>"
          operatorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          operatorCall.typeFullName shouldBe "int"
          operatorCall.argument.code.l shouldBe List("widget", "fancy")
      }
      cpg.method
        .fullNameExact("Core.Widget.operator+:int(Widget&)<const>")
        .call
        .nameExact(Operators.addition)
        .codeExact("value + other.value")
        .size shouldBe 1
      cpg.method
        .fullNameExact("Core.Invoker.operator():int(int)<const>")
        .call
        .nameExact(Operators.addition)
        .codeExact("delta + 1")
        .size shouldBe 1
      inside(cpg.method.nameExact("use").call.nameExact("operator[]").codeExact("widget[2]").l) {
        case List(operatorCall) =>
          operatorCall.methodFullName shouldBe "Core.Widget.operator[]:int(int)<const>"
          operatorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          operatorCall.typeFullName shouldBe "int"
          operatorCall.argument.code.l shouldBe List("widget", "2")
      }
      inside(cpg.method.nameExact("use").call.nameExact("operator()").codeExact("invoker(3)").l) {
        case List(operatorCall) =>
          operatorCall.methodFullName shouldBe "Core.Invoker.operator():int(int)<const>"
          operatorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          operatorCall.typeFullName shouldBe "int"
          operatorCall.argument.code.l shouldBe List("invoker", "3")
      }
    }

    "capture C++ callable object references and pointer dereference calls" in {
      val cpg = code(
        """
          |namespace Core {
          |class Invoker {
          |public:
          |  int operator()(int delta) const { return delta + 1; }
          |};
          |}
          |int use(Core::Invoker& ref, Core::Invoker* ptr) {
          |  Core::Invoker local;
          |  return ref(1) + (*ptr)(2) + local(3);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").parameter.nameExact("ref").typeFullName.l shouldBe List("Core.Invoker&")
      cpg.method.nameExact("use").parameter.nameExact("ptr").typeFullName.l shouldBe List("Core.Invoker*")
      cpg.method.nameExact("use").local.nameExact("local").typeFullName.l shouldBe List("Core.Invoker")

      inside(cpg.method.nameExact("use").call.nameExact("operator()").codeExact("ref(1)").l) {
        case List(operatorCall) =>
          operatorCall.methodFullName shouldBe "Core.Invoker.operator():int(int)<const>"
          operatorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          operatorCall.typeFullName shouldBe "int"
          operatorCall.argument.code.l shouldBe List("ref", "1")
      }
      inside(cpg.method.nameExact("use").call.nameExact("operator()").codeExact("(*ptr)(2)").l) {
        case List(operatorCall) =>
          operatorCall.methodFullName shouldBe "Core.Invoker.operator():int(int)<const>"
          operatorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          operatorCall.typeFullName shouldBe "int"
          operatorCall.argument.code.l shouldBe List("*ptr", "2")
      }
      inside(cpg.method.nameExact("use").call.nameExact("operator()").codeExact("local(3)").l) {
        case List(operatorCall) =>
          operatorCall.methodFullName shouldBe "Core.Invoker.operator():int(int)<const>"
          operatorCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          operatorCall.typeFullName shouldBe "int"
          operatorCall.argument.code.l shouldBe List("local", "3")
      }
      cpg.method.nameExact("use").call.nameExact(Defines.OperatorPointerCall).codeExact("(*ptr)(2)").l shouldBe Nil
    }

    "capture C++ template declarations and instantiated receivers" in {
      val cpg = code(
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
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.typeDecl.fullNameExact("Core.Holder").member.nameExact("value").typeFullName.l shouldBe List("T")
      cpg.method.fullNameExact("Core.Holder.get:T()").ast.isReturn.code.l shouldBe List("return value")
      cpg.method.nameExact("use").parameter.nameExact("holder").typeFullName.l shouldBe List("Core.Holder<int>")
      inside(cpg.method.nameExact("use").call.nameExact(Operators.fieldAccess).codeExact("holder.value").l) {
        case List(fieldAccess) =>
          fieldAccess.typeFullName shouldBe "T"
          fieldAccess.argument.code.l shouldBe List("holder", "value")
      }
      cpg.method.nameExact("use").call.codeExact("holder.get()").methodFullName.l shouldBe List("Core.Holder.get:T()")
      inside(cpg.method.nameExact("use").call.codeExact("Core::pick<int>(1)").l) { case List(pickCall) =>
        pickCall.name shouldBe "pick"
        pickCall.methodFullName shouldBe "Core.pick:T(T)"
        pickCall.typeFullName shouldBe "T"
      }
    }

    "capture C++ lambda methods, captures, and invocations" in {
      val cpg = code(
        """
          |int use(int base) {
          |  auto mapper = [base](int x) { return base + x; };
          |  return mapper(2);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName    = cpg.method.nameExact("use").fullName.head
      val lambdaFullName = s"$useFullName.<lambda>0:int(int)"
      cpg.method.nameExact("use").local.nameExact("mapper").typeFullName.l shouldBe List(lambdaFullName)
      cpg.method.nameExact("use").ast.isMethodRef.methodFullNameExact(lambdaFullName).typeFullName.l shouldBe
        List(lambdaFullName)

      inside(cpg.method.fullNameExact(lambdaFullName).l) { case List(lambdaMethod) =>
        lambdaMethod.name shouldBe "<lambda>0"
        lambdaMethod.signature shouldBe "int(int)"
        lambdaMethod.modifier.modifierType.l.sorted shouldBe
          List(ModifierTypes.LAMBDA, ModifierTypes.PRIVATE, ModifierTypes.STATIC, ModifierTypes.VIRTUAL).sorted
        lambdaMethod.parameter.name.l shouldBe List("x")
        lambdaMethod.parameter.typeFullName.l shouldBe List("int")
        lambdaMethod.methodReturn.typeFullName shouldBe "int"
        lambdaMethod.ast.isReturn.code.l shouldBe List("return base + x")
        lambdaMethod.local.nameExact("base").typeFullName.l shouldBe List("int")
      }

      inside(cpg.typeDecl.fullNameExact(lambdaFullName).l) { case List(lambdaType) =>
        lambdaType.name shouldBe "<lambda>0"
        lambdaType.inheritsFromTypeFullName should contain theSameElementsAs List(Defines.Function)
        inside(lambdaType.bindsOut.l) { case List(binding) =>
          binding.name shouldBe Defines.OperatorCall
          binding.methodFullName shouldBe lambdaFullName
          binding.signature shouldBe "int(int)"
        }
      }

      inside(cpg.closureBinding.l) { case List(binding) =>
        val capturedBase = cpg.method.fullNameExact(lambdaFullName).local.nameExact("base").head
        binding.closureBindingId shouldBe capturedBase.closureBindingId
        binding.evaluationStrategy shouldBe EvaluationStrategies.BY_VALUE
        binding._refOut.l shouldBe cpg.method.nameExact("use").parameter.nameExact("base").l
        binding._captureIn.l shouldBe cpg.methodRef.methodFullNameExact(lambdaFullName).l
      }

      inside(cpg.method.nameExact("use").call.nameExact(Defines.OperatorCall).codeExact("mapper(2)").l) {
        case List(lambdaCall) =>
          lambdaCall.methodFullName shouldBe s"${Defines.OperatorCall}:int(int)"
          lambdaCall.signature shouldBe "int(int)"
          lambdaCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
          lambdaCall.typeFullName shouldBe "int"
          lambdaCall.receiver.code.l shouldBe List("mapper")
          lambdaCall.argument.code.l shouldBe List("2")
      }
    }

    "capture C++ lambda default and reference captures" in {
      val cpg = code(
        """
          |int use(int base) {
          |  int delta = 3;
          |  auto by_ref = [&](int x) { return base + delta + x; };
          |  auto by_val = [=](int x) { return base + delta + x; };
          |  return by_ref(2) + by_val(1);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName = cpg.method.nameExact("use").fullName.head
      val refLambda   = s"$useFullName.<lambda>0:int(int)"
      val valLambda   = s"$useFullName.<lambda>1:int(int)"

      cpg.method.nameExact("use").local.nameExact("by_ref").typeFullName.l shouldBe List(refLambda)
      cpg.method.nameExact("use").local.nameExact("by_val").typeFullName.l shouldBe List(valLambda)
      cpg.method.fullNameExact(refLambda).local.name.l.sorted shouldBe List("base", "delta")
      cpg.method.fullNameExact(valLambda).local.name.l.sorted shouldBe List("base", "delta")

      cpg.closureBinding.filter(_.closureBindingId.contains(s"$refLambda:base")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_REFERENCE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$refLambda:delta")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_REFERENCE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$valLambda:base")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$valLambda:delta")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)

      cpg.method.nameExact("use").call.nameExact(Defines.OperatorCall).code.l should contain theSameElementsAs
        List("by_ref(2)", "by_val(1)")
    }

    "capture C++ lambda this captures inside methods" in {
      val cpg = code(
        """
          |class Widget {
          |public:
          |  int value;
          |  int read(int base) {
          |    auto reader = [this](int x) { return this->value + x; };
          |    return reader(base);
          |  }
          |};
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val readFullName   = cpg.method.nameExact("read").fullName.head
      val lambdaFullName = s"$readFullName.<lambda>0:int(int)"
      cpg.method.nameExact("read").local.nameExact("reader").typeFullName.l shouldBe List(lambdaFullName)
      cpg.method.fullNameExact(lambdaFullName).local.nameExact("this").typeFullName.l shouldBe List("Widget*")
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$lambdaFullName:this")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_SHARING)
      inside(cpg.closureBinding.filter(_.closureBindingId.contains(s"$lambdaFullName:this")).l) {
        case List(binding) =>
          binding._refOut.l shouldBe cpg.method.nameExact("read").parameter.nameExact("this").l
      }
      cpg.method.fullNameExact(lambdaFullName).call.codeExact("this->value").argument.code.l shouldBe
        List("this", "value")
    }

    "capture C++ lambda init captures" in {
      val cpg = code(
        """
          |int use(int base) {
          |  int delta = 3;
          |  auto mapper = [snap = base + delta, alias = base](int x) { return snap + alias + x; };
          |  return mapper(2);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName    = cpg.method.nameExact("use").fullName.head
      val lambdaFullName = s"$useFullName.<lambda>0:int(int)"
      cpg.method.nameExact("use").local.nameExact("mapper").typeFullName.l shouldBe List(lambdaFullName)
      cpg.method.fullNameExact(lambdaFullName).local.name.l.sorted shouldBe List("alias", "snap")
      cpg.method.fullNameExact(lambdaFullName).local.typeFullName.l.sorted shouldBe List("int", "int")
      cpg.method.fullNameExact(lambdaFullName).ast.isReturn.code.l shouldBe List("return snap + alias + x")

      cpg.closureBinding.filter(_.closureBindingId.contains(s"$lambdaFullName:snap")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$lambdaFullName:alias")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)
      inside(cpg.closureBinding.filter(_.closureBindingId.contains(s"$lambdaFullName:alias")).l) {
        case List(binding) =>
          binding._refOut.l shouldBe cpg.method.nameExact("use").parameter.nameExact("base").l
      }
      inside(cpg.closureBinding.filter(_.closureBindingId.contains(s"$lambdaFullName:snap")).l) {
        case List(binding) =>
          binding._refOut.l shouldBe Nil
      }
    }

    "capture C++ generic lambda parameters" in {
      val cpg = code(
        """
          |int use() {
          |  auto identity = [](auto value) { return value; };
          |  return identity(1);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName    = cpg.method.nameExact("use").fullName.head
      val lambdaFullName = s"$useFullName.<lambda>0:auto(auto)"
      cpg.method.nameExact("use").local.nameExact("identity").typeFullName.l shouldBe List(lambdaFullName)
      inside(cpg.method.fullNameExact(lambdaFullName).l) { case List(lambdaMethod) =>
        lambdaMethod.parameter.name.l shouldBe List("value")
        lambdaMethod.parameter.typeFullName.l shouldBe List("auto")
        lambdaMethod.methodReturn.typeFullName shouldBe "auto"
        lambdaMethod.ast.isReturn.code.l shouldBe List("return value")
      }
      inside(cpg.method.nameExact("use").call.nameExact(Defines.OperatorCall).codeExact("identity(1)").l) {
        case List(lambdaCall) =>
          lambdaCall.methodFullName shouldBe s"${Defines.OperatorCall}:auto(auto)"
          lambdaCall.signature shouldBe "auto(auto)"
          lambdaCall.typeFullName shouldBe "auto"
      }
    }

    "capture C++ constrained lambdas from the Rust parser backend" in {
      val cpg = code(
        """
          |int use() {
          |  auto l1 = []<my_concept T> (T v) { return v; };
          |  auto l2 = []<typename T> requires my_concept<T> (T v) { return v; };
          |  auto l3 = []<typename T> (T v) requires my_concept<T> { return v; };
          |  auto l4 = [](my_concept auto v) { return v; };
          |  auto l5 = []<my_concept auto v> () { return v; };
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName = cpg.method.nameExact("use").fullName.head
      val l1FullName  = s"$useFullName.<lambda>0:T(T)"
      val l2FullName  = s"$useFullName.<lambda>1:T(T)"
      val l3FullName  = s"$useFullName.<lambda>2:T(T)"
      val l4FullName  = s"$useFullName.<lambda>3:my_concept auto(my_concept auto)"
      val l5FullName  = s"$useFullName.<lambda>4:my_concept auto()"

      cpg.method.nameExact("use").local.nameExact("l1").typeFullName.l shouldBe List(l1FullName)
      cpg.method.nameExact("use").local.nameExact("l2").typeFullName.l shouldBe List(l2FullName)
      cpg.method.nameExact("use").local.nameExact("l3").typeFullName.l shouldBe List(l3FullName)
      cpg.method.nameExact("use").local.nameExact("l4").typeFullName.l shouldBe List(l4FullName)
      cpg.method.nameExact("use").local.nameExact("l5").typeFullName.l shouldBe List(l5FullName)

      Seq(l1FullName, l2FullName, l3FullName).foreach { fullName =>
        inside(cpg.method.fullNameExact(fullName).l) { case List(lambdaMethod) =>
          lambdaMethod.signature shouldBe "T(T)"
          lambdaMethod.methodReturn.typeFullName shouldBe "T"
          lambdaMethod.parameter.name.l shouldBe List("v")
          lambdaMethod.parameter.typeFullName.l shouldBe List("T")
        }
      }

      inside(cpg.method.fullNameExact(l4FullName).l) { case List(lambdaMethod) =>
        lambdaMethod.signature shouldBe "my_concept auto(my_concept auto)"
        lambdaMethod.methodReturn.typeFullName shouldBe "my_concept auto"
        lambdaMethod.parameter.name.l shouldBe List("v")
        lambdaMethod.parameter.code.l shouldBe List("my_concept auto v")
        lambdaMethod.parameter.typeFullName.l shouldBe List("my_concept auto")
      }
      inside(cpg.method.fullNameExact(l5FullName).l) { case List(lambdaMethod) =>
        lambdaMethod.signature shouldBe "my_concept auto()"
        lambdaMethod.methodReturn.typeFullName shouldBe "my_concept auto"
        lambdaMethod.parameter.l shouldBe Nil
      }
    }

    "capture C++ mutable lambda modifiers and captured-state mutation" in {
      val cpg = code(
        """
          |int use(int seed) {
          |  auto bump = [seed](int step) mutable -> int {
          |    seed += step;
          |    return seed;
          |  };
          |  auto read = [seed]() -> int { return seed; };
          |  return bump(1) + read();
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName  = cpg.method.nameExact("use").fullName.head
      val bumpFullName = s"$useFullName.<lambda>0:int(int)"
      val readFullName = s"$useFullName.<lambda>1:int()"
      cpg.method.nameExact("use").local.nameExact("bump").typeFullName.l shouldBe List(bumpFullName)
      cpg.method.nameExact("use").local.nameExact("read").typeFullName.l shouldBe List(readFullName)

      cpg.method.fullNameExact(bumpFullName).modifier.modifierType.l.sorted shouldBe
        List(ModifierTypes.LAMBDA, "MUTABLE", ModifierTypes.PRIVATE, ModifierTypes.STATIC, ModifierTypes.VIRTUAL).sorted
      cpg.method.fullNameExact(readFullName).modifier.modifierType.l should contain theSameElementsAs
        List(ModifierTypes.LAMBDA, ModifierTypes.PRIVATE, ModifierTypes.STATIC, ModifierTypes.VIRTUAL)

      cpg.method.fullNameExact(bumpFullName).local.nameExact("seed").typeFullName.l shouldBe List("int")
      cpg.method.fullNameExact(readFullName).local.nameExact("seed").typeFullName.l shouldBe List("int")
      cpg.method.fullNameExact(bumpFullName).call.nameExact(Operators.assignmentPlus).code.l shouldBe
        List("seed += step")
      cpg.method.fullNameExact(bumpFullName).ast.isReturn.code.l shouldBe List("return seed")

      cpg.closureBinding.filter(_.closureBindingId.contains(s"$bumpFullName:seed")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$readFullName:seed")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)
      cpg.method.nameExact("use").call.nameExact(Defines.OperatorCall).code.l should contain theSameElementsAs
        List("bump(1)", "read()")
    }

    "capture C++ lambdas assigned to explicit function object locals" in {
      val cpg = code(
        """
          |int use(int base) {
          |  std::function<int(int)> mapper = [base](int x) -> int { return base + x; };
          |  auto caller = [mapper](int y) -> int { return mapper(y); };
          |  return mapper(2) + caller(3);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName  = cpg.method.nameExact("use").fullName.head
      val mapperLambda = s"$useFullName.<lambda>0:int(int)"
      val callerLambda = s"$useFullName.<lambda>1:int(int)"

      cpg.method.nameExact("use").local.nameExact("mapper").typeFullName.l shouldBe
        List("std.function<int(int)>")
      cpg.method.nameExact("use").local.nameExact("caller").typeFullName.l shouldBe List(callerLambda)
      cpg.method.nameExact("use").ast.isMethodRef.methodFullNameExact(mapperLambda).typeFullName.l shouldBe
        List(mapperLambda)

      inside(cpg.method.nameExact("use").call.nameExact(Defines.OperatorCall).codeExact("mapper(2)").l) {
        case List(mapperCall) =>
          mapperCall.methodFullName shouldBe s"${Defines.OperatorCall}:int(int)"
          mapperCall.signature shouldBe "int(int)"
          mapperCall.typeFullName shouldBe "int"
          mapperCall.receiver.code.l shouldBe List("mapper")
      }
      inside(cpg.method.fullNameExact(callerLambda).call.nameExact(Defines.OperatorCall).codeExact("mapper(y)").l) {
        case List(capturedMapperCall) =>
          capturedMapperCall.methodFullName shouldBe s"${Defines.OperatorCall}:int(int)"
          capturedMapperCall.signature shouldBe "int(int)"
          capturedMapperCall.typeFullName shouldBe "int"
          capturedMapperCall.receiver.code.l shouldBe List("mapper")
      }
      inside(cpg.closureBinding.filter(_.closureBindingId.contains(s"$callerLambda:mapper")).l) {
        case List(binding) =>
          binding.evaluationStrategy shouldBe EvaluationStrategies.BY_VALUE
          binding._refOut.l shouldBe cpg.method.nameExact("use").local.nameExact("mapper").l
      }
    }

    "capture C++ nested lambda ownership and captures" in {
      val cpg = code(
        """
          |int use(int base) {
          |  auto outer = [base](int x) -> int {
          |    auto inner = [&](int y) -> int { return base + x + y; };
          |    return inner(1);
          |  };
          |  return outer(2);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val useFullName   = cpg.method.nameExact("use").fullName.head
      val outerFullName = s"$useFullName.<lambda>0:int(int)"
      val innerFullName = s"$outerFullName.<lambda>1:int(int)"
      cpg.method.nameExact("use").local.nameExact("outer").typeFullName.l shouldBe List(outerFullName)
      cpg.method.fullNameExact(outerFullName).local.nameExact("inner").typeFullName.l shouldBe List(innerFullName)

      inside(cpg.method.fullNameExact(innerFullName).l) { case List(innerMethod) =>
        innerMethod.parameter.name.l shouldBe List("y")
        innerMethod.parameter.typeFullName.l shouldBe List("int")
        innerMethod.local.name.l.sorted shouldBe List("base", "x")
        innerMethod.ast.isReturn.code.l shouldBe List("return base + x + y")
      }

      cpg.closureBinding.filter(_.closureBindingId.contains(s"$outerFullName:base")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$innerFullName:base")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_REFERENCE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$innerFullName:x")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_REFERENCE)
      inside(cpg.closureBinding.filter(_.closureBindingId.contains(s"$innerFullName:base")).l) {
        case List(binding) =>
          binding._refOut.l shouldBe cpg.method.fullNameExact(outerFullName).local.nameExact("base").l
      }
      inside(cpg.closureBinding.filter(_.closureBindingId.contains(s"$innerFullName:x")).l) {
        case List(binding) =>
          binding._refOut.l shouldBe cpg.method.fullNameExact(outerFullName).parameter.nameExact("x").l
      }

      inside(cpg.method.fullNameExact(outerFullName).call.nameExact(Defines.OperatorCall).codeExact("inner(1)").l) {
        case List(innerCall) =>
          innerCall.methodFullName shouldBe s"${Defines.OperatorCall}:int(int)"
          innerCall.receiver.code.l shouldBe List("inner")
          innerCall.typeFullName shouldBe "int"
      }
      inside(cpg.method.nameExact("use").call.nameExact(Defines.OperatorCall).codeExact("outer(2)").l) {
        case List(outerCall) =>
          outerCall.methodFullName shouldBe s"${Defines.OperatorCall}:int(int)"
          outerCall.receiver.code.l shouldBe List("outer")
          outerCall.typeFullName shouldBe "int"
      }
    }

    "capture C++ for initializer destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  ~Widget();
          |};
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int loop() {
          |  Core::Widget widget;
          |  for (Core::Widget guard(widget); 0; ) {
          |  }
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("loop").local.nameExact("widget").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("loop").local.nameExact("guard").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("loop").call.nameExact("Widget").codeExact("Core.Widget.Widget(widget)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("loop").call.nameExact("~Widget").code.l shouldBe
        List("guard.~Widget()", "widget.~Widget()")
      cpg.method.nameExact("loop").controlStructure.controlStructureType(ControlStructureTypes.FOR).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe Nil
    }

    "capture C++ default local constructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  ~Widget();
          |};
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int defaults() {
          |  Core::Widget outer;
          |  {
          |    Core::Widget scoped;
          |  }
          |  for (Core::Widget guard; 0; ) {
          |  }
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("defaults").local.nameExact("outer").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("defaults").local.nameExact("scoped").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("defaults").local.nameExact("guard").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("defaults").call.nameExact("Widget").codeExact("Core.Widget.Widget()").methodFullName.l shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void()", "Core.Widget.Widget:void()")
      val defaultAssignments = cpg.method.nameExact("defaults").call.nameExact(Operators.assignment).code.l
      defaultAssignments.filterNot(_.startsWith("<tmp>")) shouldBe
        List(
          "outer = Core.Widget.Widget()",
          "scoped = Core.Widget.Widget()",
          "guard = Core.Widget.Widget()"
        )
      defaultAssignments.filter(_.startsWith("<tmp>")) shouldBe
        List("<tmp>0 = <operator>.alloc", "<tmp>1 = <operator>.alloc", "<tmp>2 = <operator>.alloc")
      cpg.method.nameExact("defaults").call.nameExact("~Widget").code.l.sorted shouldBe
        List("guard.~Widget()", "outer.~Widget()", "scoped.~Widget()")
    }

    "capture C++ braced local constructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(int seed) {}
          |  ~Widget();
          |};
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int braces(int seed) {
          |  Core::Widget empty{};
          |  Core::Widget direct{seed};
          |  Core::Widget assigned = {seed};
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val bracedLocalNames = cpg.method.nameExact("braces").local.name.l
      bracedLocalNames.filterNot(_.startsWith("<tmp>")) shouldBe List("empty", "direct", "assigned")
      bracedLocalNames.filter(_.startsWith("<tmp>")) shouldBe List("<tmp>0", "<tmp>1", "<tmp>2")
      cpg.method.nameExact("braces").call.nameExact("Widget").codeExact("Core.Widget.Widget()").methodFullName.l shouldBe
        List("Core.Widget.Widget:void()")
      cpg.method.nameExact("braces").call.nameExact("Widget").codeExact("Core.Widget.Widget(seed)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(int)", "Core.Widget.Widget:void(int)")
      val bracedAssignments = cpg.method.nameExact("braces").call.nameExact(Operators.assignment).code.l
      bracedAssignments.filterNot(_.startsWith("<tmp>")) shouldBe
        List(
          "empty = Core.Widget.Widget()",
          "direct = Core.Widget.Widget(seed)",
          "assigned = Core.Widget.Widget(seed)"
        )
      bracedAssignments.filter(_.startsWith("<tmp>")) shouldBe
        List("<tmp>0 = <operator>.alloc", "<tmp>1 = <operator>.alloc", "<tmp>2 = <operator>.alloc")
      cpg.method.nameExact("braces").call.nameExact("~Widget").code.l shouldBe
        List("assigned.~Widget()", "direct.~Widget()", "empty.~Widget()")
    }

    "capture C++ braced constructor temporary destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  Widget(Widget&& other) {}
          |  ~Widget();
          |};
          |void accept(Widget&& widget) {}
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int use() {
          |  Core::Widget source;
          |  Core::accept(Core::Widget{});
          |  Core::accept(Core::Widget{source});
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.fullNameExact("Core.accept:void(Widget&&)").parameter.typeFullName.l shouldBe List("Widget&&")
      cpg.method.nameExact("use").call.nameExact("accept").methodFullName.l shouldBe
        List("Core.accept:void(Widget&&)", "Core.accept:void(Widget&&)")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core::Widget{}").methodFullName.l shouldBe
        List("Core.Widget.Widget:void()")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core::Widget{source}").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("use").call.nameExact("~Widget").code.l.sorted shouldBe
        List("Core::Widget{source}.~Widget()", "Core::Widget{}.~Widget()", "source.~Widget()")
    }

    "resolve C++ move constructors for rvalue initializers" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  Widget(Widget&& other) {}
          |  ~Widget();
          |};
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |Core::Widget makeWidget() {
          |  Core::Widget temp;
          |  return temp;
          |}
          |int use() {
          |  Core::Widget source;
          |  Core::Widget copied = source;
          |  Core::Widget moved = makeWidget();
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.fullNameExact("Core.Widget.Widget:void(Widget&)").parameter.typeFullName.l shouldBe
        List("Core.Widget*", "Widget&")
      cpg.method.fullNameExact("Core.Widget.Widget:void(Widget&&)").parameter.typeFullName.l shouldBe
        List("Core.Widget*", "Widget&&")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core.Widget.Widget(source)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)")
      cpg.method
        .nameExact("use")
        .call
        .nameExact("Widget")
        .codeExact("Core.Widget.Widget(makeWidget())")
        .methodFullName
        .l shouldBe List("Core.Widget.Widget:void(Widget&&)")
      cpg.method.nameExact("use").call.nameExact("~Widget").code.l shouldBe
        List("moved.~Widget()", "copied.~Widget()", "source.~Widget()")
    }

    "capture C++ constructor temporary destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  Widget(Widget&& other) {}
          |  ~Widget();
          |};
          |void accept(Widget&& widget) {}
          |int consume(Widget&& widget) { return 1; }
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int use() {
          |  Core::Widget source;
          |  Core::accept(Core::Widget());
          |  Core::accept(Core::Widget(source));
          |  Core::Widget local = Core::Widget();
          |  int result = Core::consume(Core::Widget(source));
          |  return Core::consume(Core::Widget(local)) + result;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.fullNameExact("Core.accept:void(Widget&&)").parameter.typeFullName.l shouldBe List("Widget&&")
      cpg.method.fullNameExact("Core.consume:int(Widget&&)").parameter.typeFullName.l shouldBe List("Widget&&")
      cpg.method.nameExact("use").local.nameExact("local").typeFullName.l shouldBe List("Core.Widget")
      cpg.method.nameExact("use").local.nameExact("result").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("use").call.nameExact("accept").methodFullName.l shouldBe
        List("Core.accept:void(Widget&&)", "Core.accept:void(Widget&&)")
      cpg.method.nameExact("use").call.nameExact("consume").methodFullName.l shouldBe
        List("Core.consume:int(Widget&&)", "Core.consume:int(Widget&&)")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core::Widget()").methodFullName.l shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void()")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core::Widget(source)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)", "Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core::Widget(local)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("use").call.nameExact("Widget").codeExact("Core.Widget.Widget(Core::Widget())").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&&)")
      cpg.method.nameExact("use").call.nameExact("~Widget").code.l shouldBe
        List(
          "Core::Widget().~Widget()",
          "Core::Widget(source).~Widget()",
          "Core::Widget().~Widget()",
          "Core::Widget(source).~Widget()",
          "Core::Widget(local).~Widget()",
          "local.~Widget()",
          "source.~Widget()"
        )
    }

    "capture C++ control-flow temporary destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  Widget(Widget&& other) {}
          |  ~Widget();
          |};
          |int consume(Widget&& widget) { return 1; }
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int flow(int n) {
          |  Core::Widget source;
          |  if (Core::consume(Core::Widget())) {
          |    n = n + 1;
          |  }
          |  while (Core::consume(Core::Widget(source))) {
          |    break;
          |  }
          |  for (; Core::consume(Core::Widget()); Core::consume(Core::Widget(source))) {
          |    break;
          |  }
          |  switch (Core::consume(Core::Widget(source))) {
          |  default:
          |    break;
          |  }
          |  return n;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("flow").call.nameExact("consume").methodFullName.l shouldBe
        List.fill(5)("Core.consume:int(Widget&&)")
      cpg.method.nameExact("flow").call.nameExact("Widget").codeExact("Core::Widget()").methodFullName.l shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void()")
      cpg.method.nameExact("flow").call.nameExact("Widget").codeExact("Core::Widget(source)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)", "Core.Widget.Widget:void(Widget&)", "Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("flow").call.nameExact("~Widget").code.l.sorted shouldBe
        List(
          "Core::Widget().~Widget()",
          "Core::Widget().~Widget()",
          "Core::Widget(source).~Widget()",
          "Core::Widget(source).~Widget()",
          "Core::Widget(source).~Widget()",
          "source.~Widget()"
        )
      cpg.method.nameExact("flow").controlStructure.controlStructureType(ControlStructureTypes.IF).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe List("Core::Widget().~Widget()")
      cpg.method.nameExact("flow").controlStructure.controlStructureType(ControlStructureTypes.WHILE).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe List("Core::Widget(source).~Widget()")
      cpg.method.nameExact("flow").controlStructure.controlStructureType(ControlStructureTypes.FOR).ast.isCall
        .nameExact("~Widget")
        .code
        .l
        .sorted shouldBe List("Core::Widget().~Widget()", "Core::Widget(source).~Widget()")
      cpg.method.nameExact("flow").controlStructure.controlStructureType(ControlStructureTypes.SWITCH).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe List("Core::Widget(source).~Widget()")
    }

    "capture C++ logical and ternary temporary destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  Widget(Widget&& other) {}
          |  ~Widget();
          |};
          |int consume(Widget&& widget) { return 1; }
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int mix(int n) {
          |  Core::Widget source;
          |  int both = Core::consume(Core::Widget()) && Core::consume(Core::Widget(source));
          |  int either = Core::consume(Core::Widget(source)) || Core::consume(Core::Widget());
          |  int selected = n ? Core::consume(Core::Widget()) : Core::consume(Core::Widget(source));
          |  return both + either + selected;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("mix").call.nameExact(Operators.logicalAnd).code.l shouldBe
        List("Core::consume(Core::Widget()) && Core::consume(Core::Widget(source))")
      cpg.method.nameExact("mix").call.nameExact(Operators.logicalOr).code.l shouldBe
        List("Core::consume(Core::Widget(source)) || Core::consume(Core::Widget())")
      cpg.method.nameExact("mix").call.nameExact(Operators.conditional).code.l shouldBe
        List("n ? Core::consume(Core::Widget()) : Core::consume(Core::Widget(source))")
      cpg.method.nameExact("mix").call.nameExact("consume").methodFullName.l shouldBe
        List.fill(6)("Core.consume:int(Widget&&)")
      cpg.method.nameExact("mix").call.nameExact("Widget").codeExact("Core::Widget()").methodFullName.l shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void()", "Core.Widget.Widget:void()")
      cpg.method.nameExact("mix").call.nameExact("Widget").codeExact("Core::Widget(source)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)", "Core.Widget.Widget:void(Widget&)", "Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("mix").call.nameExact("~Widget").code.l.sorted shouldBe
        List(
          "Core::Widget().~Widget()",
          "Core::Widget().~Widget()",
          "Core::Widget().~Widget()",
          "Core::Widget(source).~Widget()",
          "Core::Widget(source).~Widget()",
          "Core::Widget(source).~Widget()",
          "source.~Widget()"
        )
    }

    "capture C++ throw temporary destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  Widget(Widget&& other) {}
          |  ~Widget();
          |};
          |int consume(Widget&& widget) { return 1; }
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int fail(int n) {
          |  Core::Widget source;
          |  if (n) {
          |    throw Core::consume(Core::Widget(source));
          |  }
          |  throw Core::consume(Core::Widget());
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("fail").controlStructure.controlStructureType(ControlStructureTypes.THROW).code.l shouldBe
        List("throw Core::consume(Core::Widget(source))", "throw Core::consume(Core::Widget())")
      cpg.method.nameExact("fail").call.nameExact("consume").methodFullName.l shouldBe
        List("Core.consume:int(Widget&&)", "Core.consume:int(Widget&&)")
      cpg.method.nameExact("fail").call.nameExact("Widget").codeExact("Core::Widget()").methodFullName.l shouldBe
        List("Core.Widget.Widget:void()")
      cpg.method.nameExact("fail").call.nameExact("Widget").codeExact("Core::Widget(source)").methodFullName.l shouldBe
        List("Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("fail").call.nameExact("~Widget").code.l.sorted shouldBe
        List(
          "Core::Widget().~Widget()",
          "Core::Widget(source).~Widget()",
          "source.~Widget()",
          "source.~Widget()"
        )
      cpg.method.nameExact("fail").controlStructure.controlStructureType(ControlStructureTypes.IF).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe List("Core::Widget(source).~Widget()", "source.~Widget()")
    }

    "capture C++ try catch statements" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  ~Widget();
          |};
          |int consume(Widget& widget) { return 1; }
          |void handle(Widget& widget) {}
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |void guarded(int n) {
          |  Core::Widget source;
          |  try {
          |    Core::Widget local;
          |    throw Core::consume(source);
          |  } catch (Core::Widget caught) {
          |    Core::handle(caught);
          |  } catch (...) {
          |    n = 0;
          |  }
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("guarded").controlStructure.controlStructureType(ControlStructureTypes.TRY).l) {
        case List(tryNode) =>
          tryNode.tryBodyOut.astChildren.isControlStructure.controlStructureType(ControlStructureTypes.THROW).code.l shouldBe
            List("throw Core::consume(source)")
          inside(tryNode.catchBodyOut.isControlStructure.l) { case List(typedCatch, catchAll) =>
            typedCatch.controlStructureType shouldBe ControlStructureTypes.CATCH
            catchAll.controlStructureType shouldBe ControlStructureTypes.CATCH
            typedCatch.ast.isLocal.code.l shouldBe List("Core::Widget caught")
            typedCatch.ast.isCall.nameExact("handle").methodFullName.l shouldBe List("Core.handle:void(Widget&)")
            typedCatch.ast.isCall.nameExact("~Widget").code.l shouldBe List("caught.~Widget()")
            catchAll.ast.isLocal.l shouldBe Nil
            catchAll.ast.isCall.nameExact(Operators.assignment).code.l shouldBe List("n = 0")
          }
      }
      cpg.method.nameExact("guarded").call.nameExact("consume").methodFullName.l shouldBe
        List("Core.consume:int(Widget&)")
      cpg.method.nameExact("guarded").call.nameExact("~Widget").code.l.sorted shouldBe
        List("caught.~Widget()", "local.~Widget()", "source.~Widget()")
    }

    "capture C++ jump destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  ~Widget();
          |};
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |int jumps(int n) {
          |  Core::Widget outer;
          |  for (Core::Widget guard(outer); n; n = n - 1) {
          |    Core::Widget body(outer);
          |    if (n == 1) {
          |      Core::Widget skipped(outer);
          |      continue;
          |    }
          |    if (n == 2) {
          |      Core::Widget stopped(outer);
          |      break;
          |    }
          |  }
          |  return 0;
          |}
          |int returns(int n) {
          |  Core::Widget outer;
          |  if (n) {
          |    Core::Widget scoped(outer);
          |    return 1;
          |  }
          |  return 0;
          |}
          |int switches(int n) {
          |  Core::Widget outer;
          |  switch (n) {
          |  case 1:
          |    Core::Widget caseLocal(outer);
          |    break;
          |  default:
          |    break;
          |  }
          |  return 1;
          |}
          |int switchContinue(int n) {
          |  Core::Widget outer;
          |  while (n) {
          |    Core::Widget body(outer);
          |    switch (n) {
          |    case 1:
          |      Core::Widget inSwitch(outer);
          |      continue;
          |    default:
          |      break;
          |    }
          |    break;
          |  }
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("jumps").call.nameExact("~Widget").code.l.sorted shouldBe
        List(
          "body.~Widget()",
          "body.~Widget()",
          "body.~Widget()",
          "guard.~Widget()",
          "outer.~Widget()",
          "skipped.~Widget()",
          "stopped.~Widget()"
        )
      inside(cpg.method.nameExact("jumps").controlStructure.controlStructureType(ControlStructureTypes.IF).l) {
        case List(continueIf, breakIf) =>
          continueIf.ast.isCall.nameExact("~Widget").code.l shouldBe List("skipped.~Widget()", "body.~Widget()")
          breakIf.ast.isCall.nameExact("~Widget").code.l shouldBe List("stopped.~Widget()", "body.~Widget()")
      }

      cpg.method.nameExact("returns").call.nameExact("~Widget").code.l.sorted shouldBe
        List("outer.~Widget()", "outer.~Widget()", "scoped.~Widget()")
      cpg.method.nameExact("returns").controlStructure.controlStructureType(ControlStructureTypes.IF).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe List("scoped.~Widget()", "outer.~Widget()")

      cpg.method.nameExact("switches").call.nameExact("~Widget").code.l shouldBe
        List("caseLocal.~Widget()", "outer.~Widget()")
      cpg.method.nameExact("switches").controlStructure.controlStructureType(ControlStructureTypes.SWITCH).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe Nil

      cpg.method.nameExact("switchContinue").call.nameExact("~Widget").code.l.sorted shouldBe
        List("body.~Widget()", "body.~Widget()", "inSwitch.~Widget()", "inSwitch.~Widget()", "outer.~Widget()")
      cpg.method.nameExact("switchContinue").controlStructure.controlStructureType(ControlStructureTypes.SWITCH).ast.isCall
        .nameExact("~Widget")
        .code
        .l shouldBe List("inSwitch.~Widget()", "body.~Widget()")
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

    "capture C++ heap constructor and delete destructors" in {
      val cpg = code(
        """
          |namespace Core {
          |struct Widget {
          |  Widget();
          |  Widget(Widget &other);
          |  ~Widget();
          |};
          |}
          |Core::Widget *heap(Core::Widget &source) {
          |  Core::Widget *first = new Core::Widget();
          |  Core::Widget *second = new Core::Widget{source};
          |  delete first;
          |  delete second;
          |  return second;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("heap").local.nameExact("first").typeFullName.l shouldBe List("Core.Widget*")
      cpg.method.nameExact("heap").local.nameExact("second").typeFullName.l shouldBe List("Core.Widget*")
      cpg.method.nameExact("heap").call.nameExact(Operators.alloc).code.l.sorted shouldBe
        List("new Core::Widget()", "new Core::Widget{source}")
      cpg.method.nameExact("heap").call.nameExact("Widget").code.l.sorted shouldBe
        List("Core.Widget.Widget()", "Core.Widget.Widget(source)")
      cpg.method.nameExact("heap").call.nameExact("Widget").methodFullName.l.sorted shouldBe
        List("Core.Widget.Widget:void()", "Core.Widget.Widget:void(Widget&)")
      cpg.method.nameExact("heap").call.nameExact("~Widget").code.l.sorted shouldBe
        List("first->~Widget()", "second->~Widget()")
      cpg.method.nameExact("heap").call.nameExact("~Widget").methodFullName.l shouldBe
        List("Core.Widget.~Widget:void()", "Core.Widget.~Widget:void()")
      cpg.method.nameExact("heap").call.nameExact(Operators.delete).code.l shouldBe List("delete first", "delete second")
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

    "normalize truthy scalar control conditions from the Rust parser backend" in {
      val cpg = code("""
          |int truthy(int x, int *ptr) {
          |  if (x) {
          |    x = x + 1;
          |  }
          |  while (ptr) {
          |    ptr = 0;
          |  }
          |  do {
          |    x = x - 1;
          |  } while (x);
          |  for (; x; x = x - 1) {
          |    x = x + 2;
          |  }
          |  if (1) {
          |    x = x + 3;
          |  }
          |  if (x < 0) {
          |    x = 0;
          |  }
          |  if (!ptr) {
          |    x = x + 4;
          |  }
          |  return x;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("truthy").ifBlock.condition.code.l shouldBe List("x != 0", "1", "x < 0", "!ptr")
      cpg.method.nameExact("truthy").whileBlock.condition.code.l shouldBe List("ptr != NULL")
      cpg.method.nameExact("truthy").doBlock.condition.code.l shouldBe List("x != 0")
      cpg.method.nameExact("truthy").forBlock.condition.code.l shouldBe List("x != 0")
      cpg.method.nameExact("truthy").call.nameExact(Operators.notEquals).code.l shouldBe
        List("x != 0", "ptr != NULL", "x != 0", "x != 0")
    }

    "capture C++ selection initializers from the Rust parser backend" in {
      val cpg = code("""
          |struct Pair {
          |  int first;
          |  int second;
          |};
          |Pair makePair();
          |
          |int seed(int x) {
          |  return x;
          |}
          |
          |int use(int n) {
          |  if (int x = seed(n); x) {
          |    n = x;
          |  }
          |  if (int q = seed(n)) {
          |    n = q;
          |  }
          |  if (auto [first, second] = makePair(); first) {
          |    n = second;
          |  }
          |  while (int w = seed(n)) {
          |    n = w;
          |    break;
          |  }
          |  while (auto [left, right] = makePair()) {
          |    n = left + right;
          |    break;
          |  }
          |  switch (int y = seed(n); y) {
          |  case 1:
          |    return y;
          |  default:
          |    return n;
          |  }
          |}
          |""".stripMargin, "Test0.cpp").withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.local.name.l should contain allElementsOf List("x", "q", "first", "second", "w", "left", "right", "y")
      cpg.local.nameExact("x").typeFullName.l shouldBe List("int")
      cpg.local.nameExact("q").typeFullName.l shouldBe List("int")
      cpg.local.nameExact("first").typeFullName.l shouldBe List("int")
      cpg.local.nameExact("second").typeFullName.l shouldBe List("int")
      cpg.local.nameExact("w").typeFullName.l shouldBe List("int")
      cpg.local.nameExact("left").typeFullName.l shouldBe List("int")
      cpg.local.nameExact("right").typeFullName.l shouldBe List("int")
      cpg.local.nameExact("y").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("use").ifBlock.condition.code.l should contain allElementsOf List("x != 0", "q != 0", "first != 0")
      cpg.method.nameExact("use").whileBlock.condition.code.l should contain("w != 0")
      cpg.method.nameExact("use").controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).condition.code.l shouldBe
        List("y")
      inside(cpg.method.nameExact("use").ifBlock.l) { case List(initStatementIf, conditionDeclarationIf, structuredIf) =>
        initStatementIf.condition.ast.isLocal.name.l shouldBe Nil
        initStatementIf.condition.ast.isCall.nameExact("seed").code.l shouldBe Nil
        conditionDeclarationIf.condition.ast.isLocal.name.l shouldBe List("q")
        conditionDeclarationIf.condition.ast.isCall.nameExact("seed").code.l shouldBe List("seed(n)")
        structuredIf.condition.ast.isLocal.name.l shouldBe Nil
        structuredIf.condition.ast.isIdentifier.nameExact("first").refsTo.l shouldBe
          List(cpg.method.nameExact("use").local.nameExact("first").head)
      }
      inside(cpg.method.nameExact("use").whileBlock.l) { case List(simpleWhile, structuredWhile) =>
        simpleWhile.condition.ast.isLocal.name.l shouldBe List("w")
        simpleWhile.condition.ast.isCall.nameExact("seed").code.l shouldBe List("seed(n)")
        structuredWhile.condition.ast.isLocal.name.l should contain allElementsOf List("left", "right")
        structuredWhile.condition.ast.isCall.nameExact("makePair").code.l shouldBe List("makePair()")
        structuredWhile.condition.code.l.head should startWith("<tmp>")
      }
      inside(cpg.method.nameExact("use").controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).l) {
        case List(switchBlock) =>
          switchBlock.condition.ast.isLocal.name.l shouldBe Nil
          switchBlock.condition.ast.isCall.nameExact("seed").code.l shouldBe Nil
      }
      cpg.method.nameExact("use").call.nameExact("seed").methodFullName.l shouldBe List.fill(4)("seed:int(int)")
      cpg.method.nameExact("use").call.nameExact("makePair").methodFullName.l shouldBe List.fill(2)("makePair:Pair()")
      val assignmentCodes = cpg.method.nameExact("use").call.nameExact(Operators.assignment).code.l
      assignmentCodes should contain allElementsOf List(
        "x = seed(n)",
        "q = seed(n)",
        "w = seed(n)",
        "y = seed(n)",
        "n = x",
        "n = q",
        "n = second",
        "n = w"
      )
      assignmentCodes.exists(_.startsWith("first = ")) shouldBe true
      assignmentCodes.exists(_.startsWith("second = ")) shouldBe true
      assignmentCodes.exists(_.startsWith("left = ")) shouldBe true
      assignmentCodes.exists(_.startsWith("right = ")) shouldBe true
    }

    "capture C++ while condition declaration destructor cleanup" in {
      val cpg = code(
        """
          |namespace Core {
          |class Widget {
          |public:
          |  Widget();
          |  Widget(const Widget& other) {}
          |  ~Widget();
          |  operator bool() const { return true; }
          |};
          |Widget make();
          |}
          |Core::Widget::Widget() {}
          |Core::Widget::~Widget() {}
          |Core::Widget Core::make() {
          |  Core::Widget widget;
          |  return widget;
          |}
          |int conditionLifetime(int n) {
          |  while (Core::Widget guard = Core::make()) {
          |    if (n == 1) {
          |      continue;
          |    }
          |    if (n == 2) {
          |      break;
          |    }
          |    n = n - 1;
          |  }
          |  return n;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("conditionLifetime").whileBlock.l) { case List(whileBlock) =>
        whileBlock.condition.ast.isLocal.name.l should contain("guard")
        whileBlock.condition.ast.isCall.nameExact("make").code.l shouldBe List("Core::make()")
        inside(whileBlock.condition.ast.isCall.nameExact("operator bool").codeExact("guard.operator bool()").l) {
          case List(operatorBool) =>
            operatorBool.methodFullName shouldBe "Core.Widget.operator bool:bool()<const>"
            operatorBool.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
            operatorBool.typeFullName shouldBe "bool"
            operatorBool.argument.code.l shouldBe List("guard")
        }
        whileBlock.condition.ast.isCall.nameExact(Operators.notEquals).codeExact("guard != 0").l shouldBe Nil
        whileBlock.ast.isCall.nameExact("~Widget").code.l.sorted shouldBe
          List("guard.~Widget()", "guard.~Widget()")
      }
      inside(cpg.method.nameExact("conditionLifetime").controlStructure.controlStructureType(ControlStructureTypes.IF).l) {
        case List(continueIf, breakIf) =>
          continueIf.ast.isCall.nameExact("~Widget").code.l shouldBe List("guard.~Widget()")
          breakIf.ast.isCall.nameExact("~Widget").code.l shouldBe Nil
      }
      cpg.method.nameExact("conditionLifetime").call.nameExact("~Widget").code.l.sorted shouldBe
        List("guard.~Widget()", "guard.~Widget()", "guard.~Widget()")
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

    "capture C++ range-based for loops from the Rust parser backend" in {
      val cpg = code(
        """
          |int sum(int *items) {
          |  int total = 0;
          |  for (int value : items) {
          |    total += value;
          |  }
          |  return total;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("sum").controlStructure.controlStructureType(ControlStructureTypes.FOR).l) {
        case List(rangeFor) =>
          rangeFor.code shouldBe "for (int value : items) {\n    total += value;\n  }"
          rangeFor.condition.code.l shouldBe List("items != NULL")
      }
      cpg.method.nameExact("sum").local.nameExact("value").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("sum").call.nameExact(Operators.assignmentPlus).code.l shouldBe List("total += value")
      cpg.identifier.nameExact("items").refsTo.l shouldBe
        List(cpg.method.nameExact("sum").parameter.nameExact("items").head)
      cpg.identifier.nameExact("value").refsTo.l shouldBe List(cpg.method.nameExact("sum").local.nameExact("value").head)
    }

    "capture C++ range-based for loops with initializers from the Rust parser backend" in {
      val cpg = code(
        """
          |void each(int *list) {
          |  for (auto v = list; auto& e : v) {
          |    e += 1;
          |  }
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("each").controlStructure.controlStructureType(ControlStructureTypes.FOR).l) {
        case List(rangeFor) =>
          rangeFor.condition.code.l shouldBe List("v != NULL")
          rangeFor.astChildren.isLocal.name.l shouldBe List("v", "e")
      }
      cpg.method.nameExact("each").local.nameExact("v").typeFullName.l shouldBe List("int*")
      cpg.method.nameExact("each").local.nameExact("e").typeFullName.l shouldBe List("auto&")
      cpg.call.nameExact(Operators.assignment).code.l shouldBe List("*v = list")
      cpg.call.nameExact(Operators.assignmentPlus).code.l shouldBe List("e += 1")
      cpg.identifier.nameExact("list").refsTo.l shouldBe
        List(cpg.method.nameExact("each").parameter.nameExact("list").head)
      cpg.identifier.nameExact("v").refsTo.l should contain(cpg.method.nameExact("each").local.nameExact("v").head)
      cpg.identifier.nameExact("e").refsTo.l should contain(cpg.method.nameExact("each").local.nameExact("e").head)
    }

    "capture C++ range-based for loops with structured bindings from the Rust parser backend" in {
      val cpg = code(
        """
          |struct Pair {
          |  int first;
          |  int second;
          |};
          |int sumPairs(Pair *pairs) {
          |  int total = 0;
          |  for (auto [first, second] : pairs) {
          |    total += first + second;
          |  }
          |  return total;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("sumPairs").controlStructure.controlStructureType(ControlStructureTypes.FOR).l) {
        case List(rangeFor) =>
          rangeFor.condition.code.l shouldBe List("pairs != NULL")
      }
      val temp = cpg.method.nameExact("sumPairs").local.filter(_.name.startsWith("<tmp>")).head
      temp.typeFullName shouldBe "Pair*"
      cpg.method.nameExact("sumPairs").local.nameExact("first").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("sumPairs").local.nameExact("second").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("sumPairs").call.nameExact(Operators.assignment).code.l should contain allElementsOf List(
        s"*${temp.name} = pairs",
        s"first = ${temp.name}.first",
        s"second = ${temp.name}.second"
      )
      cpg.method.nameExact("sumPairs").call.nameExact(Operators.fieldAccess).code.l should contain theSameElementsAs List(
        s"${temp.name}.first",
        s"${temp.name}.second"
      )
      cpg.method.nameExact("sumPairs").call.nameExact(Operators.assignmentPlus).code.l shouldBe
        List("total += first + second")
      cpg.identifier.nameExact("first").refsTo.dedup.l shouldBe List(
        cpg.method.nameExact("sumPairs").local.nameExact("first").head
      )
      cpg.identifier.nameExact("second").refsTo.dedup.l shouldBe List(
        cpg.method.nameExact("sumPairs").local.nameExact("second").head
      )
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
          |  int alignment = alignof(int);
          |  y += x > 0 ? x : -x;
          |  return y;
          |}
          |""".stripMargin).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.call.nameExact(Operators.assignment).code.l should contain theSameElementsAs List(
        "y = (int)sizeof(x)",
        "alignment = alignof(int)"
      )
      cpg.call.nameExact(Operators.cast).code.l shouldBe List("(int)sizeof(x)")
      cpg.call.nameExact(Operators.sizeOf).code.l should contain theSameElementsAs List("sizeof(x)", "alignof(int)")
      cpg.call.nameExact(Operators.assignmentPlus).code.l shouldBe List("y += x > 0 ? x : -x")
      cpg.call.nameExact(Operators.conditional).code.l shouldBe List("x > 0 ? x : -x")
      cpg.call.nameExact(Operators.greaterThan).code.l shouldBe List("x > 0")
      cpg.call.nameExact(Operators.minus).code.l shouldBe List("-x")
    }

    "capture C++ named casts from the Rust parser backend" in {
      val cpg = code(
        """
          |int casts(float x, void *ptr) {
          |  int a = static_cast<int>(x);
          |  int b = const_cast<int>(a);
          |  int c = dynamic_cast<int>(b);
          |  int d = reinterpret_cast<int>(ptr);
          |  return a + b + c + d;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.call.nameExact(Operators.cast).code.l shouldBe
        List("static_cast<int>(x)", "const_cast<int>(a)", "dynamic_cast<int>(b)", "reinterpret_cast<int>(ptr)")
      cpg.call.codeExact("static_cast<int>(x)").name.l shouldBe List(Operators.cast)
      cpg.call.codeExact("const_cast<int>(a)").name.l shouldBe List(Operators.cast)
      cpg.call.codeExact("dynamic_cast<int>(b)").name.l shouldBe List(Operators.cast)
      cpg.call.codeExact("reinterpret_cast<int>(ptr)").name.l shouldBe List(Operators.cast)
      cpg.call.nameExact(Operators.cast).typeFullName.l shouldBe List("int", "int", "int", "int")
    }

    "capture C++ three-way comparison from the Rust parser backend" in {
      val cpg = code(
        """
          |bool foo() {
          |  bool x = 1 <=> 2;
          |  return x;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.local.nameExact("x").typeFullName.l shouldBe List("bool")
      cpg.call.nameExact(Operators.assignment).code.l shouldBe List("x = 1 <=> 2")
      inside(cpg.call.codeExact("1 <=> 2").l) { case List(compare) =>
        compare.name shouldBe Operators.compare
        compare.methodFullName shouldBe Operators.compare
        compare.argument.code.l shouldBe List("1", "2")
      }
    }

    "capture C++ boolean and nullptr literals from the Rust parser backend" in {
      val cpg = code(
        """
          |bool flags(int *ptr) {
          |  bool ok = true;
          |  bool nope = false;
          |  return ptr != nullptr;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.literal.code.l should contain allElementsOf List("true", "false", "nullptr")
      cpg.literal.codeExact("true").typeFullName.l shouldBe List("bool")
      cpg.literal.codeExact("false").typeFullName.l shouldBe List("bool")
      cpg.literal.codeExact("nullptr").typeFullName.l shouldBe List("std.nullptr_t")
      cpg.identifier.nameExact("nullptr").l shouldBe Nil
      cpg.call.nameExact(Operators.notEquals).code.l shouldBe List("ptr != nullptr")
    }

    "capture C++ extended string literals from the Rust parser backend" in {
      val cpg = code(
        """
          |const char *strings() {
          |  const char *raw = R"(hello)";
          |  const char *joined = "a" "b";
          |  auto tagged = 42_km;
          |  return raw;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.literal.code.l should contain allElementsOf List("R\"(hello)\"", "\"a\" \"b\"", "42_km")
      cpg.literal.codeExact("R\"(hello)\"").typeFullName.l shouldBe List("char[6]")
      cpg.literal.codeExact("\"a\" \"b\"").typeFullName.l shouldBe List("char[3]")
      cpg.literal.codeExact("42_km").typeFullName.l shouldBe List(Defines.Any)
      cpg.identifier.codeExact("42_km").l shouldBe Nil
    }

    "capture C++ UTF-8 string and character literals from the Rust parser backend" in {
      val cpg = code(
        """
          |char8_t utf8_str[] = u8"abcde";
          |void chars() {
          |  char x = u8'x';
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.local.nameExact("utf8_str").typeFullName.l shouldBe List("char8_t[6]")
      inside(cpg.call.nameExact(Operators.assignment).codeExact("""utf8_str[] = u8"abcde"""").l) {
        case List(assignmentCall) =>
          assignmentCall.argument.isIdentifier.code.l shouldBe List("utf8_str[]")
          assignmentCall.argument.isLiteral.code.l shouldBe List("""u8"abcde"""")
      }
      cpg.literal.codeExact("""u8"abcde"""").typeFullName.l shouldBe List("char[6]")
      cpg.method.nameExact("chars").local.nameExact("x").typeFullName.l shouldBe List("char")
      cpg.method.nameExact("chars").literal.codeExact("u8'x'").typeFullName.l shouldBe List("char")
    }

    "capture C++ offsetof expressions from the Rust parser backend" in {
      val cpg = code(
        """
          |struct Pair {
          |  int first;
          |  int second;
          |};
          |int offset() {
          |  return offsetof(Pair, second);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.call.nameExact("offsetof").l) { case List(offsetOf) =>
        offsetOf.code shouldBe "offsetof(Pair, second)"
        offsetOf.methodFullName shouldBe "offsetof"
        offsetOf.argument.code.l shouldBe List("Pair", "second")
      }
      cpg.literal.code.l should contain allElementsOf List("Pair", "second")
      cpg.identifier.codeExact("second").l shouldBe Nil
    }

    "capture C++ fold expressions from the Rust parser backend" in {
      val cpg = code(
        """
          |template <typename... Args>
          |bool logicalAnd(Args... args) {
          |  return (true && ... && args);
          |}
          |template <typename... Args>
          |auto sum(Args... args) {
          |  return (... + args);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("logicalAnd").ast.isReturn.astChildren.isCall.nameExact("<operator>.fold").l) {
        case List(retExpr) =>
          retExpr.typeFullName shouldBe "bool"
          retExpr.code shouldBe "(true && ... && args)"
          retExpr.argument.isMethodRef.code.l shouldBe List(Operators.logicalAnd)
          retExpr.argument.code.l shouldBe List(Operators.logicalAnd, "true", "args")
      }
      inside(cpg.method.nameExact("sum").ast.isReturn.astChildren.isCall.nameExact("<operator>.fold").l) {
        case List(retExpr) =>
          retExpr.typeFullName shouldBe "Args"
          retExpr.code shouldBe "(... + args)"
          retExpr.argument.isMethodRef.code.l shouldBe List(Operators.addition)
          retExpr.argument.code.l shouldBe List(Operators.addition, "args", "args")
      }
    }

    "capture C++ parameter pack expansions from the Rust parser backend" in {
      val cpg = code(
        """
          |void foo(int x, int*... args) {
          |  foo(x, args...);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("foo").l) { case List(fooMethod) =>
        fooMethod.signature shouldBe "void(int,int*)"
        inside(fooMethod.parameter.l) { case List(x, args) =>
          x.name shouldBe "x"
          x.typeFullName shouldBe "int"
          x.isVariadic shouldBe false
          args.name shouldBe "args"
          args.code shouldBe "int*... args"
          args.typeFullName shouldBe "int*"
          args.isVariadic shouldBe true
        }
        inside(fooMethod.call.nameExact("foo").l) { case List(fooCall) =>
          fooCall.code shouldBe "foo(x, args...)"
          fooCall.argument.code.l shouldBe List("x", "args")
        }
        cpg.identifier.codeExact("args").refsTo.l shouldBe List(fooMethod.parameter.nameExact("args").head)
      }
    }

    "capture C++ lambda parameter-pack init captures from the Rust parser backend" in {
      val cpg = code(
        """
          |template <typename... Args>
          |auto f1(Args&&... args) {
          |  return [...args = std::forward<Args>(args)] {};
          |}
          |
          |template <typename... Args>
          |auto f2(Args&&... args) {
          |  return [&...args = std::forward<Args>(args)] {};
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val f1Lambda = s"${cpg.method.nameExact("f1").fullName.head}.<lambda>0:void()"
      val f2Lambda = s"${cpg.method.nameExact("f2").fullName.head}.<lambda>1:void()"

      cpg.method.nameExact("f1", "f2").signature.sorted.l shouldBe List("auto(Args&&)", "auto(Args&&)")
      cpg.method.nameExact("f1").ast.isReturn.astChildren.isMethodRef.methodFullName.l shouldBe List(f1Lambda)
      cpg.method.nameExact("f2").ast.isReturn.astChildren.isMethodRef.methodFullName.l shouldBe List(f2Lambda)
      cpg.method.fullNameExact(f1Lambda).local.nameExact("args").typeFullName.l shouldBe List("Args&&")
      cpg.method.fullNameExact(f2Lambda).local.nameExact("args").typeFullName.l shouldBe List("Args&&")
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$f1Lambda:args")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_VALUE)
      cpg.closureBinding.filter(_.closureBindingId.contains(s"$f2Lambda:args")).evaluationStrategy.l shouldBe
        List(EvaluationStrategies.BY_REFERENCE)
    }

    "capture C++ classic varargs from the Rust parser backend" in {
      val cpg = code(
        """
          |int foo(const char *a, ...){ return 0; }
          |int bar(const char *a...){ return 0; }
          |
          |void main() {
          |  foo("a", "b", "c");
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("foo").l) { case List(fooMethod) =>
        fooMethod.signature shouldBe "int(char*,...)"
        inside(fooMethod.parameter.l) { case List(a, ellipsis) =>
          a.name shouldBe "a"
          a.code shouldBe "const char *a"
          a.typeFullName shouldBe "char*"
          a.isVariadic shouldBe false
          ellipsis.name shouldBe "<param>2"
          ellipsis.code shouldBe "<param>2..."
          ellipsis.typeFullName shouldBe "char*"
          ellipsis.isVariadic shouldBe true
        }
      }
      inside(cpg.method.nameExact("bar").l) { case List(barMethod) =>
        barMethod.signature shouldBe "int(char*,...)"
        inside(barMethod.parameter.l) { case List(a, ellipsis) =>
          a.name shouldBe "a"
          a.code shouldBe "const char *a"
          a.typeFullName shouldBe "char*"
          a.isVariadic shouldBe false
          ellipsis.name shouldBe "<param>2"
          ellipsis.code shouldBe "<param>2..."
          ellipsis.typeFullName shouldBe "char*"
          ellipsis.isVariadic shouldBe true
        }
      }
      inside(cpg.call.nameExact("foo").l) { case List(fooCall) =>
        fooCall.methodFullName shouldBe "foo:int(char*,...)"
        fooCall.signature shouldBe "int(char*,...)"
      }
    }

    "capture C++ trailing return types from the Rust parser backend" in {
      val cpg = code(
        """
          |auto f(int x) -> long { return x; }
          |auto ptr(int *p) -> int* { return p; }
          |auto value() -> decltype(1 + 2);
          |struct Widget {
          |  auto size() const -> int;
          |};
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("f").l) { case List(fMethod) =>
        fMethod.signature shouldBe "long(int)"
        fMethod.methodReturn.typeFullName shouldBe "long"
        fMethod.parameter.nameExact("x").typeFullName.l shouldBe List("int")
      }
      inside(cpg.method.nameExact("ptr").l) { case List(ptrMethod) =>
        ptrMethod.signature shouldBe "int*(int*)"
        ptrMethod.methodReturn.typeFullName shouldBe "int*"
        ptrMethod.parameter.nameExact("p").typeFullName.l shouldBe List("int*")
      }
      inside(cpg.method.nameExact("value").l) { case List(valueMethod) =>
        valueMethod.signature shouldBe "decltype(1 + 2)()"
        valueMethod.methodReturn.typeFullName shouldBe "decltype(1 + 2)"
        valueMethod.isExternal shouldBe true
      }
      inside(cpg.method.nameExact("size").l) { case List(sizeMethod) =>
        sizeMethod.signature shouldBe "int()<const>"
        sizeMethod.methodReturn.typeFullName shouldBe "int"
        sizeMethod.isExternal shouldBe true
      }
    }

    "capture C++ decltype qualified field access from the Rust parser backend" in {
      val cpg = code(
        """
          |void method() {
          |  int local = 1;
          |  constexpr bool is_std_array_v = decltype(local)::value;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.local.nameExact("is_std_array_v").typeFullName.l shouldBe List("bool")
      inside(cpg.call.codeExact("decltype(local)::value").l) { case List(fieldAccess) =>
        fieldAccess.methodFullName shouldBe Operators.fieldAccess
        fieldAccess.argument(2).code shouldBe "value"
        inside(fieldAccess.argument(1)) { case typeOf: io.shiftleft.codepropertygraph.generated.nodes.Call =>
          typeOf.code shouldBe "decltype(local)"
          typeOf.methodFullName shouldBe Defines.OperatorTypeOf
          typeOf.argument(1).code shouldBe "local"
        }
      }
    }

    "capture C++20 concept requires expressions from the Rust parser backend" in {
      val cpg = code(
        """
          |template <typename T>
          |concept callable = requires (T f) { f(); };
          |
          |template <typename T>
          |  requires requires (T x) { x + x; }
          |T add(T a, T b) {
          |  return a + b;
          |}
          |
          |template <typename T>
          |  requires callable<T>
          |void f(T v);
          |
          |void f4(my_concept auto v);
          |
          |template <my_concept auto v>
          |void f5();
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("callable").l shouldBe Nil
      cpg.method.nameExact("add").signature.l shouldBe List("T(T,T)")
      cpg.method.nameExact("f").external.signature.l shouldBe List("void(T)")
      inside(cpg.method.nameExact("f4").external.l) { case List(f4Method) =>
        f4Method.signature shouldBe "void(my_concept auto)"
        inside(f4Method.parameter.l) { case List(parameter) =>
          parameter.name shouldBe "v"
          parameter.code shouldBe "my_concept auto v"
          parameter.typeFullName shouldBe "my_concept auto"
        }
      }
      cpg.method.nameExact("f5").external.signature.l shouldBe List("void()")
      inside(cpg.method.nameExact("requires").l) { case List(requiresMethod) =>
        requiresMethod.fullName shouldBe "requires:requires(T)"
        requiresMethod.signature shouldBe "requires(T)"
        requiresMethod.code shouldBe "requires"
        requiresMethod.isExternal shouldBe true
        requiresMethod.methodReturn.typeFullName shouldBe "requires"
        inside(requiresMethod.parameter.l) { case List(parameter) =>
          parameter.name shouldBe "f"
          parameter.code shouldBe "T f"
          parameter.typeFullName shouldBe "T"
        }
      }
    }

    "capture C++20 coroutine statements from the Rust parser backend" in {
      val cpg = code(
        """
          |int main() {
          |  co_await x();
          |  co_return y();
          |}
          |
          |generator<int> range(int start, int end) {
          |  while (start < end) {
          |    co_yield start;
          |    start++;
          |  }
          |}
          |
          |task<void> echo(socket s) {
          |  auto data = co_await s.async_read();
          |  co_await async_write(s, data);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.nameExact("main").call.nameExact("<operator>.await").l) { case List(awaitCall) =>
        awaitCall.code shouldBe "co_await x()"
        awaitCall.argument.isCall.code.l shouldBe List("x()")
      }
      inside(cpg.method.nameExact("main").ast.isReturn.codeExact("co_return y()").l) { case List(returnNode) =>
        returnNode.astChildren.isCall.code.l shouldBe List("y()")
      }
      inside(cpg.method.nameExact("range").call.nameExact("<operator>.yield").l) { case List(yieldCall) =>
        yieldCall.code shouldBe "co_yield start"
        yieldCall.argument.isIdentifier.code.l shouldBe List("start")
      }

      cpg.method.nameExact("echo").local.nameExact("data").typeFullName.l shouldBe List("auto")
      cpg.method.nameExact("echo").call.nameExact("<operator>.await").code.l should contain theSameElementsAs
        List("co_await s.async_read()", "co_await async_write(s, data)")
      cpg.method.nameExact("echo").call.codeExact("s.async_read()").size shouldBe 1
      cpg.method.nameExact("echo").call.codeExact("async_write(s, data)").size shouldBe 1
    }

    "capture C++20 likely and unlikely attributed statements from the Rust parser backend" in {
      val cpg = code(
        """
          |void foo() {
          |  switch (n) {
          |    case 1:
          |      case1();
          |      break;
          |    [[likely]] case 2:
          |      case2();
          |      break;
          |  }
          |
          |  if (random > 0) [[likely]] {
          |    likelyIf();
          |  }
          |
          |  while (unlikely_truthy_condition) [[unlikely]] {
          |    unlikelyWhile();
          |  }
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method
        .nameExact("foo")
        .controlStructure
        .controlStructureTypeExact(ControlStructureTypes.SWITCH)
        .ast
        .collectAll[JumpTarget]
        .code
        .l shouldBe List("case 1:", "[[likely]] case 2:")

      cpg.method.nameExact("foo").call.code.l should contain allElementsOf List(
        "case1()",
        "case2()",
        "random > 0",
        "likelyIf()",
        "unlikelyWhile()"
      )
      cpg.method.nameExact("foo").whileBlock.condition.code.l shouldBe List("unlikely_truthy_condition != 0")
      cpg.method.nameExact("foo").ast.isIdentifier.code.l.should(contain("unlikely_truthy_condition"))
      cpg.method.nameExact("foo").ast.isIdentifier.code(".*\\[\\[(likely|unlikely)\\]\\].*").l shouldBe Nil
    }

    "capture C++20 using enum switch cases from the Rust parser backend" in {
      val cpg = code(
        """
          |enum class rgba_color_channel { red, green, blue, alpha };
          |
          |int to_int(rgba_color_channel my_channel) {
          |  switch (my_channel) {
          |    using enum rgba_color_channel;
          |    case red:   return 1;
          |    case green: return 2;
          |    case blue:  return 3;
          |    case alpha: return 4;
          |  }
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("to_int").controlStructure.controlStructureTypeExact(ControlStructureTypes.SWITCH).size shouldBe 1
      cpg.jumpTarget.code.l shouldBe List(
        "case red:",
        "case green:",
        "case blue:",
        "case alpha:"
      )
      cpg.call.nameExact(Operators.fieldAccess).code.l shouldBe List(
        "rgba_color_channel.red",
        "rgba_color_channel.green",
        "rgba_color_channel.blue",
        "rgba_color_channel.alpha"
      )
      cpg.identifier.codeExact("using enum rgba_color_channel").l shouldBe Nil
      cpg.identifier.codeExact("using enum rgba_color_channel;").l shouldBe Nil
    }

    "capture C++20 explicit bool constructor templates from the Rust parser backend" in {
      val cpg = code(
        """
          |struct foo {
          |  template <typename T>
          |  explicit(!std::is_integral_v<T>) foo(T) {}
          |};
          |
          |void use() {
          |  foo a = 123;
          |  foo c {"123"};
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.fullNameExact("foo.foo:void(T)").l) { case List(constructor) =>
        constructor.signature shouldBe "void(T)"
        constructor.methodReturn.typeFullName shouldBe "void"
        constructor.parameter.name.l shouldBe List("this", "param1")
        constructor.parameter.typeFullName.l shouldBe List("foo*", "T")
      }
      cpg.method.nameExact("use").local.nameExact("a", "c").typeFullName.l shouldBe List("foo", "foo")
      cpg.method.nameExact("use").call.nameExact(Operators.assignment).code.l should contain allElementsOf List(
        "a = foo.foo(123)",
        """c = foo.foo("123")"""
      )
    }

    "capture C++20 consteval and constinit declarations from the Rust parser backend" in {
      val cpg = code(
        """
          |consteval int sqr(int n) {
          |  return n * n;
          |}
          |
          |constexpr const char* f(bool p) {
          |  return p ? "constant initializer" : g();
          |}
          |
          |void use() {
          |  constexpr int r = sqr(100);
          |  constinit const char *c = f(true);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("sqr").signature.l shouldBe List("int(int)")
      cpg.method.nameExact("f").signature.l shouldBe List("char*(bool)")
      inside(cpg.method.nameExact("use").local.nameExact("r", "c").l) { case List(rLocal, cLocal) =>
        rLocal.typeFullName shouldBe "int"
        rLocal.code shouldBe "constexpr int r"
        cLocal.typeFullName shouldBe "char*"
        cLocal.code shouldBe "const char *c"
      }
      cpg.method.nameExact("use").call.nameExact("sqr").code.l shouldBe List("sqr(100)")
      cpg.method.nameExact("use").call.nameExact("f").code.l shouldBe List("f(true)")
    }

    "infer C++17 class template argument deduction braced locals from the Rust parser backend" in {
      val cpg = code(
        """
          |template <typename T>
          |struct container {
          |  container(T t) {}
          |  template <typename Iter>
          |  container(Iter beg, Iter end);
          |};
          |
          |template <typename Iter>
          |container(Iter b, Iter e) -> container<typename std::iterator_traits<Iter>::value_type>;
          |
          |void use() {
          |  std::mutex mtx;
          |  auto lck = std::lock_guard{ mtx };
          |  auto p = new std::pair{ 1.0, 2.0 };
          |  container a{ 7 };
          |  std::vector<double> values{ 1.0, 2.0, 3.0 };
          |  auto b = container{ values.begin(), values.end() };
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").local.nameExact("lck").typeFullName.l shouldBe List("std.lock_guard")
      cpg.method.nameExact("use").local.nameExact("p").typeFullName.l shouldBe List("std.pair*")
      cpg.method.nameExact("use").local.nameExact("a").typeFullName.l shouldBe List("container")
      cpg.method.nameExact("use").local.nameExact("b").typeFullName.l shouldBe List("container")

      cpg.method.nameExact("use").call.codeExact("std::lock_guard{ mtx }").typeFullName.l shouldBe List(
        "std.lock_guard"
      )
      cpg.method.nameExact("use").call.codeExact("container{ values.begin(), values.end() }").typeFullName.l shouldBe
        List("container")
    }

    "infer C++ auto local types from initializer expressions in the Rust parser backend" in {
      val cpg = code(
        """
          |int infer(int x, int *ptr) {
          |  auto literal = 1;
          |  auto copied = x;
          |  auto casted = static_cast<int>(x);
          |  auto pointer = ptr;
          |  int values[2];
          |  auto indexed = values[0];
          |  return literal + copied + casted + indexed;
          |}
          |int inferRefs(int x, int *ptr) {
          |  auto &ref = x;
          |  auto &&rref = static_cast<int>(x);
          |  auto *copiedPtr = ptr;
          |  auto *addressedPtr = &x;
          |  return ref + rref + *copiedPtr + *addressedPtr;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("infer").local.nameExact("literal").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("infer").local.nameExact("copied").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("infer").local.nameExact("casted").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("infer").local.nameExact("pointer").typeFullName.l shouldBe List("int*")
      cpg.method.nameExact("infer").local.nameExact("indexed").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("inferRefs").local.nameExact("ref").typeFullName.l shouldBe List("int&")
      cpg.method.nameExact("inferRefs").local.nameExact("rref").typeFullName.l shouldBe List("int&&")
      cpg.method.nameExact("inferRefs").local.nameExact("copiedPtr").typeFullName.l shouldBe List("int*")
      cpg.method.nameExact("inferRefs").local.nameExact("addressedPtr").typeFullName.l shouldBe List("int*")
      cpg.method.nameExact("inferRefs").call.nameExact(Operators.addressOf).code.l shouldBe List("&x")
      cpg.method.nameExact("inferRefs").call.nameExact(Operators.assignment).code.l should contain allElementsOf List(
        "&ref = x",
        "&rref = static_cast<int>(x)",
        "*copiedPtr = ptr",
        "*addressedPtr = &x"
      )
    }

    "infer C++17 braced auto initializer-list local types from the Rust parser backend" in {
      val cpg = code(
        """
          |void use() {
          |  auto x1 = {1, 2, 3};
          |  auto x2 {3};
          |  auto x3 {3.0};
          |  auto x4 = 3.0f;
          |  auto x5 = 3.0L;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").local.nameExact("x1").typeFullName.l shouldBe List("std.initializer_list<int>")
      cpg.method.nameExact("use").local.nameExact("x2").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("use").local.nameExact("x3").typeFullName.l shouldBe List("double")
      cpg.method.nameExact("use").local.nameExact("x4").typeFullName.l shouldBe List("float")
      cpg.method.nameExact("use").local.nameExact("x5").typeFullName.l shouldBe List("long double")
      cpg.method.nameExact("use").call.nameExact(Operators.arrayInitializer).codeExact("{1, 2, 3}").typeFullName.l shouldBe
        List("std.initializer_list<int>")
      cpg.literal.codeExact("3.0").typeFullName.l shouldBe List("double")
      cpg.literal.codeExact("3.0f").typeFullName.l shouldBe List("float")
      cpg.literal.codeExact("3.0L").typeFullName.l shouldBe List("long double")
    }

    "capture C++ structured bindings from the Rust parser backend" in {
      val cpg = code(
        """
          |struct Pair {
          |  int first;
          |  int second;
          |};
          |Pair make();
          |int use() {
          |  auto [first, second] = make();
          |  return first + second;
          |}
          |int useArray() {
          |  int values[2];
          |  auto [left, right] = values;
          |  return left + right;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      val temp = cpg.method.nameExact("use").local.filter(_.name.startsWith("<tmp>")).head
      temp.typeFullName shouldBe "Pair"
      cpg.method.nameExact("use").local.nameExact("first").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("use").local.nameExact("second").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("use").call.nameExact(Operators.assignment).code.l should contain theSameElementsAs List(
        s"${temp.name} = make()",
        s"first = ${temp.name}.first",
        s"second = ${temp.name}.second"
      )
      cpg.method.nameExact("use").call.nameExact(Operators.fieldAccess).code.l should contain theSameElementsAs List(
        s"${temp.name}.first",
        s"${temp.name}.second"
      )
      cpg.identifier.nameExact("first").refsTo.dedup.l shouldBe List(cpg.method.nameExact("use").local.nameExact("first").head)
      cpg.identifier.nameExact("second").refsTo.dedup.l shouldBe List(
        cpg.method.nameExact("use").local.nameExact("second").head
      )

      val arrayTemp = cpg.method.nameExact("useArray").local.filter(_.name.startsWith("<tmp>")).head
      arrayTemp.typeFullName shouldBe "int[]"
      cpg.method.nameExact("useArray").local.nameExact("left").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("useArray").local.nameExact("right").typeFullName.l shouldBe List("int")
      cpg.method.nameExact("useArray").call.nameExact(Operators.assignment).code.l should contain theSameElementsAs List(
        s"${arrayTemp.name} = values",
        s"left = ${arrayTemp.name}[0]",
        s"right = ${arrayTemp.name}[1]"
      )
      cpg.method.nameExact("useArray").call.nameExact(Operators.indirectIndexAccess).code.l should contain theSameElementsAs List(
        s"${arrayTemp.name}[0]",
        s"${arrayTemp.name}[1]"
      )
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
      val cpg = code(
        """
          |typedef const char * foo;
          |typedef foo * bar;
          |using baz = bar;
          |using qux = const char *;
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.typeDecl.nameExact("foo").aliasTypeFullName.l shouldBe List("char*")
      cpg.typeDecl.nameExact("bar").aliasTypeFullName.l shouldBe List("char**")
      cpg.typeDecl.nameExact("baz").aliasTypeFullName.l shouldBe List("char**")
      cpg.typeDecl.nameExact("qux").aliasTypeFullName.l shouldBe List("char*")
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

    "capture C++ aggregate designated initializer field assignments from the Rust parser backend" in {
      val cpg = code(
        """
          |struct A {
          |  int x;
          |  int y;
          |  int z;
          |};
          |void foo() {
          |  A a {.x = 1, .z = 2};
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("foo").local.nameExact("a").typeFullName.l shouldBe List("A")
      cpg.method.nameExact("foo").call.nameExact(Operators.assignment).code.l should contain allElementsOf List(
        "a.x = 1",
        "a.z = 2"
      )
      cpg.method.nameExact("foo").call.nameExact(Operators.fieldAccess).code.l should contain allElementsOf List(
        "a.x",
        "a.z"
      )
      cpg.identifier.nameExact("a").refsTo.l should contain(cpg.method.nameExact("foo").local.nameExact("a").head)
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
