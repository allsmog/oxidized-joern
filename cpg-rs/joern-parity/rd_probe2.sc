import io.joern.dataflowengineoss.passes.reachingdef._
import io.shiftleft.codepropertygraph.generated.nodes._
@main def exec(inputPath: String, mname: String) = {
  importCode(inputPath, "p")
  val m = cpg.method.nameExact(mname).head
  val problem = ReachingDefProblem.create(m)
  val solution = new DataFlowSolver().calculateMopSolutionForwards(problem)
  val fg = problem.flowGraph.asInstanceOf[ReachingDefFlowGraph]
  val n2n = fg.numberToNode
  val tf = solution.problem.transferFunction.asInstanceOf[ReachingDefTransferFunction]
  val gen = tf.gen
  def code(n: Any): String = n match {
    case x: CfgNode => x.code.replace("\n","\\n").take(20)
    case _ => n.toString
  }
  n2n.toList.sortBy(_._1).foreach { case (num, node) =>
    val g = gen.getOrElse(node, scala.collection.mutable.BitSet()).toList.sorted.mkString(",")
    val in = solution.in(node).toList.sorted.mkString(",")
    println(s"N$num|${node.label}|${code(node)}|gen={$g}|in={$in}")
  }
  println("=== REACHING_DEF edges ===")
  m.ast.foreach { src =>
    src match {
      case c: CfgNode =>
        c._reachingDefOut.foreach { dst =>
          // find edge property
          println(s"EDGE|${code(c)} -> ${code(dst.asInstanceOf[CfgNode])}")
        }
      case _ =>
    }
  }
}
