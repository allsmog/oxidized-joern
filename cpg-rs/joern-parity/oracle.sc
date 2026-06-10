import scala.jdk.CollectionConverters._
import io.shiftleft.codepropertygraph.generated.nodes
import io.shiftleft.codepropertygraph.generated.nodes.{AstNode, Method, StoredNode}
import scala.collection.mutable

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

  // --- EDGES section. Every node is addressed <homeMethodFullName>#<idx>,
  // where home = nearest enclosing METHOD (itself for methods) and idx = the
  // node's line index within that method's dump block (deterministic on both
  // sides since the AST dumps are byte-identical). Non-AST-walk nodes use
  // T:/F:/NB:/NS:/D: label prefixes.
  val addr = mutable.Map[Long, String]()
  cpg.method.sortBy(_.fullName).toList.foreach { m =>
    var idx = 0
    def rec(n: AstNode, insideNested: Boolean): Unit = {
      if (!insideNested && !addr.contains(n.id)) addr(n.id) = s"${m.fullName}#$idx"
      idx += 1
      val nested = insideNested || (n.isInstanceOf[Method] && (n.id != m.id))
      n.astChildren.toList.sortBy(_.order).foreach(c => rec(c, nested))
    }
    rec(m, false)
  }
  def address(n: StoredNode): Option[String] = n match {
    case t: nodes.Type => Some(s"T:" + t.fullName)
    case f: nodes.File => Some(s"F:" + f.name)
    case nb: nodes.NamespaceBlock => Some(s"NB:" + nb.fullName)
    case ns: nodes.Namespace => Some(s"NS:" + ns.name)
    case td: nodes.TypeDecl if !addr.contains(td.id) => Some(s"D:" + td.fullName)
    case other => addr.get(other.id)
  }
  val kinds = Set("ARGUMENT","CALL","CFG","CONDITION","CONTAINS","DO_BODY","EVAL_TYPE",
                  "FALSE_BODY","FOR_BODY","FOR_INIT","FOR_UPDATE","PARAMETER_LINK",
                  "REF","SOURCE_FILE","TRUE_BODY")
  val lines = mutable.SortedSet[String]()
  cpg.all.foreach { n =>
    n.outE.foreach { e =>
      if (kinds(e.label)) {
        for (s <- address(e.src.asInstanceOf[StoredNode]);
             d <- address(e.dst.asInstanceOf[StoredNode]))
          lines += s"${e.label} $s -> $d"
      }
    }
  }
  lines.foreach(l => println("EDGES|" + l))

  // FLOWS section: REACHING_DEF (def -> use, with VARIABLE) — the data-dependence
  // backbone reachableBy walks. Edge carries a VARIABLE property.
  val flows = mutable.SortedSet[String]()
  cpg.all.foreach { n =>
    n.outE.foreach { e =>
      if (e.label == "REACHING_DEF") {
        val v = Option(e.property).map(_.toString).getOrElse("")
        for (s <- address(e.src.asInstanceOf[StoredNode]);
             d <- address(e.dst.asInstanceOf[StoredNode]))
          flows += s"REACHING_DEF[$v] $s -> $d"
      }
    }
  }
  flows.foreach(l => println("FLOWS|" + l))
}
