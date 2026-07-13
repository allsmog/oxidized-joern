package io.joern.x2cpg.testfixtures

import flatgraph.misc.TestUtils.*
import io.shiftleft.codepropertygraph.generated.nodes.{NewBlock, NewCall, NewMethod, NewMethodReturn}
import io.shiftleft.codepropertygraph.generated.{Cpg, DispatchTypes, EdgeTypes}
import org.scalatest.matchers.should.Matchers
import org.scalatest.wordspec.AnyWordSpec

class CpgEquivalenceTests extends AnyWordSpec with Matchers {

  "CpgEquivalence" should {

    "treat equal CPGs as equivalent modulo runtime node ids" in withCpgs(
      sampleCpg(reverseInsertion = false),
      sampleCpg(reverseInsertion = true)
    ) { (actual, expected) =>
      CpgEquivalence.compare(actual, expected).isEquivalent shouldBe true
    }

    "report a mutated node property" in withCpgs(
      sampleCpg(methodName = "renamed"),
      sampleCpg()
    ) { (actual, expected) =>
      val comparison = CpgEquivalence.compare(actual, expected)

      comparison.isEquivalent shouldBe false
      comparison.diff should include("[Nodes only in actual]")
      comparison.diff should include("NAME=renamed")
      comparison.diff should include("[Nodes only in expected]")
      comparison.diff should include("NAME=main")
    }

    "report an edge mismatch" in withCpgs(
      sampleCpg(includeCallAstEdge = false),
      sampleCpg(includeCallAstEdge = true)
    ) { (actual, expected) =>
      val comparison = CpgEquivalence.compare(actual, expected)

      comparison.isEquivalent shouldBe false
      comparison.diff should include("[Edges only in expected]")
      comparison.diff should include("EDGE|")
    }
  }

  private def sampleCpg(
    methodName: String = "main",
    reverseInsertion: Boolean = false,
    includeCallAstEdge: Boolean = true
  ): Cpg = {
    val cpg   = Cpg.empty
    val graph = cpg.graph

    val method = NewMethod()
      .name(methodName)
      .fullName(methodName)
      .signature("int()")
      .code(s"int $methodName()")
      .filename("Test0.c")
      .isExternal(false)
      .lineNumber(1)
      .order(1)
    val methodReturn = NewMethodReturn()
      .typeFullName("int")
      .code("RET")
      .lineNumber(1)
      .order(2)
    val block = NewBlock()
      .code("{}")
      .typeFullName("void")
      .argumentIndex(1)
      .order(1)
    val call = NewCall()
      .name("foo")
      .methodFullName("foo")
      .signature("int()")
      .dispatchType(DispatchTypes.STATIC_DISPATCH)
      .typeFullName("int")
      .code("foo()")
      .argumentIndex(1)
      .order(1)

    val nodes =
      if (reverseInsertion) Seq(call, block, methodReturn, method)
      else Seq(method, methodReturn, block, call)
    nodes.foreach(graph.addNode)

    graph.applyDiff { diffGraph =>
      diffGraph.addEdge(method, methodReturn, EdgeTypes.AST)
      diffGraph.addEdge(method, block, EdgeTypes.AST)
      if (includeCallAstEdge) {
        diffGraph.addEdge(block, call, EdgeTypes.AST)
      }
    }

    cpg
  }

  private def withCpgs[T](actual: Cpg, expected: Cpg)(fun: (Cpg, Cpg) => T): T = {
    try fun(actual, expected)
    finally {
      actual.close()
      expected.close()
    }
  }
}
