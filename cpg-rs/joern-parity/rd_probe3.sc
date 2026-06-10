import io.joern.dataflowengineoss.passes.reachingdef._
import io.shiftleft.codepropertygraph.generated.nodes._
@main def exec(inputPath: String, mname: String) = {
  importCode(inputPath, "p")
  val m = cpg.method.nameExact(mname).head
  val problem = ReachingDefProblem.create(m)
  val fg = problem.flowGraph.asInstanceOf[ReachingDefFlowGraph]
  val n2n = fg.numberToNode
  val tf = problem.transferFunction.asInstanceOf[OptimizedReachingDefTransferFunction]
  def code(n: Any): String = n match {
    case x: CfgNode => x.code.replace("\n","\\n").take(20)
    case _ => n.toString
  }
  println("=== loneIdentifiers ===")
  tf.loneIdentifiers.foreach { case (call, defs) =>
    println(s"LONE|call=${code(call)}|defs=${defs.mkString(",")}")
  }
  println("=== gen (optimized) ===")
  n2n.toList.sortBy(_._1).foreach { case (num, node) =>
    val g = tf.gen.getOrElse(node, scala.collection.mutable.BitSet()).toList.sorted.mkString(",")
    println(s"N$num|${node.label}|${code(node)}|gen={$g}")
  }
}
