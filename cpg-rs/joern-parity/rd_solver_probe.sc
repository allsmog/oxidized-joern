import io.joern.dataflowengineoss.passes.reachingdef._
@main def exec(inputPath: String) = {
  importCode(inputPath, "p")
  val m = cpg.method.nameExact("bsearch").head
  val problem = ReachingDefProblem.create(m)
  val solution = new DataFlowSolver().calculateMopSolutionForwards(problem)
  val fg = problem.flowGraph.asInstanceOf[ReachingDefFlowGraph]
  val n2n = fg.numberToNode
  def lbl(n: Any): String = n2n.find(_._2 == n).map(_._1.toString).getOrElse("?")
  // print pred structure + in-set for each node in RPO
  fg.allNodesReversePostOrder.foreach { node =>
    val num = problem.flowGraph.asInstanceOf[ReachingDefFlowGraph]
    val preds = fg.pred(node).map(p => p.asInstanceOf[io.shiftleft.codepropertygraph.generated.nodes.CfgNode].code.replace("\n","\\n").take(12)).mkString(",")
    val inset = solution.in(node).toList.sorted.mkString(",")
    val code = node.code.replace("\n","\\n").take(16)
    println(s"NODE|${node.label}:$code preds=[$preds] in={$inset}")
  }
}
