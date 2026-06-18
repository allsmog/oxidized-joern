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
          |int choose(int value, int scale = 1) { return value + scale; }
          |long choose(long value) { return value; }
          |template <typename T>
          |T choose(T value) { return value; }
          |int preferDefault(Base& value) { return 1; }
          |short preferDefault(Mid& value, int scale = 1) { return 2; }
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
          |  long wide,
          |  Core::Mid& mid,
          |  Core::Chooser& chooser,
          |  Core::Box& box,
          |  Core::Source& source,
          |  const Core::ConstSource& constSource,
          |  Core::ValueSource& valueSource
          |) {
          |  return Core::pick(leaf) +
          |    Core::pick(constLeaf) +
          |    Core::pick(Core::makeLeaf()) +
          |    Core::pick(mid) +
          |    Core::pickPtr(leafPtr) +
          |    Core::pickPtr(constLeafPtr) +
          |    Core::choose(leaf) +
          |    Core::choose(1) +
          |    Core::choose(1, 2) +
          |    Core::choose(wide) +
          |    Core::preferDefault(leaf) +
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
          |template <typename T>
          |class Holder {
          |public:
          |  T& operator[](int index);
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
          |T makeExplicit();
          |template <typename T>
          |T& ref(T& value) { return value; }
          |template <typename T>
          |const T& cref(const T& value) { return value; }
          |}
          |long use(Core::Leaf& leaf, const Core::Leaf& constLeaf, Core::Holder<Core::Leaf>& holder) {
          |  return Core::pick(Core::id(Core::makeLeaf())) +
          |    Core::pick(Core::idAuto(Core::makeLeaf())) +
          |    Core::pick(Core::idAutoForward(Core::makeLeaf())) +
          |    Core::pick(Core::idAutoBranch(true, Core::makeLeaf(), Core::makeLeaf())) +
          |    Core::pick(Core::idAutoBranchLocal(true, Core::makeLeaf(), Core::makeLeaf())) +
          |    Core::pickNumber(Core::add(1, 2)) +
          |    Core::pick(Core::idDecltype(leaf)) +
          |    Core::pick(Core::idDecltypeParen(Core::makeLeaf())) +
          |    Core::pick(Core::idDecltypeBranch(true, leaf, leaf)) +
          |    Core::pick(Core::first(holder)) +
          |    Core::pick(Core::firstRef(holder)) +
          |    Core::pick(Core::makeExplicit<Core::Leaf>()) +
          |    Core::pick(Core::ref(leaf)) +
          |    Core::pick(Core::cref(constLeaf));
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
      cpg.method.nameExact("use").call.codeExact("Core::makeExplicit<Core::Leaf>()").typeFullName.l shouldBe
        List("Core.Leaf")
      cpg.method.nameExact("use").call.codeExact("Core::ref(leaf)").typeFullName.l shouldBe
        List("Core.Leaf&")
      cpg.method.nameExact("use").call.codeExact("Core::cref(constLeaf)").typeFullName.l shouldBe
        List("const Core.Leaf&")
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
        .codeExact("Core::pick(Core::makeExplicit<Core::Leaf>())")
        .methodFullName
        .l shouldBe
        List("Core.pick:char(Leaf&&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::ref(leaf))").methodFullName.l shouldBe
        List("Core.pick:short(Mid&)")
      cpg.method.nameExact("use").call.codeExact("Core::pick(Core::cref(constLeaf))").methodFullName.l shouldBe
        List("Core.pick:long(Mid&)")
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
      cpg.method.nameExact("readMemberOperator").call.nameExact("operator+").codeExact("source + meter").typeFullName.l shouldBe
        List("const Core.Meter&")
      cpg.method.nameExact("readFreeOperator").call.codeExact("(source - meter).value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readFreeOperator").call.nameExact("operator-").codeExact("source - meter").typeFullName.l shouldBe
        List("const Core.Meter&")
      cpg.method.nameExact("readIndexOperator").call.codeExact("source[0].value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readIndexOperator").call.nameExact("operator[]").codeExact("source[0]").typeFullName.l shouldBe
        List("const Core.Meter&")
      cpg.method.nameExact("readCallOperator").call.codeExact("source().value()").methodFullName.l shouldBe
        List("Core.Meter.value:int()<const>")
      cpg.method.nameExact("readCallOperator").call.nameExact("operator()").codeExact("source()").typeFullName.l shouldBe
        List("const Core.Meter&")
      cpg.method.nameExact("readAssignmentOperator").call.codeExact("(source = meter).value()").methodFullName.l shouldBe
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
