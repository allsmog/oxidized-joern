@main def exec(inputPath: String) = {
  importCode(inputPath, "return-flow-probe")
  cpg.call.nameExact("sink").toList.sortBy(_.method.name).foreach { sink =>
    val flows = sink.argument.reachableBy(cpg.call.nameExact("source")).toList
    println("RESULT|" + sink.method.name + "|" + flows.nonEmpty)
  }
}
