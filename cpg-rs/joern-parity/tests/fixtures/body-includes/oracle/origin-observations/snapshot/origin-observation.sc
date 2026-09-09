import scala.jdk.CollectionConverters._
import io.shiftleft.codepropertygraph.generated.nodes.{AstNode, StoredNode}
import java.nio.charset.StandardCharsets
import java.util.Base64

// Separate raw observations, never a replacement for oracle.sc's projection.
// Every present property is UTF-8 Base64; ~ means absent, and an empty encoded
// field is a present empty string. Do not trim or normalize source properties.
def encoded(value: String): String =
  Base64.getEncoder.encodeToString(value.getBytes(StandardCharsets.UTF_8))

def property(node: StoredNode, key: String): String = {
  val pm = node.propertiesMap.asScala
  Option(pm.getOrElse(key, null)).map(v => encoded(v.toString)).getOrElse("~")
}

@main def exec(inputPath: String) = {
  os.list(os.Path(inputPath)).filter(os.isDir(_)).sortBy(_.last).foreach { path =>
    val caseName = path.last
    println("ORIGIN_CASE|" + caseName)
    importCode(path.toString, caseName + "-raw-origins")
    val keys = List("NAME", "CODE", "FULL_NAME", "METHOD_FULL_NAME", "TYPE_FULL_NAME",
                    "ORDER", "ARGUMENT_INDEX", "FILENAME", "LINE_NUMBER", "COLUMN_NUMBER",
                    "LINE_NUMBER_END", "COLUMN_NUMBER_END", "OFFSET", "OFFSET_END")
    var astCount = 0
    var typeCount = 0
    var sourceFileEdgeCount = 0
    def properties(node: StoredNode): String =
      keys.map(k => k + "=" + property(node, k)).mkString("|")
    def sourceFiles(node: StoredNode, occurrence: String): Unit = {
      val rows = scala.collection.mutable.ArrayBuffer[String]()
      node.outE.foreach { edge =>
        if (edge.label == "SOURCE_FILE") {
          val target = edge.dst.asInstanceOf[StoredNode]
          rows += "ORIGIN_SOURCE_FILE|" + occurrence + "|SOURCE_NODE_ID=" + node.id +
            "|TARGET_NODE_ID=" + target.id + "|TARGET_LABEL=" + target.label +
            "|TARGET_NAME=" + property(target, "NAME") +
            "|TARGET_FILENAME=" + property(target, "FILENAME")
        }
      }
      rows.sorted.foreach { row => println(row); sourceFileEdgeCount += 1 }
    }
    // Same method sorting and child ordering as the unchanged canonical oracle.
    // The ordinal includes nested-method walks exactly as that AST view does.
    cpg.method.sortBy(_.fullName).toList.foreach { method =>
      var index = 0
      def visit(node: AstNode, depth: Int, parentIndex: Int): Unit = {
        val current = index
        index += 1
        astCount += 1
        val occurrence = "VIEW=" + encoded(method.fullName) + "|INDEX=" + current
        println("ORIGIN_AST|" + occurrence + "|PARENT_INDEX=" + parentIndex +
          "|DEPTH=" + depth + "|NODE_ID=" + node.id + "|LABEL=" + node.label +
          "|VIEW_METHOD_FILENAME=" + property(method, "FILENAME") + "|" + properties(node))
        sourceFiles(node, occurrence)
        node.astChildren.toList.sortBy(_.order).foreach(child => visit(child, depth + 1, current))
      }
      visit(method, 0, -1)
    }
    // Include type declarations outside a method AST, while leaving duplicate
    // appearances explicit. TYPE_DECL filename is independent of method owner.
    cpg.typeDecl.sortBy(_.fullName).foreach { node =>
      typeCount += 1
      val occurrence = "TYPE_DECL_FULL_NAME=" + property(node, "FULL_NAME")
      println("ORIGIN_TYPE_DECL|" + occurrence + "|NODE_ID=" + node.id + "|" + properties(node))
      sourceFiles(node, occurrence)
    }
    println("ORIGIN_END|" + caseName + "|AST_OCCURRENCES=" + astCount +
      "|TYPE_DECL_RECORDS=" + typeCount + "|SOURCE_FILE_EDGES=" + sourceFileEdgeCount)
  }
}
