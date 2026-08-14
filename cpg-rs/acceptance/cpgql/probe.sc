import io.shiftleft.codepropertygraph.cpgloading.CpgLoader
import io.shiftleft.semanticcpg.language.*
import java.nio.charset.StandardCharsets
import java.util.Base64

def emit(id: String, values: List[Any]): Unit = {
  val normalized = values.map(_.toString).sorted.mkString("\u001f")
  val encoded = Base64.getEncoder.encodeToString(normalized.getBytes(StandardCharsets.UTF_8))
  println(s"CPGQL\t$id\t$encoded")
}

@main def exec(cpgPath: String) = {
  val cpg = CpgLoader.load(cpgPath)
  try {
    emit("methods", cpg.method("helper|main").name.l)
    emit("parameters", cpg.method("main").parameter.name.l)
    emit("calls", cpg.method("main").call.name.l)
    emit("argument", cpg.call("strcpy").argument(2).code.l)
    emit("name-exact", cpg.call.nameExact("getenv", "strcpy").name.l)
    emit("name-not", cpg.method("main").call.nameNot("<operator>.*").name.l)
    emit("line-range", cpg.method("main").call.lineNumberGt(8).lineNumberLte(13).code.l)
    emit("where", cpg.method("helper|main").where(_.call.name("strcpy")).name.l)
    emit("where-not", cpg.method("helper|main").whereNot(_.call.name("strcpy")).name.l)
    emit("ast-kind", cpg.method("main").ast.isCall.name.l)
    emit("in-ast", cpg.call("strcpy").inAst.isMethod.name.l)
    emit("call-out", cpg.method("main").callOut.name.l)
    emit("cfg-next", cpg.call("strcpy").cfgNext.code.l)
    emit("ref", cpg.identifier("input").where(_.method.name("main")).refsTo.name.l)
    emit("type", cpg.identifier("input").where(_.method.name("main")).typ.fullName.l)
    emit("reaching-def", cpg.call("getenv").reachingDefOut.code.l)
    emit("reachable-by", cpg.call("strcpy").argument(2).reachableBy(cpg.call("getenv")).code.l)
    emit("repeat", cpg.call("strcpy").repeat(_.astParent)(_.until(_.isMethod)).isMethod.name.l)
    emit("condition", cpg.controlStructure.condition.code.l)
    emit("empty", cpg.call("strcpy").argument(1).reachableBy(cpg.call("getenv")).code.l)
    emit("assignments", cpg.assignment.code.l)
    emit("returns", cpg.method("main").ast.isReturn.code.l)
    emit("boolean-and", cpg.method.and(_.name("main"), _.call("strcpy")).name.l)
    emit("boolean-or", cpg.method.or(_.name("main"), _.name("helper")).name.l)
    emit("in-call", cpg.identifier("input").where(_.method.name("main")).inCall.name.l)
    emit("repeat-max-depth", cpg.method("main").repeat(_.astChildren)(_.emit(_.isCall).maxDepth(2)).isCall.name.l)
    emit("flow-paths", cpg.call("strcpy").argument(2).reachableByFlows(cpg.call("getenv")).map(_.elements.map(_.code).mkString(" -> ")).l)
    emit("dominates", cpg.call("getenv").dominates.code.l)
    emit("dominated-by", cpg.call("strcpy").dominatedBy.code.l)
    emit("post-dominates", cpg.call("strcpy").postDominates.code.l)
    emit("post-dominated-by", cpg.call("strcpy").postDominatedBy.code.l)
    emit("controls", cpg.call.code("argc > 1").controls.code.l)
    emit("controlled-by", cpg.call("strcpy").controlledBy.code.l)
  } finally cpg.close()
}
