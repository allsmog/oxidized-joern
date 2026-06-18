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

    "rank overloads by inheritance distance and reference binding" in {
      val cpg = code(
        """
          |#define CORE_NULL 0
          |namespace Core {
          |class Base {};
          |class Mid : public Base {};
          |class Leaf : public Mid {};
          |Leaf makeLeaf();
          |int pick(Base& value) { return 1; }
          |short pick(Mid& value) { return 2; }
          |long pick(const Mid& value) { return 3; }
          |char pick(Leaf&& value) { return 4; }
          |int pickPtr(Base* value) { return 1; }
          |short pickPtr(Mid* value) { return 2; }
          |long pickPtr(const Mid* value) { return 3; }
          |int pickConstIntPtr(const int* value) { return 1; }
          |long pickConstIntPtr(long value) { return 2; }
          |int pickVoidPtr(const void* value) { return 1; }
          |long pickVoidPtr(long value) { return 2; }
          |int pickArray(int* value) { return 1; }
          |long pickArray(long value) { return 2; }
          |int pickConstArray(const int* value) { return 1; }
          |long pickConstArray(long value) { return 2; }
          |int pickArrayVoid(const void* value) { return 1; }
          |long pickArrayVoid(long value) { return 2; }
          |int pickPointerBool(bool value) { return 1; }
          |long pickPointerBool(long value) { return 2; }
          |int pickArrayBool(bool value) { return 1; }
          |long pickArrayBool(long value) { return 2; }
          |int pickZeroNull(Mid* value) { return 1; }
          |int pickZeroNullAmbiguous(Mid* value) { return 1; }
          |long pickZeroNullAmbiguous(long value) { return 2; }
          |int choose(int value, int scale = 1) { return value + scale; }
          |long choose(long value) { return value; }
          |template <typename T>
          |T choose(T value) { return value; }
          |int preferDefault(Base& value) { return 1; }
          |short preferDefault(Mid& value, int scale = 1) { return 2; }
          |int rankIntegral(int value) { return 1; }
          |long rankIntegral(long value) { return 2; }
          |double rankFloat(double value) { return 1; }
          |long double rankFloat(long double value) { return 2; }
          |int pickSignedLiteral(int value) { return 1; }
          |long pickSignedLiteral(long value) { return 2; }
          |int pickUnsignedLiteral(int value) { return 1; }
          |short pickUnsignedLiteral(unsigned int value) { return 2; }
          |int pickLongLongLiteral(long value) { return 1; }
          |short pickLongLongLiteral(long long value) { return 2; }
          |int pickUnsignedLongLiteral(unsigned int value) { return 1; }
          |short pickUnsignedLongLiteral(unsigned long value) { return 2; }
          |int pickUnsignedLongLongLiteral(unsigned int value) { return 1; }
          |short pickUnsignedLongLongLiteral(unsigned long long value) { return 2; }
          |int pickNull(Mid* value) { return 1; }
          |long pickNull(long value) { return 2; }
          |class TargetBase {};
          |class TargetMid : public TargetBase {};
          |class Source {
          |public:
          |  operator TargetMid&();
          |};
          |class ConstSource {
          |public:
          |  operator const TargetMid&() const;
          |};
          |class ValueSource {
          |public:
          |  operator TargetMid();
          |};
          |int pickConverted(TargetBase& value) { return 1; }
          |short pickConverted(TargetMid& value) { return 2; }
          |long pickConverted(const TargetBase& value) { return 3; }
          |char pickConverted(TargetMid&& value) { return 4; }
          |class Chooser {
          |public:
          |  int select(Base& value) { return 1; }
          |  short select(Mid& value) { return 2; }
          |  long select(const Mid& value) { return 3; }
          |  char select(Leaf&& value) { return 4; }
          |  int selectPtr(Base* value) { return 1; }
          |  short selectPtr(Mid* value) { return 2; }
          |  long selectPtr(const Mid* value) { return 3; }
          |};
          |class Box {
          |public:
          |  Leaf operator[](int index) { return makeLeaf(); }
          |  Leaf& operator()();
          |};
          |}
          |long use(
          |  Core::Leaf& leaf,
          |  const Core::Leaf& constLeaf,
          |  Core::Leaf* leafPtr,
          |  const Core::Leaf* constLeafPtr,
          |  int* intPtr,
          |  long wide,
          |  Core::Mid& mid,
          |  Core::Chooser& chooser,
          |  Core::Box& box,
          |  Core::Source& source,
          |  const Core::ConstSource& constSource,
          |  Core::ValueSource& valueSource
          |) {
          |  int values[2];
          |  const int constValues[2];
          |  return Core::pick(leaf) +
          |    Core::pick(constLeaf) +
          |    Core::pick(Core::makeLeaf()) +
          |    Core::pick(mid) +
          |    Core::pickPtr(leafPtr) +
          |    Core::pickPtr(constLeafPtr) +
          |    Core::pickConstIntPtr(intPtr) +
          |    Core::pickVoidPtr(intPtr) +
          |    Core::pickArray(values) +
          |    Core::pickConstArray(constValues) +
          |    Core::pickArrayVoid(values) +
          |    Core::pickPointerBool(intPtr) +
          |    Core::pickArrayBool(values) +
          |    Core::pickZeroNull(0) +
          |    Core::pickZeroNull(0L) +
          |    Core::pickZeroNull(0x0) +
          |    Core::pickZeroNull(0b0) +
          |    Core::pickZeroNull(0B0u) +
          |    Core::pickZeroNull(0'0) +
          |    Core::pickZeroNull(0x0'0L) +
          |    Core::pickZeroNull((0)) +
          |    Core::pickZeroNull(CORE_NULL) +
          |    Core::pickZeroNull(+0) +
          |    Core::pickZeroNull(-0) +
          |    Core::pickZeroNullAmbiguous(0) +
          |    Core::pickZeroNullAmbiguous(CORE_NULL) +
          |    Core::choose(leaf) +
          |    Core::choose(1) +
          |    Core::choose(1, 2) +
          |    Core::choose(wide) +
          |    Core::preferDefault(leaf) +
          |    Core::rankIntegral('x') +
          |    Core::rankIntegral(true) +
          |    Core::rankFloat(1.0f) +
          |    Core::pickSignedLiteral(1L) +
          |    Core::pickUnsignedLiteral(1u) +
          |    Core::pickLongLongLiteral(1LL) +
          |    Core::pickUnsignedLongLiteral(1UL) +
          |    Core::pickUnsignedLongLongLiteral(0b1ULL) +
          |    Core::pickNull(nullptr) +
          |    Core::pickConverted(source) +
          |    Core::pickConverted(constSource) +
          |    Core::pickConverted(valueSource) +
          |    chooser.select(leaf) +
          |    chooser.select(constLeaf) +
          |    chooser.select(Core::makeLeaf()) +
          |    chooser.selectPtr(leafPtr) +
          |    chooser.selectPtr(constLeafPtr) +
          |    Core::pick(box[0]) +
          |    Core::pick(box());
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::pick(leaf)").methodFullName.l shouldBe
        List("Core.pick:short(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(constLeaf)").methodFullName.l shouldBe
        List("Core.pick:long(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::makeLeaf())").methodFullName.l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(mid)").methodFullName.l shouldBe
        List("Core.pick:short(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pickPtr(leafPtr)").methodFullName.l shouldBe
        List("Core.pickPtr:short(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickPtr(constLeafPtr)").methodFullName.l shouldBe
        List("Core.pickPtr:long(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickConstIntPtr(intPtr)").methodFullName.l shouldBe
        List("Core.pickConstIntPtr:int(int*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickVoidPtr(intPtr)").methodFullName.l shouldBe
        List("Core.pickVoidPtr:int(void*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickArray(values)").methodFullName.l shouldBe
        List("Core.pickArray:int(int*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickConstArray(constValues)").methodFullName.l shouldBe
        List("Core.pickConstArray:int(int*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickArrayVoid(values)").methodFullName.l shouldBe
        List("Core.pickArrayVoid:int(void*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickPointerBool(intPtr)").methodFullName.l shouldBe
        List("Core.pickPointerBool:int(bool)")
      cpg.method.nameExact("use").call.codeExact("Core::pickArrayBool(values)").methodFullName.l shouldBe
        List("Core.pickArrayBool:int(bool)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(0)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(0L)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(0x0)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(0b0)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(0B0u)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(0'0)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(0x0'0L)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull((0))").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(CORE_NULL)").methodFullName.l shouldBe
        List("Core.pickZeroNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(+0)").methodFullName.l shouldBe
        List("Core.pickZeroNull")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNull(-0)").methodFullName.l shouldBe
        List("Core.pickZeroNull")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNullAmbiguous(0)").methodFullName.l shouldBe
        List("Core.pickZeroNullAmbiguous")
      cpg.method.nameExact("use").call.codeExact("Core::pickZeroNullAmbiguous(CORE_NULL)").methodFullName.l shouldBe
        List("Core.pickZeroNullAmbiguous")
      cpg.method.nameExact("use").call.codeExact("Core::choose(leaf)").methodFullName.l shouldBe
        List("Core.choose:T(T)")
      cpg.method.nameExact("use").call.codeExact("Core::choose(1)").methodFullName.l shouldBe
        List("Core.choose:int(int,int)")
      cpg.method.nameExact("use").call.codeExact("Core::choose(1, 2)").methodFullName.l shouldBe
        List("Core.choose:int(int,int)")
      cpg.method.nameExact("use").call.codeExact("Core::choose(wide)").methodFullName.l shouldBe
        List("Core.choose:long(long)")
      cpg.method.nameExact("use").call.codeExact("Core::preferDefault(leaf)").methodFullName.l shouldBe
        List("Core.preferDefault:short(Mid&,int)")
      cpg.method.nameExact("use").call.codeExact("Core::rankIntegral('x')").methodFullName.l shouldBe
        List("Core.rankIntegral:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::rankIntegral(true)").methodFullName.l shouldBe
        List("Core.rankIntegral:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::rankFloat(1.0f)").methodFullName.l shouldBe
        List("Core.rankFloat:double(double)")
      cpg.method.nameExact("use").call.codeExact("Core::pickSignedLiteral(1L)").methodFullName.l shouldBe
        List("Core.pickSignedLiteral:long(long)")
      cpg.method.nameExact("use").call.codeExact("Core::pickUnsignedLiteral(1u)").methodFullName.l shouldBe
        List("Core.pickUnsignedLiteral:short(unsigned int)")
      cpg.method.nameExact("use").call.codeExact("Core::pickLongLongLiteral(1LL)").methodFullName.l shouldBe
        List("Core.pickLongLongLiteral:short(long long)")
      cpg.method.nameExact("use").call.codeExact("Core::pickUnsignedLongLiteral(1UL)").methodFullName.l shouldBe
        List("Core.pickUnsignedLongLiteral:short(unsigned long)")
      cpg.method.nameExact("use").call.codeExact("Core::pickUnsignedLongLongLiteral(0b1ULL)").methodFullName.l shouldBe
        List("Core.pickUnsignedLongLongLiteral:short(unsigned long long)")
      cpg.method.nameExact("use").call.codeExact("Core::pickNull(nullptr)").methodFullName.l shouldBe
        List("Core.pickNull:int(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickConverted(source)").methodFullName.l shouldBe
        List("Core.pickConverted:short(TargetMid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pickConverted(constSource)").methodFullName.l shouldBe
        List("Core.pickConverted:long(TargetBase&)")
      cpg.method.nameExact("use").call.codeExact("Core::pickConverted(valueSource)").methodFullName.l shouldBe
        List("Core.pickConverted:char(TargetMid&&)")
      cpg.method.nameExact("use").call.codeExact("chooser.select(leaf)").methodFullName.l shouldBe
        List("Core.Chooser.select:short(Mid&)")
      cpg.method.nameExact("use").call.codeExact("chooser.select(constLeaf)").methodFullName.l shouldBe
        List("Core.Chooser.select:long(Mid&)")
      cpg.method.nameExact("use").call.codeExact("chooser.select(Core::makeLeaf())").methodFullName.l shouldBe
        List("Core.Chooser.select:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("chooser.selectPtr(leafPtr)").methodFullName.l shouldBe
        List("Core.Chooser.selectPtr:short(Mid*)")
      cpg.method.nameExact("use").call.codeExact("chooser.selectPtr(constLeafPtr)").methodFullName.l shouldBe
        List("Core.Chooser.selectPtr:long(Mid*)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(box[0])").methodFullName.l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(box())").methodFullName.l shouldBe
        List("Core.pick:short(Mid&)")
    }

    "type floating literals for overload ranking" in {
      val cpg = code(
        """
          |namespace Core {
          |int pickFloat(float value) { return 1; }
          |short pickFloat(double value) { return 2; }
          |int pickDouble(float value) { return 1; }
          |short pickDouble(double value) { return 2; }
          |int pickLongDouble(double value) { return 1; }
          |short pickLongDouble(long double value) { return 2; }
          |int pickHexFloat(float value) { return 1; }
          |short pickHexFloat(double value) { return 2; }
          |int pickHexDouble(float value) { return 1; }
          |short pickHexDouble(double value) { return 2; }
          |int pickHexLongDouble(double value) { return 1; }
          |short pickHexLongDouble(long double value) { return 2; }
          |int pickSeparated(float value) { return 1; }
          |short pickSeparated(double value) { return 2; }
          |}
          |long use() {
          |  return Core::pickFloat(1.0f) +
          |    Core::pickDouble(1.0) +
          |    Core::pickLongDouble(1.0L) +
          |    Core::pickHexFloat(0x1.0p0f) +
          |    Core::pickHexDouble(0x1.0p0) +
          |    Core::pickHexLongDouble(0x1.0p0L) +
          |    Core::pickSeparated(1'000.0f);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::pickFloat(1.0f)").methodFullName.l shouldBe
        List("Core.pickFloat:int(float)")
      cpg.method.nameExact("use").call.codeExact("Core::pickDouble(1.0)").methodFullName.l shouldBe
        List("Core.pickDouble:short(double)")
      cpg.method.nameExact("use").call.codeExact("Core::pickLongDouble(1.0L)").methodFullName.l shouldBe
        List("Core.pickLongDouble:short(long double)")
      cpg.method.nameExact("use").call.codeExact("Core::pickHexFloat(0x1.0p0f)").methodFullName.l shouldBe
        List("Core.pickHexFloat:int(float)")
      cpg.method.nameExact("use").call.codeExact("Core::pickHexDouble(0x1.0p0)").methodFullName.l shouldBe
        List("Core.pickHexDouble:short(double)")
      cpg.method.nameExact("use").call.codeExact("Core::pickHexLongDouble(0x1.0p0L)").methodFullName.l shouldBe
        List("Core.pickHexLongDouble:short(long double)")
      cpg.method.nameExact("use").call.codeExact("Core::pickSeparated(1'000.0f)").methodFullName.l shouldBe
        List("Core.pickSeparated:int(float)")
    }

    "type character literals for overload ranking" in {
      val cpg = code(
        """
          |namespace Core {
          |int pickOrdinary(int value) { return 1; }
          |short pickOrdinary(char value) { return 2; }
          |int pickWide(int value) { return 1; }
          |short pickWide(wchar_t value) { return 2; }
          |int pickChar8(int value) { return 1; }
          |short pickChar8(char8_t value) { return 2; }
          |int pickChar16(int value) { return 1; }
          |short pickChar16(char16_t value) { return 2; }
          |int pickChar32(int value) { return 1; }
          |short pickChar32(char32_t value) { return 2; }
          |int pickMulti(int value) { return 1; }
          |short pickMulti(char value) { return 2; }
          |int promoteWide(int value) { return 1; }
          |long promoteWide(long value) { return 2; }
          |int promoteChar8(int value) { return 1; }
          |long promoteChar8(long value) { return 2; }
          |int promoteChar16(int value) { return 1; }
          |long promoteChar16(long value) { return 2; }
          |int convertChar32(int value) { return 1; }
          |long convertChar32(long value) { return 2; }
          |}
          |long use() {
          |  return Core::pickOrdinary('x') +
          |    Core::pickWide(L'x') +
          |    Core::pickChar8(u8'x') +
          |    Core::pickChar16(u'x') +
          |    Core::pickChar32(U'x') +
          |    Core::pickMulti('ab') +
          |    Core::promoteWide(L'x') +
          |    Core::promoteChar8(u8'x') +
          |    Core::promoteChar16(u'x') +
          |    Core::convertChar32(U'x');
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::pickOrdinary('x')").methodFullName.l shouldBe
        List("Core.pickOrdinary:short(char)")
      cpg.method.nameExact("use").call.codeExact("Core::pickWide(L'x')").methodFullName.l shouldBe
        List("Core.pickWide:short(wchar_t)")
      cpg.method.nameExact("use").call.codeExact("Core::pickChar8(u8'x')").methodFullName.l shouldBe
        List("Core.pickChar8:short(char8_t)")
      cpg.method.nameExact("use").call.codeExact("Core::pickChar16(u'x')").methodFullName.l shouldBe
        List("Core.pickChar16:short(char16_t)")
      cpg.method.nameExact("use").call.codeExact("Core::pickChar32(U'x')").methodFullName.l shouldBe
        List("Core.pickChar32:short(char32_t)")
      cpg.method.nameExact("use").call.codeExact("Core::pickMulti('ab')").methodFullName.l shouldBe
        List("Core.pickMulti:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::promoteWide(L'x')").methodFullName.l shouldBe
        List("Core.promoteWide:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::promoteChar8(u8'x')").methodFullName.l shouldBe
        List("Core.promoteChar8:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::promoteChar16(u'x')").methodFullName.l shouldBe
        List("Core.promoteChar16:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::convertChar32(U'x')").methodFullName.l shouldBe
        List("Core.convertChar32")
    }

    "reject non-const lvalue reference overloads for rvalues" in {
      val cpg = code(
        """
          |namespace Core {
          |class Item {};
          |Item makeItem();
          |long bind(const int& value) { return 1; }
          |short bind(int& value) { return 2; }
          |long bindItem(const Item& value) { return 1; }
          |short bindItem(Item& value) { return 2; }
          |}
          |long use(int& value) {
          |  return Core::bind(1) + Core::bind(value) + Core::bindItem(Core::makeItem());
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::bind(1)").methodFullName.l shouldBe
        List("Core.bind:long(int&)")
      cpg.method.nameExact("use").call.codeExact("Core::bind(value)").methodFullName.l shouldBe
        List("Core.bind:short(int&)")
      cpg.method.nameExact("use").call.codeExact("Core::bindItem(Core::makeItem())").methodFullName.l shouldBe
        List("Core.bindItem:long(Item&)")
    }

    "filter overloads with incompatible arguments before scoring" in {
      val cpg = code(
        """
          |namespace Core {
          |class TargetBase {};
          |class TargetMid : public TargetBase {};
          |class Source {
          |public:
          |  operator TargetMid&();
          |};
          |class Wrong {};
          |class Root {};
          |class N1 : public Root {};
          |class N2 : public N1 {};
          |class N3 : public N2 {};
          |class N4 : public N3 {};
          |class N5 : public N4 {};
          |class N6 : public N5 {};
          |class N7 : public N6 {};
          |class N8 : public N7 {};
          |class N9 : public N8 {};
          |class N10 : public N9 {};
          |class N11 : public N10 {};
          |class N12 : public N11 {};
          |int route(Wrong& source, N12& value) { return 1; }
          |long route(TargetBase& source, Root& value) { return 2; }
          |}
          |long use(Core::Source& source, Core::N12& value) {
          |  return Core::route(source, value);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::route(source, value)").methodFullName.l shouldBe
        List("Core.route:long(TargetBase&,Root&)")
    }

    "leave ambiguous overload calls unresolved" in {
      val cpg = code(
        """
          |namespace Core {
          |class Base {};
          |class Mid : public Base {};
          |class Leaf : public Mid {};
          |int mix(Base& left, Mid& right) { return 1; }
          |long mix(Mid& left, Base& right) { return 2; }
          |}
          |long use(Core::Leaf& leaf) {
          |  return Core::mix(leaf, leaf);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::mix(leaf, leaf)").methodFullName.l shouldBe
        List("Core.mix")
    }

    "leave ambiguous contextual conversion calls unresolved" in {
      val cpg = code(
        """
          |namespace Core {
          |class Base {};
          |class Left : public Base {};
          |class Right : public Base {};
          |class Source {
          |public:
          |  operator Left&();
          |  operator Right&();
          |};
          |int take(Base& value) { return 1; }
          |long take(long value) { return 2; }
          |}
          |long use(Core::Source& source) {
          |  return Core::take(source);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::take(source)").methodFullName.l shouldBe
        List("Core.take")
      cpg.method.nameExact("use").call.name("operator .*").l shouldBe Nil
    }

    "resolve constructor conversions during overload resolution" in {
      val cpg = code(
        """
          |namespace Core {
          |class Box {
          |public:
          |  Box(int value) {}
          |  Box(long value) {}
          |};
          |class Left {
          |public:
          |  Left(int value) {}
          |};
          |class Right {
          |public:
          |  Right(int value) {}
          |};
          |int onlyBox(Box value) { return 1; }
          |int onlyConstBoxRef(const Box& value) { return 1; }
          |int onlyBoxRef(Box& value) { return 1; }
          |int preferLong(Box value) { return 1; }
          |long preferLong(long value) { return 2; }
          |int ambiguous(Left value) { return 1; }
          |long ambiguous(Right value) { return 2; }
          |}
          |long use(int seed) {
          |  return Core::onlyBox(seed) +
          |    Core::onlyConstBoxRef(seed) +
          |    Core::onlyBoxRef(seed) +
          |    Core::preferLong(seed) +
          |    Core::ambiguous(seed);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::onlyBox(seed)").methodFullName.l shouldBe
        List("Core.onlyBox:int(Box)")
      cpg.method.nameExact("use").call.codeExact("Core::onlyBox(seed)").argument.code.l shouldBe
        List("Core.Box.Box(seed)")
      cpg.method.nameExact("use").call.codeExact("Core::onlyConstBoxRef(seed)").methodFullName.l shouldBe
        List("Core.onlyConstBoxRef:int(Box&)")
      cpg.method.nameExact("use").call.codeExact("Core::onlyConstBoxRef(seed)").argument.code.l shouldBe
        List("Core.Box.Box(seed)")
      cpg.method.nameExact("use").call.codeExact("Core::onlyBoxRef(seed)").methodFullName.l shouldBe
        List("Core.onlyBoxRef")
      cpg.method.nameExact("use").call.codeExact("Core::preferLong(seed)").methodFullName.l shouldBe
        List("Core.preferLong:long(long)")
      cpg.method.nameExact("use").call.codeExact("Core::ambiguous(seed)").methodFullName.l shouldBe
        List("Core.ambiguous")
    }

    "ignore explicit constructors for implicit overload conversions" in {
      val cpg = code(
        """
          |namespace Core {
          |class Hidden {
          |public:
          |  explicit Hidden(int value) {}
          |};
          |class Visible {
          |public:
          |  Visible(int value) {}
          |};
          |class ConditionalVisible {
          |public:
          |  explicit(false) ConditionalVisible(int value) {}
          |};
          |class ConditionalHidden {
          |public:
          |  explicit(true) ConditionalHidden(int value) {}
          |};
          |int onlyHidden(Hidden value) { return 1; }
          |int onlyConditionalVisible(ConditionalVisible value) { return 1; }
          |int onlyConditionalHidden(ConditionalHidden value) { return 1; }
          |long choose(Hidden value) { return 1; }
          |int choose(Visible value) { return 2; }
          |}
          |long use(int seed) {
          |  Core::Hidden direct(seed);
          |  return Core::onlyHidden(seed) +
          |    Core::onlyConditionalVisible(seed) +
          |    Core::onlyConditionalHidden(seed) +
          |    Core::choose(seed);
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core.Hidden.Hidden(seed)").methodFullName.l should contain(
        "Core.Hidden.Hidden:void(int)"
      )
      cpg.method.nameExact("use").call.codeExact("Core::onlyHidden(seed)").methodFullName.l shouldBe
        List("Core.onlyHidden")
      cpg.method.nameExact("use").call.codeExact("Core::onlyConditionalVisible(seed)").methodFullName.l shouldBe
        List("Core.onlyConditionalVisible:int(ConditionalVisible)")
      cpg.method.nameExact("use").call.codeExact("Core::onlyConditionalVisible(seed)").argument.code.l shouldBe
        List("Core.ConditionalVisible.ConditionalVisible(seed)")
      cpg.method.nameExact("use").call.codeExact("Core::onlyConditionalHidden(seed)").methodFullName.l shouldBe
        List("Core.onlyConditionalHidden")
      cpg.method.nameExact("use").call.codeExact("Core::choose(seed)").methodFullName.l shouldBe
        List("Core.choose:int(Visible)")
      cpg.method.nameExact("use").call.codeExact("Core::choose(seed)").argument.code.l shouldBe
        List("Core.Visible.Visible(seed)")
    }

    "ignore explicit constructors for copy initialization" in {
      val cpg = code(
        """
          |namespace Core {
          |class Hidden {
          |public:
          |  explicit Hidden(int value) {}
          |};
          |class Visible {
          |public:
          |  Visible(int value) {}
          |};
          |class ConditionalVisible {
          |public:
          |  explicit(false) ConditionalVisible(int value) {}
          |};
          |class ConditionalHidden {
          |public:
          |  explicit(true) ConditionalHidden(int value) {}
          |};
          |}
          |int use(int seed) {
          |  Core::Hidden direct(seed);
          |  Core::Hidden copyHidden = seed;
          |  Core::Visible copyVisible = seed;
          |  Core::ConditionalVisible copyConditionalVisible = seed;
          |  Core::ConditionalHidden copyConditionalHidden = seed;
          |  return seed;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("direct = Core.Hidden.Hidden(seed)").l should have size 1
      cpg.method.nameExact("use").call.codeExact("copyHidden = Core.Hidden.Hidden(seed)").l shouldBe Nil
      cpg.method.nameExact("use").call.codeExact("copyHidden = seed").l should have size 1
      cpg.method.nameExact("use").call.codeExact("copyVisible = Core.Visible.Visible(seed)").l should have size 1
      cpg.method
        .nameExact("use")
        .call
        .codeExact("copyConditionalVisible = Core.ConditionalVisible.ConditionalVisible(seed)")
        .l should have size 1
      cpg.method
        .nameExact("use")
        .call
        .codeExact("copyConditionalHidden = Core.ConditionalHidden.ConditionalHidden(seed)")
        .l shouldBe Nil
      cpg.method.nameExact("use").call.codeExact("copyConditionalHidden = seed").l should have size 1
    }

    "ignore explicit constructors for copy list initialization" in {
      val cpg = code(
        """
          |namespace Core {
          |class Hidden {
          |public:
          |  explicit Hidden(int value) {}
          |};
          |class Visible {
          |public:
          |  Visible(int value) {}
          |};
          |class ConditionalVisible {
          |public:
          |  explicit(false) ConditionalVisible(int value) {}
          |};
          |class ConditionalHidden {
          |public:
          |  explicit(true) ConditionalHidden(int value) {}
          |};
          |}
          |Core::Hidden globalDirectList{1};
          |Core::Hidden globalCopyListHidden = {1};
          |Core::Visible globalCopyListVisible = {1};
          |Core::ConditionalVisible globalCopyListConditionalVisible = {1};
          |Core::ConditionalHidden globalCopyListConditionalHidden = {1};
          |int use(int seed) {
          |  Core::Hidden directList{seed};
          |  Core::Hidden copyListHidden = {seed};
          |  Core::Visible copyListVisible = {seed};
          |  Core::ConditionalVisible copyListConditionalVisible = {seed};
          |  Core::ConditionalHidden copyListConditionalHidden = {seed};
          |  return seed;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.call.codeExact("globalDirectList = Core.Hidden.Hidden(1)").l should have size 1
      cpg.call.codeExact("globalCopyListHidden = Core.Hidden.Hidden(1)").l shouldBe Nil
      cpg.call.codeExact("globalCopyListVisible = Core.Visible.Visible(1)").l should have size 1
      cpg.call
        .codeExact("globalCopyListConditionalVisible = Core.ConditionalVisible.ConditionalVisible(1)")
        .l should have size 1
      cpg.call
        .codeExact("globalCopyListConditionalHidden = Core.ConditionalHidden.ConditionalHidden(1)")
        .l shouldBe Nil
      cpg.method.nameExact("use").call.codeExact("directList = Core.Hidden.Hidden(seed)").l should have size 1
      cpg.method.nameExact("use").call.codeExact("copyListHidden = Core.Hidden.Hidden(seed)").l shouldBe Nil
      cpg.method.nameExact("use").call.codeExact("copyListVisible = Core.Visible.Visible(seed)").l should have size 1
      cpg.method
        .nameExact("use")
        .call
        .codeExact("copyListConditionalVisible = Core.ConditionalVisible.ConditionalVisible(seed)")
        .l should have size 1
      cpg.method
        .nameExact("use")
        .call
        .codeExact("copyListConditionalHidden = Core.ConditionalHidden.ConditionalHidden(seed)")
        .l shouldBe Nil
    }

    "ignore explicit constructors for array element initialization" in {
      val cpg = code(
        """
          |namespace Core {
          |class Hidden {
          |public:
          |  explicit Hidden(int value) {}
          |};
          |class Visible {
          |public:
          |  Visible(int value) {}
          |};
          |class ConditionalVisible {
          |public:
          |  explicit(false) ConditionalVisible(int value) {}
          |};
          |class ConditionalHidden {
          |public:
          |  explicit(true) ConditionalHidden(int value) {}
          |};
          |class Owner {
          |  Hidden hidden[1];
          |  Visible visible[1];
          |  ConditionalVisible conditionalVisible[1];
          |  ConditionalHidden conditionalHidden[1];
          |public:
          |  Owner(int seed) : hidden{{seed}}, visible{{seed}}, conditionalVisible{{seed}},
          |    conditionalHidden{{seed}} {}
          |};
          |}
          |Core::Hidden globalDirectHidden[1]{{1}};
          |Core::Hidden globalCopyHidden[1] = {{1}};
          |Core::Hidden globalFlatHidden[1] = {1};
          |Core::Visible globalCopyVisible[1] = {{1}};
          |Core::Visible globalFlatVisible[1] = {1};
          |Core::ConditionalVisible globalCopyConditionalVisible[1] = {{1}};
          |Core::ConditionalHidden globalCopyConditionalHidden[1] = {{1}};
          |int use(int seed) {
          |  Core::Hidden localDirectHidden[1]{{seed}};
          |  Core::Hidden localCopyHidden[1] = {{seed}};
          |  Core::Hidden localFlatHidden[1] = {seed};
          |  Core::Visible localCopyVisible[1] = {{seed}};
          |  Core::Visible localFlatVisible[1] = {seed};
          |  Core::ConditionalVisible localCopyConditionalVisible[1] = {{seed}};
          |  Core::ConditionalHidden localCopyConditionalHidden[1] = {{seed}};
          |  Core::Owner owner(seed);
          |  return seed;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.call.codeExact("globalDirectHidden[0] = Core.Hidden.Hidden(1)").l shouldBe Nil
      cpg.call.codeExact("globalCopyHidden[0] = Core.Hidden.Hidden(1)").l shouldBe Nil
      cpg.call.codeExact("globalFlatHidden[0] = Core.Hidden.Hidden(1)").l shouldBe Nil
      cpg.call.codeExact("globalCopyVisible[0] = Core.Visible.Visible(1)").l should have size 1
      cpg.call.codeExact("globalFlatVisible[0] = Core.Visible.Visible(1)").l should have size 1
      cpg.call
        .codeExact("globalCopyConditionalVisible[0] = Core.ConditionalVisible.ConditionalVisible(1)")
        .l should have size 1
      cpg.call
        .codeExact("globalCopyConditionalHidden[0] = Core.ConditionalHidden.ConditionalHidden(1)")
        .l shouldBe Nil
      cpg.method.nameExact("use").call.codeExact("localDirectHidden[0] = Core.Hidden.Hidden(seed)").l shouldBe Nil
      cpg.method.nameExact("use").call.codeExact("localCopyHidden[0] = Core.Hidden.Hidden(seed)").l shouldBe Nil
      cpg.method.nameExact("use").call.codeExact("localFlatHidden[0] = Core.Hidden.Hidden(seed)").l shouldBe Nil
      cpg.method
        .nameExact("use")
        .call
        .codeExact("localCopyVisible[0] = Core.Visible.Visible(seed)")
        .l should have size 1
      cpg.method
        .nameExact("use")
        .call
        .codeExact("localFlatVisible[0] = Core.Visible.Visible(seed)")
        .l should have size 1
      cpg.method
        .nameExact("use")
        .call
        .codeExact("localCopyConditionalVisible[0] = Core.ConditionalVisible.ConditionalVisible(seed)")
        .l should have size 1
      cpg.method
        .nameExact("use")
        .call
        .codeExact("localCopyConditionalHidden[0] = Core.ConditionalHidden.ConditionalHidden(seed)")
        .l shouldBe Nil
      cpg.method
        .fullNameExact("Core.Owner.Owner:void(int)")
        .call
        .codeExact("this->hidden[0] = Core.Hidden.Hidden(seed)")
        .l shouldBe Nil
      cpg.method
        .fullNameExact("Core.Owner.Owner:void(int)")
        .call
        .codeExact("this->visible[0] = Core.Visible.Visible(seed)")
        .l should have size 1
      cpg.method
        .fullNameExact("Core.Owner.Owner:void(int)")
        .call
        .codeExact("this->conditionalVisible[0] = Core.ConditionalVisible.ConditionalVisible(seed)")
        .l should have size 1
      cpg.method
        .fullNameExact("Core.Owner.Owner:void(int)")
        .call
        .codeExact("this->conditionalHidden[0] = Core.ConditionalHidden.ConditionalHidden(seed)")
        .l shouldBe Nil
    }

    "ignore explicit conversion operators for implicit overload conversions" in {
      val cpg = code(
        """
          |namespace Core {
          |class ExplicitOnly {
          |public:
          |  explicit operator int() const { return 1; }
          |};
          |class Numeric {
          |public:
          |  explicit operator int() const { return 1; }
          |  operator long() const { return 2; }
          |};
          |class ConditionalVisibleNumeric {
          |public:
          |  explicit(false) operator int() const { return 3; }
          |};
          |class ConditionalHiddenNumeric {
          |public:
          |  explicit(true) operator int() const { return 4; }
          |};
          |class Flag {
          |public:
          |  explicit operator bool() const { return true; }
          |};
          |int onlyInt(int value) { return 1; }
          |int onlyConditionalInt(int value) { return 1; }
          |int onlyConditionalHiddenInt(int value) { return 1; }
          |int onlyFlagBool(bool value) { return 1; }
          |int choose(int value) { return 1; }
          |long choose(long value) { return 2; }
          |}
          |long use(Core::ExplicitOnly explicitOnly,
          |  Core::Numeric numeric,
          |  Core::ConditionalVisibleNumeric conditionalVisible,
          |  Core::ConditionalHiddenNumeric conditionalHidden,
          |  Core::Flag flag) {
          |  if (flag) {
          |    return Core::onlyInt(explicitOnly) +
          |      Core::onlyConditionalInt(conditionalVisible) +
          |      Core::onlyConditionalHiddenInt(conditionalHidden) +
          |      Core::choose(numeric) +
          |      Core::onlyFlagBool(flag);
          |  }
          |  return 0;
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").controlStructure.condition.ast.isCall.nameExact("operator bool").code.l shouldBe
        List("flag.operator bool()")
      cpg.method.nameExact("use").call.codeExact("Core::onlyInt(explicitOnly)").methodFullName.l shouldBe
        List("Core.onlyInt")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::onlyConditionalInt(conditionalVisible)")
        .methodFullName
        .l shouldBe
        List("Core.onlyConditionalInt:int(int)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::onlyConditionalInt(conditionalVisible)")
        .argument
        .code
        .l shouldBe
        List("conditionalVisible.operator int()")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::onlyConditionalHiddenInt(conditionalHidden)")
        .methodFullName
        .l shouldBe
        List("Core.onlyConditionalHiddenInt")
      cpg.method.nameExact("use").call.codeExact("Core::choose(numeric)").methodFullName.l shouldBe
        List("Core.choose:long(long)")
      cpg.method.nameExact("use").call.codeExact("Core::choose(numeric)").argument.code.l shouldBe
        List("numeric.operator long()")
      cpg.method.nameExact("use").call.codeExact("Core::onlyFlagBool(flag)").methodFullName.l shouldBe
        List("Core.onlyFlagBool")
    }

    "specialize function template returns during overload resolution" in {
      val cpg = code(
        """
          |namespace Core {
          |class Base {};
          |class Mid : public Base {};
          |class Leaf : public Mid {};
          |Leaf makeLeaf();
          |int pick(Base& value) { return 1; }
          |short pick(Mid& value) { return 2; }
          |long pick(const Mid& value) { return 3; }
          |char pick(Leaf&& value) { return 4; }
          |int pickNumber(int value) { return 1; }
          |long pickNumber(long value) { return 2; }
          |int pickBool(bool value) { return 1; }
          |long pickBool(int value) { return 2; }
          |int pickList(std::initializer_list<int> values) { return 1; }
          |long pickList(long value) { return 2; }
          |int pickConstLeafPtr(const Leaf* value) { return 1; }
          |long pickConstLeafPtr(long value) { return 2; }
          |int pickIntPtr(int* value) { return 1; }
          |long pickIntPtr(long value) { return 2; }
          |template <typename T>
          |class Holder {
          |public:
          |  T& operator[](int index);
          |  template <typename U>
          |  U memberExplicit();
          |};
          |template <typename T>
          |T id(T value) { return value; }
          |template <typename T>
          |auto idAuto(T value) { return value; }
          |template <typename T>
          |auto idAutoForward(T value) { return idAuto(value); }
          |template <typename T>
          |auto idAutoBranch(bool choose, T left, T right) {
          |  if (choose) {
          |    return left;
          |  } else {
          |    return right;
          |  }
          |}
          |template <typename T>
          |auto idAutoBranchLocal(bool choose, T left, T right) {
          |  if (choose) {
          |    auto tmp = left;
          |    return tmp;
          |  } else {
          |    auto tmp = right;
          |    return tmp;
          |  }
          |}
          |template <typename T>
          |auto add(T left, T right) { return left + right; }
          |template <typename T>
          |auto negate(T value) { return -value; }
          |template <typename T>
          |auto logicalNot(T value) { return !value; }
          |template <typename T>
          |auto assignValue(T& target, T value) { return target = value; }
          |template <typename T>
          |decltype(auto) assignRef(T& target, T value) { return (target = value); }
          |template <typename T>
          |auto list(T left, T right) {
          |  auto tmp = {left, right};
          |  return tmp;
          |}
          |template <typename T>
          |decltype(auto) idDecltype(T& value) { return value; }
          |template <typename T>
          |decltype(auto) idDecltypeParen(T value) { return (value); }
          |template <typename T>
          |decltype(auto) idDecltypeBranch(bool choose, T& left, T& right) {
          |  if (choose) {
          |    return left;
          |  } else {
          |    return right;
          |  }
          |}
          |template <typename T>
          |auto first(Holder<T>& holder) { return holder[0]; }
          |template <typename T>
          |decltype(auto) firstRef(Holder<T>& holder) { return holder[0]; }
          |template <typename T>
          |auto nestedFirst(Holder<Holder<T>>& holder) { return holder[0][0]; }
          |template <typename T>
          |T makeExplicit();
          |template <typename T>
          |T& ref(T& value) { return value; }
          |template <typename T>
          |const T& cref(const T& value) { return value; }
          |template <typename T>
          |const T* cptr(const T* value) { return value; }
          |template <typename T>
          |T* decayArray(T* value) { return value; }
          |}
          |long use(Core::Leaf& leaf, const Core::Leaf& constLeaf, Core::Holder<Core::Leaf>& holder) {
          |  Core::Holder<Core::Holder<Core::Leaf>> nestedHolder;
          |  int values[2];
          |  return Core::pick(Core::id(Core::makeLeaf())) +
          |    Core::pick(Core::idAuto(Core::makeLeaf())) +
          |    Core::pick(Core::idAutoForward(Core::makeLeaf())) +
          |    Core::pick(Core::idAutoBranch(true, Core::makeLeaf(), Core::makeLeaf())) +
          |    Core::pick(Core::idAutoBranchLocal(true, Core::makeLeaf(), Core::makeLeaf())) +
          |    Core::pickNumber(Core::add(1, 2)) +
          |    Core::pickNumber(Core::negate(1)) +
          |    Core::pickBool(Core::logicalNot(true)) +
          |    Core::pick(Core::assignValue(leaf, Core::makeLeaf())) +
          |    Core::pick(Core::assignRef(leaf, Core::makeLeaf())) +
          |    Core::pickList(Core::list(1, 2)) +
          |    Core::pick(Core::idDecltype(leaf)) +
          |    Core::pick(Core::idDecltypeParen(Core::makeLeaf())) +
          |    Core::pick(Core::idDecltypeBranch(true, leaf, leaf)) +
          |    Core::pick(Core::first(holder)) +
          |    Core::pick(Core::firstRef(holder)) +
          |    Core::pick(Core::nestedFirst(nestedHolder)) +
          |    Core::pick(holder.memberExplicit<Core::Leaf>()) +
          |    Core::pick(Core::makeExplicit<Core::Leaf>()) +
          |    Core::pick(Core::ref(leaf)) +
          |    Core::pick(Core::cref(constLeaf)) +
          |    Core::pickConstLeafPtr(Core::cptr(&leaf)) +
          |    Core::pickIntPtr(Core::decayArray(values));
          |}
          |""".stripMargin,
        "Test0.cpp"
      ).withConfig(Config(parserBackend = ParserBackend.Oxidized))

      cpg.method.nameExact("use").call.codeExact("Core::id(Core::makeLeaf())").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::idAuto(Core::makeLeaf())").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::idAutoForward(Core::makeLeaf())").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::idAutoBranch(true, Core::makeLeaf(), Core::makeLeaf())")
        .typeFullName
        .l shouldBe List("Core.Leaf")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::idAutoBranchLocal(true, Core::makeLeaf(), Core::makeLeaf())")
        .typeFullName
        .l shouldBe List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::add(1, 2)").typeFullName.l shouldBe
        List("int")
      cpg.method.nameExact("use").call.codeExact("Core::negate(1)").typeFullName.l shouldBe
        List("int")
      cpg.method.nameExact("use").call.codeExact("Core::logicalNot(true)").typeFullName.l shouldBe
        List("bool")
      cpg.method.nameExact("use").call.codeExact("Core::assignValue(leaf, Core::makeLeaf())").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::assignRef(leaf, Core::makeLeaf())").typeFullName.l shouldBe
        List("Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::list(1, 2)").typeFullName.l shouldBe
        List("std.initializer_list<int>")
      cpg.method.nameExact("use").call.codeExact("Core::idDecltype(leaf)").typeFullName.l shouldBe
        List("Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::idDecltypeParen(Core::makeLeaf())").typeFullName.l shouldBe
        List("Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::idDecltypeBranch(true, leaf, leaf)").typeFullName.l shouldBe
        List("Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::first(holder)").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::firstRef(holder)").typeFullName.l shouldBe
        List("Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::nestedFirst(nestedHolder)").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("holder.memberExplicit<Core::Leaf>()").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::makeExplicit<Core::Leaf>()").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::ref(leaf)").typeFullName.l shouldBe
        List("Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::cref(constLeaf)").typeFullName.l shouldBe
        List("const Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::cptr(&leaf)").typeFullName.l shouldBe
        List("const Core.Leaf*")
      cpg.method.nameExact("use").call.codeExact("Core::decayArray(values)").typeFullName.l shouldBe
        List("int*")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::id(Core::makeLeaf()))").methodFullName.l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::idAuto(Core::makeLeaf()))").methodFullName.l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::idAutoForward(Core::makeLeaf()))")
        .methodFullName
        .l shouldBe List("Core.pick:char(Leaf&&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::idAutoBranch(true, Core::makeLeaf(), Core::makeLeaf()))")
        .methodFullName
        .l shouldBe List("Core.pick:char(Leaf&&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::idAutoBranchLocal(true, Core::makeLeaf(), Core::makeLeaf()))")
        .methodFullName
        .l shouldBe List("Core.pick:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("Core::pickNumber(Core::add(1, 2))").methodFullName.l shouldBe
        List("Core.pickNumber:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::pickNumber(Core::negate(1))").methodFullName.l shouldBe
        List("Core.pickNumber:int(int)")
      cpg.method.nameExact("use").call.codeExact("Core::pickBool(Core::logicalNot(true))").methodFullName.l shouldBe
        List("Core.pickBool:int(bool)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::assignValue(leaf, Core::makeLeaf()))")
        .methodFullName
        .l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::assignRef(leaf, Core::makeLeaf()))")
        .methodFullName
        .l shouldBe
        List("Core.pick:short(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pickList(Core::list(1, 2))").methodFullName.l shouldBe
        List("Core.pickList:int(std::initializer_list<int>)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::idDecltype(leaf))").methodFullName.l shouldBe
        List("Core.pick:short(Mid&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::idDecltypeParen(Core::makeLeaf()))")
        .methodFullName
        .l shouldBe List("Core.pick:short(Mid&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::idDecltypeBranch(true, leaf, leaf))")
        .methodFullName
        .l shouldBe List("Core.pick:short(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::first(holder))").methodFullName.l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::firstRef(holder))").methodFullName.l shouldBe
        List("Core.pick:short(Mid&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::nestedFirst(nestedHolder))")
        .methodFullName
        .l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(holder.memberExplicit<Core::Leaf>())")
        .methodFullName
        .l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method
        .nameExact("use")
        .call
        .codeExact("Core::pick(Core::makeExplicit<Core::Leaf>())")
        .methodFullName
        .l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::ref(leaf))").methodFullName.l shouldBe
        List("Core.pick:short(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::cref(constLeaf))").methodFullName.l shouldBe
        List("Core.pick:long(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pickConstLeafPtr(Core::cptr(&leaf))").methodFullName.l shouldBe
        List("Core.pickConstLeafPtr:int(Leaf*)")
      cpg.method.nameExact("use").call.codeExact("Core::pickIntPtr(Core::decayArray(values))").methodFullName.l shouldBe
        List("Core.pickIntPtr:int(int*)")
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
          |using MeterAlias = Meter;
          |const Meter globalMeter;
          |const MeterAlias aliasGlobal;
          |const MeterAlias& borrowAlias(Meter& meter) { return meter; }
          |auto borrowTrailing(Meter& meter) -> const MeterAlias& { return meter; }
          |class Holder {
          |public:
          |  const Meter field;
          |  const MeterAlias aliasField;
          |  Meter mutableField;
          |  int readField() { return this->field.value() + this->aliasField.value() + this->mutableField.value(); }
          |};
          |class OperatorSource {
          |public:
          |  const MeterAlias& operator+(Meter& meter) { return meter; }
          |  const MeterAlias& operator[](int) { return aliasGlobal; }
          |  const MeterAlias& operator()() { return aliasGlobal; }
          |  const MeterAlias& operator=(Meter& meter) { return meter; }
          |};
          |const MeterAlias& operator-(OperatorSource& source, Meter& meter) {
          |  return meter;
          |}
          |class Convertible {
          |public:
          |  operator const MeterAlias&() { return aliasGlobal; }
          |};
          |}
          |typedef Core::Meter GlobalMeterAlias;
          |const GlobalMeterAlias& borrowTypedef(Core::Meter& meter) {
          |  return meter;
          |}
          |int readConst(const Core::Meter& meter) {
          |  return meter.value();
          |}
          |int readContextualConversion(Core::Convertible& source) {
          |  return readConst(source);
          |}
          |int readAlias(Core::Meter& meter) {
          |  const Core::Meter& alias = meter;
          |  return alias.value();
          |}
          |int readTypedef(const GlobalMeterAlias& meter) {
          |  return meter.value();
          |}
          |int readTypedefAlias(Core::Meter& meter) {
          |  const GlobalMeterAlias& alias = meter;
          |  return alias.value();
          |}
          |int readNamespaceAlias(const Core::MeterAlias& meter) {
          |  return meter.value();
          |}
          |int readReturn(Core::Meter& meter) {
          |  return Core::borrowAlias(meter).value();
          |}
          |int readTrailingReturn(Core::Meter& meter) {
          |  return Core::borrowTrailing(meter).value();
          |}
          |int readTypedefReturn(Core::Meter& meter) {
          |  return borrowTypedef(meter).value();
          |}
          |int readCast(Core::Meter& meter) {
          |  return static_cast<const Core::Meter&>(meter).value();
          |}
          |int readAliasCast(Core::Meter& meter) {
          |  return static_cast<const GlobalMeterAlias&>(meter).value();
          |}
          |int readAutoCopy(Core::Meter& meter) {
          |  auto copy = Core::borrowAlias(meter);
          |  return copy.value();
          |}
          |int readAutoRef(Core::Meter& meter) {
          |  auto& ref = Core::borrowAlias(meter);
          |  return ref.value();
          |}
          |int readConstAuto(Core::Meter& meter) {
          |  const auto copy = Core::borrowAlias(meter);
          |  return copy.value();
          |}
          |int readLambdaReturn(Core::Meter& meter) {
          |  auto pick = [](Core::Meter& input) -> const Core::Meter& { return input; };
          |  return pick(meter).value();
          |}
          |int readMemberOperator(Core::OperatorSource& source, Core::Meter& meter) {
          |  return (source + meter).value();
          |}
          |int readFreeOperator(Core::OperatorSource& source, Core::Meter& meter) {
          |  return (source - meter).value();
          |}
          |int readIndexOperator(Core::OperatorSource& source) {
          |  return source[0].value();
          |}
          |int readCallOperator(Core::OperatorSource& source) {
          |  return source().value();
          |}
          |int readAssignmentOperator(Core::OperatorSource& source, Core::Meter& meter) {
          |  return (source = meter).value();
          |}
          |int readGlobal() {
          |  return Core::globalMeter.value();
          |}
          |int readAliasGlobal() {
          |  return Core::aliasGlobal.value();
          |}
          |int use() {
          |  Core::Meter meter;
          |  return meter.viaMutable() + meter.viaConst() + readConst(meter) + readAlias(meter);
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
      cpg.method.nameExact("readConst").parameter.nameExact("meter").typeFullName.l shouldBe List("Core.Meter&")
      cpg.method.nameExact("readConst").call.codeExact("meter.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      inside(cpg.method.nameExact("readContextualConversion").call.filter(_.name.startsWith("operator ")).l) {
        case List(conversionCall) =>
          conversionCall.typeFullName shouldBe "const Core.Meter&"
      }
      cpg.method.nameExact("readAlias").local.nameExact("alias").typeFullName.l shouldBe List("Core.Meter&")
      cpg.method.nameExact("readAlias").call.codeExact("alias.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readTypedef").call.codeExact("meter.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readTypedefAlias").call.codeExact("alias.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readNamespaceAlias").call.codeExact("meter.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readReturn").call.codeExact("Core::borrowAlias(meter).value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readTrailingReturn")
        .call
        .codeExact("Core::borrowTrailing(meter).value()")
        .methodFullName
        .l shouldBe List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readTypedefReturn").call.codeExact("borrowTypedef(meter).value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readCast")
        .call
        .codeExact("static_cast<const Core::Meter&>(meter).value()")
        .methodFullName
        .l shouldBe List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readAliasCast")
        .call
        .codeExact("static_cast<const GlobalMeterAlias&>(meter).value()")
        .methodFullName
        .l shouldBe List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readAutoCopy").call.codeExact("copy.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()")
      cpg.method.nameExact("readAutoRef").call.codeExact("ref.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readConstAuto").call.codeExact("copy.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readLambdaReturn").call.codeExact("pick(meter).value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readMemberOperator").call.codeExact("(source + meter).value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readMemberOperator")
        .call
        .nameExact("operator+")
        .codeExact("source + meter")
        .typeFullName
        .l shouldBe
        List("const Core.Meter&")
      cpg.method.nameExact("readFreeOperator").call.codeExact("(source - meter).value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readFreeOperator")
        .call
        .nameExact("operator-")
        .codeExact("source - meter")
        .typeFullName
        .l shouldBe
        List("const Core.Meter&")
      cpg.method.nameExact("readIndexOperator").call.codeExact("source[0].value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readIndexOperator")
        .call
        .nameExact("operator[]")
        .codeExact("source[0]")
        .typeFullName
        .l shouldBe
        List("const Core.Meter&")
      cpg.method.nameExact("readCallOperator").call.codeExact("source().value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readCallOperator")
        .call
        .nameExact("operator()")
        .codeExact("source()")
        .typeFullName
        .l shouldBe
        List("const Core.Meter&")
      cpg.method
        .nameExact("readAssignmentOperator")
        .call
        .codeExact("(source = meter).value()")
        .methodFullName
        .l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .nameExact("readAssignmentOperator")
        .call
        .nameExact("operator=")
        .codeExact("source = meter")
        .typeFullName
        .l shouldBe List("const Core.Meter&")
      cpg.method
        .fullNameExact("Core.Holder.readField:int()")
        .call
        .codeExact("this->field.value()")
        .methodFullName
        .l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method
        .fullNameExact("Core.Holder.readField:int()")
        .call
        .codeExact("this->aliasField.value()")
        .methodFullName
        .l shouldBe List("Core.Meter.value:int()<const>")
      cpg.method
        .fullNameExact("Core.Holder.readField:int()")
        .call
        .codeExact("this->mutableField.value()")
        .methodFullName
        .l shouldBe List("Core.Meter.value:int()")
      cpg.method.nameExact("readGlobal").call.codeExact("Core::globalMeter.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readAliasGlobal").call.codeExact("Core::aliasGlobal.value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
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
