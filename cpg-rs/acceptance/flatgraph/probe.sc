import io.shiftleft.codepropertygraph.cpgloading.CpgLoader
import io.shiftleft.semanticcpg.language.*

@main def exec(cpgPath: String) = {
  val graph = CpgLoader.load(cpgPath)
  try {
    println(s"FLATGRAPH_OK methods=${graph.method.size} calls=${graph.call.size} files=${graph.file.size}")
    println("METHOD_NAMES=" + graph.method.name.sorted.l.mkString(","))
  } finally graph.close()
}
