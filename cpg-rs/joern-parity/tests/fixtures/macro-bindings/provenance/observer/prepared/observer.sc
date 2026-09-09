import scala.jdk.CollectionConverters._
import io.shiftleft.codepropertygraph.generated.nodes
import io.shiftleft.codepropertygraph.generated.nodes.{AstNode, Method, StoredNode}
import scala.collection.mutable

def dumpCanonical(cpg: io.shiftleft.codepropertygraph.generated.Cpg): Unit = {
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
          // Match the AST/CODE and Rust transport: one record per line.
          flows += s"REACHING_DEF[${v.replace("\n", "\\n")}] $s -> $d"
      }
    }
  }
  flows.foreach(l => println("FLOWS|" + l))
}

// Separate supplemental protocol: never changes the canonical projection.
def typedValue(value: Any): ujson.Value = value match {
  case null => ujson.Obj("kind" -> "null")
  case s: String => ujson.Obj("kind" -> "string", "value" -> s)
  case b: java.lang.Boolean => ujson.Obj("kind" -> "boolean", "value" -> b.booleanValue())
  case n: java.lang.Number => ujson.Obj("kind" -> "number", "class" -> n.getClass.getName, "value" -> n.toString)
  case c: java.lang.Character => ujson.Obj("kind" -> "character", "value" -> c.toString)
  case a if a.getClass.isArray =>
    val entries = (0 until java.lang.reflect.Array.getLength(a)).map(i => typedValue(java.lang.reflect.Array.get(a, i)))
    ujson.Obj("kind" -> "array", "class" -> a.getClass.getName, "values" -> ujson.Arr.from(entries))
  case m: java.util.Map[?, ?] =>
    val entries = m.entrySet().asScala.toSeq.map(e => ujson.Obj("key" -> typedValue(e.getKey), "value" -> typedValue(e.getValue)))
    ujson.Obj("kind" -> "map", "class" -> m.getClass.getName, "entries" -> ujson.Arr.from(entries.sortBy(ujson.write(_))))
  case xs: java.lang.Iterable[?] =>
    ujson.Obj("kind" -> "sequence", "class" -> xs.getClass.getName, "values" -> ujson.Arr.from(xs.iterator().asScala.map(typedValue).toSeq))
  case xs: scala.collection.Iterable[?] =>
    ujson.Obj("kind" -> "sequence", "class" -> xs.getClass.getName, "values" -> ujson.Arr.from(xs.iterator.map(typedValue).toSeq))
  case other => throw new IllegalArgumentException("Unsupported stored property class: " + other.getClass.getName)
}

def supplemental(cpg: io.shiftleft.codepropertygraph.generated.Cpg, caseName: String): Unit = {
  val allNodes = cpg.all.toList.sortBy(_.id)
  var edgeCount = 0L
  println("OBS_BEGIN|" + ujson.write(ujson.Obj("schemaVersion" -> 1, "case" -> caseName)))
  allNodes.foreach { n =>
    val props = n.propertiesMap.asScala.toSeq.sortBy(_._1).map { case (k, v) => (k, typedValue(v)) }
    println("OBS_NODE|" + ujson.write(ujson.Obj("id" -> n.id.toString, "label" -> n.label, "properties" -> ujson.Obj.from(props))))
    // Lists preserve repeated equal edge records. IDs join only within this CPG.
    val edges = n.outE.toList.map { e =>
      ujson.Obj("source" -> e.src.asInstanceOf[StoredNode].id.toString,
        "destination" -> e.dst.asInstanceOf[StoredNode].id.toString,
        "label" -> e.label, "property" -> typedValue(e.property))
    }.sortBy(ujson.write(_))
    edges.foreach { e => println("OBS_EDGE|" + ujson.write(e)); edgeCount += 1 }
  }
  println("OBS_END|" + ujson.write(ujson.Obj("case" -> caseName,
    "nodeCount" -> allNodes.size.toString, "edgeCount" -> edgeCount.toString)))
}

@main def exec(manifestPath: String): Unit = {
  val manifest = ujson.read(os.read(os.Path(manifestPath)))
  manifest("cases").arr.foreach { row =>
    val name = row("case").str
    val copied = java.nio.file.Paths.get(row("copiedCpg").str)
    require(java.nio.file.Files.isRegularFile(copied) && java.nio.file.Files.size(copied) > 0)
    // false removes the close-time persistence path in this pinned runtime.
    val loaded = io.shiftleft.codepropertygraph.generated.Cpg.withStorage(copied, false)
    try {
      println("CASE|" + name)
      dumpCanonical(loaded)
      supplemental(loaded, name)
    } finally loaded.close()
  }
}
