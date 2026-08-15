import io.shiftleft.codepropertygraph.cpgloading.CpgLoader
import io.shiftleft.semanticcpg.language.*
import java.nio.charset.StandardCharsets
import java.util.Base64

def emit(id: String, values: List[Any]): Unit = {
  def canonical(value: Any): String = {
    val rendered = value.toString
    if id == "parameter-types" || id == "return-types" then
      rendered.replace("::", ".").split("\\.").last
    else if id == "returns" then rendered.stripSuffix(";")
    else rendered
  }
  val normalized = values.map(canonical).sorted.mkString("\u001f")
  val encoded = Base64.getEncoder.encodeToString(normalized.getBytes(StandardCharsets.UTF_8))
  println(s"LANGUAGE\t$id\t$encoded")
}

@main def exec(cpgPath: String) = {
  val cpg = CpgLoader.load(cpgPath)
  try {
    emit("user-methods", cpg.method.isExternal(false).nameExact("source", "transform", "sink", "main").name.l)
    emit("source-parameter", cpg.method.isExternal(false).nameExact("source").parameter.name.l)
    emit("transform-parameter", cpg.method.isExternal(false).nameExact("transform").parameter.name.l)
    emit("sink-parameter", cpg.method.isExternal(false).nameExact("sink").parameter.name.l)
    emit("main-parameter", cpg.method.isExternal(false).nameExact("main").parameter.name.l)
    emit("user-calls", cpg.call.nameExact("source", "transform", "sink").name.l)
    emit("main-calls", cpg.method.isExternal(false).nameExact("main").call.nameExact("source", "transform", "sink").name.l)
    emit("call-targets", cpg.call.nameExact("source", "transform", "sink").callee.name.l)
    emit("parameter-types", cpg.method.isExternal(false).nameExact("source", "transform", "sink", "main").parameter.typeFullName.l)
    emit("return-types", cpg.method.isExternal(false).nameExact("source", "transform", "sink", "main").methodReturn.typeFullName.l)
    emit("returns", cpg.method.isExternal(false).nameExact("source", "transform", "sink", "main").ast.isReturn.code.l)
    emit("references", cpg.identifier.nameExact("raw", "clean").refsTo.name.dedup.l)
    emit("source-flow", cpg.call.nameExact("sink").argument(1).reachableBy(cpg.call.nameExact("source")).code.l)
  } finally cpg.close()
}
