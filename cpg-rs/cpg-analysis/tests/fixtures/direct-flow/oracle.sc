@main def exec(inputPath: String) = {
  importCode(inputPath, "direct-flow-probe")
  val entries = cpg.method.name(".*_entry").toList.sortBy(_.name)
  entries.foreach { entry =>
    val sources = entry.ast.isCall.nameExact("source").toList
    val flows = cpg.call.nameExact("sink").argument.reachableBy(sources.iterator).toList
    println("RESULT|" + entry.name + "|" + flows.nonEmpty)
  }
}
