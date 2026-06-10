import scala.jdk.CollectionConverters._
import io.shiftleft.codepropertygraph.generated.nodes.AstNode

@main def exec(inputPath: String) = {
  importCode(inputPath, "proj")
  val keys = List("NAME","CODE","TYPE_FULL_NAME","FULL_NAME","METHOD_FULL_NAME",
                  "SIGNATURE","ORDER","ARGUMENT_INDEX","DISPATCH_TYPE")
  def dump(n: AstNode, depth: Int): Unit = {
    val pm = n.propertiesMap.asScala
    val props = keys.flatMap(k => Option(pm.getOrElse(k, null)).map(v =>
      s"$k=" + v.toString.replace("\n","\\n").trim)).mkString(" ")
    println("AST|" + "  "*depth + n.label + " " + props)
    n.astChildren.toList.sortBy(_.order).foreach(c => dump(c, depth+1))
  }
  // Only real user-defined methods (exclude <global> and <operator>.* scaffolding).
  cpg.method.filterNot(_.name.startsWith("<")).sortBy(_.fullName).toList.foreach { m =>
    dump(m, 0); println("AST|")
  }
}
