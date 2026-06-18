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
          |}
          |int use() {
          |  int value = 1;
          |  Core::Derived derived;
          |  return derived.pick(value) + derived.callOwn(value);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.fullNameExact("Core.Base.call:int(int&)").call.codeExact("pick(value)").methodFullName.l shouldBe List(
        "Core.Base.pick:int(int&)"
      )
      cpg.method.fullNameExact("Core.Derived.callOwn:int(int&)").call.codeExact("pick(value)").methodFullName.l shouldBe
        List("Core.Derived.pick:int(int)")
      inside(cpg.method.nameExact("use").call.nameExact("pick").codeExact("derived.pick(value)").l) {
        case List(pickCall) =>
          pickCall.methodFullName shouldBe "Core.Derived.pick:int(int)"
          pickCall.dispatchType shouldBe DispatchTypes.STATIC_DISPATCH
          pickCall.receiver.code.l shouldBe Nil
      }
    }
  }
}
