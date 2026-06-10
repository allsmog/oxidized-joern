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
  // All methods, scaffolding included (<global> wrappers, <operator>.* stubs).
  cpg.method.sortBy(_.fullName).toList.foreach { m =>
    dump(m, 0); println("AST|")
  }
  // Non-method scaffolding nodes, one NODES| line each.
  def nprops(n: io.shiftleft.codepropertygraph.generated.nodes.StoredNode, ks: List[String]): String = {
    val pm = n.propertiesMap.asScala
    ks.flatMap(k => Option(pm.getOrElse(k, null)).map(v =>
      s"$k=" + v.toString.replace("\n","\\n").trim)).mkString(" ")
  }
  cpg.metaData.foreach(m => println("NODES|META_DATA " + nprops(m, List("LANGUAGE"))))
  cpg.file.sortBy(_.name).foreach(f => println("NODES|FILE " + nprops(f, List("NAME","ORDER"))))
  cpg.namespaceBlock.sortBy(_.fullName).foreach(n =>
    println("NODES|NAMESPACE_BLOCK " + nprops(n, List("NAME","FULL_NAME","FILENAME","ORDER"))))
  cpg.namespace.sortBy(_.name).foreach(n => println("NODES|NAMESPACE " + nprops(n, List("NAME","ORDER"))))
  cpg.typeDecl.sortBy(_.fullName).foreach(t =>
    println("NODES|TYPE_DECL " + nprops(t, List("NAME","FULL_NAME","CODE","IS_EXTERNAL","AST_PARENT_TYPE","AST_PARENT_FULL_NAME","FILENAME","ORDER"))))
  cpg.typ.sortBy(_.fullName).foreach(t =>
    println("NODES|TYPE " + nprops(t, List("NAME","FULL_NAME","TYPE_DECL_FULL_NAME"))))
}
