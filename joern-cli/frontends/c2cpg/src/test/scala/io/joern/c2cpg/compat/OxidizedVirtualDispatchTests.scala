package io.joern.c2cpg.compat

import io.joern.c2cpg.Config
import io.joern.c2cpg.parser.ParserBackend
import io.joern.c2cpg.testfixtures.C2CpgSuite
import io.shiftleft.codepropertygraph.generated.{DispatchTypes, ModifierTypes}
import io.shiftleft.semanticcpg.language.*

class OxidizedVirtualDispatchTests extends C2CpgSuite {

  "The oxidized C++ virtual dispatch model" should {

    "propagate virtual dispatch through implicit overrides" in {
      val cpg = code(
        """
          |namespace Core {
          |class Base {
          |public:
          |  virtual int render(int scale) { return scale; }
          |};
          |class Mid : public Base {
          |public:
          |  int render(int scale) { return scale + 1; }
          |};
          |class Leaf : public Mid {
          |public:
          |  int render(int scale) { return scale + 2; }
          |};
          |class RefBase {
          |public:
          |  virtual int touch(int& value) { return value; }
          |};
          |class RefDerived : public RefBase {
          |public:
          |  int touch(int value) { return value + 1; }
          |};
          |}
          |int use() {
          |  Core::Mid mid;
          |  Core::Leaf leaf;
          |  Core::Base *basePtr = &leaf;
          |  return mid.render(1) + leaf.render(2) + basePtr->render(3);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.typeDecl.fullNameExact("Core.Mid").inheritsFromTypeFullName.l shouldBe List("Core.Base")
      cpg.typeDecl.fullNameExact("Core.Leaf").inheritsFromTypeFullName.l shouldBe List("Core.Mid")
      cpg.method.fullNameExact("Core.Base.render:int(int)").modifier.modifierType.l shouldBe List(ModifierTypes.VIRTUAL)
      cpg.method.fullNameExact("Core.Mid.render:int(int)").modifier.modifierType.l shouldBe List(ModifierTypes.VIRTUAL)
      cpg.method.fullNameExact("Core.Leaf.render:int(int)").modifier.modifierType.l shouldBe List(ModifierTypes.VIRTUAL)
      cpg.method.fullNameExact("Core.RefBase.touch:int(int&)").modifier.modifierType.l shouldBe List(
        ModifierTypes.VIRTUAL
      )
      cpg.method.fullNameExact("Core.RefDerived.touch:int(int)").modifier.modifierType.l shouldBe Nil

      inside(cpg.method.nameExact("use").call.nameExact("render").codeExact("mid.render(1)").l) {
        case List(renderCall) =>
          renderCall.methodFullName shouldBe "Core.Mid.render:int(int)"
          renderCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
          renderCall.receiver.code.l shouldBe List("mid")
      }
      inside(cpg.method.nameExact("use").call.nameExact("render").codeExact("leaf.render(2)").l) {
        case List(renderCall) =>
          renderCall.methodFullName shouldBe "Core.Leaf.render:int(int)"
          renderCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
          renderCall.receiver.code.l shouldBe List("leaf")
      }
      inside(cpg.method.nameExact("use").call.nameExact("render").codeExact("basePtr->render(3)").l) {
        case List(renderCall) =>
          renderCall.methodFullName shouldBe "Core.Base.render:int(int)"
          renderCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
          renderCall.receiver.code.l shouldBe List("basePtr")
      }
    }

    "respect derived member overload hiding before overload scoring" in {
      val cpg = code(
        """
          |namespace Core {
          |class Base {
          |public:
          |  int pick(int& value) { return value; }
          |  int call(int& value) { return pick(value); }
          |};
          |class Derived : public Base {
          |public:
          |  int pick(int value) { return value + 1; }
          |  int callOwn(int& value) { return pick(value); }
          |};
          |class Reintroduced : public Base {
          |public:
          |  using Base::pick;
          |  int pick(int value) { return value + 2; }
          |  int callOwn(int& value) { return pick(value); }
          |};
          |}
          |int use() {
          |  int value = 1;
          |  Core::Derived derived;
          |  Core::Reintroduced reintroduced;
          |  return derived.pick(value) + derived.callOwn(value) + reintroduced.pick(value) + reintroduced.callOwn(value);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.fullNameExact("Core.Base.call:int(int&)").call.codeExact("pick(value)").methodFullName.l shouldBe List(
        "Core.Base.pick:int(int&)"
      )
      cpg.method.fullNameExact("Core.Derived.callOwn:int(int&)").call.codeExact("pick(value)").methodFullName.l shouldBe
        List("Core.Derived.pick:int(int)")
      cpg.method
        .fullNameExact("Core.Reintroduced.callOwn:int(int&)")
        .call
        .codeExact("pick(value)")
        .methodFullName
        .l shouldBe
        List("Core.Base.pick:int(int&)")
      inside(cpg.method.nameExact("use").call.nameExact("pick").codeExact("derived.pick(value)").l) {
        case List(pickCall) =>
          pickCall.methodFullName shouldBe "Core.Derived.pick:int(int)"
          pickCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          pickCall.receiver.code.l shouldBe Nil
      }
      inside(cpg.method.nameExact("use").call.nameExact("pick").codeExact("reintroduced.pick(value)").l) {
        case List(pickCall) =>
          pickCall.methodFullName shouldBe "Core.Base.pick:int(int&)"
          pickCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          pickCall.receiver.code.l shouldBe Nil
      }
    }

    "prefer const member overloads for const this" in {
      val cpg = code(
        """
          |namespace Core {
          |class Meter {
          |public:
          |  int value() { return 1; }
          |  int value() const { return 2; }
          |  int viaMutable() { return value() + this->value(); }
          |  int viaConst() const { return value() + this->value(); }
          |};
          |}
          |int use() {
          |  Core::Meter meter;
          |  return meter.viaMutable() + meter.viaConst();
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.fullNameExact("Core.Meter.viaMutable:int()").call.nameExact("value").methodFullName.l shouldBe
        List("Core.Meter.value:int()", "Core.Meter.value:int()")
      cpg.method.fullNameExact("Core.Meter.viaConst:int()<const>").parameter.nameExact("this").typeFullName.l shouldBe
        List("const Core.Meter*")
      cpg.method.fullNameExact("Core.Meter.viaConst:int()<const>").call.nameExact("value").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>", "Core.Meter.value:int()<const>")
    }

    "force static dispatch for explicit base-qualified member calls" in {
      val cpg = code(
        """
          |namespace Core {
          |class Base {
          |public:
          |  virtual int render(int scale) { return scale; }
          |};
          |class Derived : public Base {
          |public:
          |  int render(int scale) { return Base::render(scale) + this->Base::render(scale); }
          |};
          |}
          |int use() {
          |  Core::Derived derived;
          |  return derived.render(1) + derived.Base::render(2);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      inside(cpg.method.fullNameExact("Core.Derived.render:int(int)").call.codeExact("Base::render(scale)").l) {
        case List(renderCall) =>
          renderCall.name shouldBe "render"
          renderCall.methodFullName shouldBe "Core.Base.render:int(int)"
          renderCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          renderCall.receiver.code.l shouldBe Nil
      }
      inside(cpg.method.fullNameExact("Core.Derived.render:int(int)").call.codeExact("this->Base::render(scale)").l) {
        case List(renderCall) =>
          renderCall.name shouldBe "render"
          renderCall.methodFullName shouldBe "Core.Base.render:int(int)"
          renderCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          renderCall.receiver.code.l shouldBe Nil
      }
      inside(cpg.method.nameExact("use").call.codeExact("derived.render(1)").l) { case List(renderCall) =>
        renderCall.methodFullName shouldBe "Core.Derived.render:int(int)"
        renderCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
        renderCall.receiver.code.l shouldBe List("derived")
      }
      inside(cpg.method.nameExact("use").call.codeExact("derived.Base::render(2)").l) { case List(renderCall) =>
        renderCall.name shouldBe "render"
        renderCall.methodFullName shouldBe "Core.Base.render:int(int)"
        renderCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
        renderCall.receiver.code.l shouldBe Nil
      }
    }

    "dispatch virtual destructors for single-object delete" in {
      val cpg = code(
        """
          |namespace Core {
          |class Base {
          |public:
          |  virtual ~Base();
          |};
          |class Derived : public Base {
          |public:
          |  ~Derived();
          |};
          |}
          |void destroy(Core::Base *ptr) {
          |  delete ptr;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.fullNameExact("Core.Base.~Base:void()").modifier.modifierType.l shouldBe List(ModifierTypes.VIRTUAL)
      inside(cpg.method.nameExact("destroy").call.nameExact("~Base").codeExact("ptr->~Base()").l) {
        case List(destructorCall) =>
          destructorCall.methodFullName shouldBe "Core.Base.~Base:void()"
          destructorCall.dispatchType shouldBe DispatchTypes.DYNAMIC_DISPATCH
          destructorCall.receiver.code.l shouldBe List("ptr")
      }
    }
  }
}
