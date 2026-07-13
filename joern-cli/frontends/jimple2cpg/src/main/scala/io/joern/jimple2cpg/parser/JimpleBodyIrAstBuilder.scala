package io.joern.jimple2cpg.parser

import io.joern.jimple2cpg.parser.JimpleAstGenRunner.*
import io.joern.x2cpg.{Ast, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, Operators, PropertyNames}

import scala.util.matching.Regex

object JimpleBodyIrAstBuilder {

  final case class MethodBodyAst(ast: Ast, cfgEdges: List[(NewNode, NewNode)])

  private final case class SyntheticLocal(name: String, typeFullName: String, offset: Long)

  private val CaughtExceptionRefTypeFullName = "java.lang.Throwable"
  private val CaughtExceptionRefCode         = "@caughtexception"
  private val IdentifierPattern: Regex       = """[A-Za-z_$][A-Za-z0-9_$]*(?:\.[A-Za-z_$][A-Za-z0-9_$]*)*""".r
  private val JimpleCompilerLocalPattern     = """l\d+""".r
  private given ValidationMode               = ValidationMode.Disabled

  def methodBodyAst(method: JimpleMethodInfo): Ast = methodBodyAstWithCfg(method, excludedLocalNames = Set.empty).ast

  def methodBodyAstWithCfg(method: JimpleMethodInfo, excludedLocalNames: Set[String]): MethodBodyAst = {
    method.code match {
      case Some(codeInfo) => methodBodyAstWithCfg(codeInfo, excludedLocalNames)
      case None           => MethodBodyAst(Ast(NewBlock().typeFullName(Defines.Any)), Nil)
    }
  }

  def methodBodyAst(method: JimpleMethodInfo, excludedLocalNames: Set[String]): Ast =
    methodBodyAstWithCfg(method, excludedLocalNames).ast

  def methodBodyAst(codeInfo: JimpleMethodCodeInfo): Ast =
    methodBodyAstWithCfg(codeInfo, excludedLocalNames = Set.empty).ast

  def methodBodyAstWithCfg(codeInfo: JimpleMethodCodeInfo, excludedLocalNames: Set[String]): MethodBodyAst = {
    val lineNumberTable      = codeInfo.lineNumbers.sortBy(_.startPc)
    val declaredLocals       = codeInfo.localVariables.map(_.name).toSet
    val syntheticLocals      = syntheticLocalInfos(codeInfo.bodyIr, declaredLocals)
    val localTypes           = localTypesFor(codeInfo, syntheticLocals)
    val instructionsByOffset = codeInfo.instructions.map(instruction => instruction.offset -> instruction).toMap
    val localAsts =
      localVariableAsts(codeInfo, excludedLocalNames, lineNumberTable) ++
        syntheticLocalAsts(syntheticLocals, excludedLocalNames, lineNumberTable)
    val statementAsts =
      codeInfo.bodyIr.map(entry =>
        entry -> astForBodyIrEntry(
          entry,
          lineNumberFor(lineNumberTable, entry.offset),
          localTypes,
          lineNumberTable,
          instructionsByOffset
        )
      )
    val cfgEdges = cfgEdgesFor(statementAsts, codeInfo.exceptionTable)
    MethodBodyAst(
      Ast(NewBlock().typeFullName(Defines.Any)).withChildren(localAsts ++ statementAsts.map(_._2)),
      cfgEdges
    )
  }

  def methodBodyAst(codeInfo: JimpleMethodCodeInfo, excludedLocalNames: Set[String]): Ast =
    methodBodyAstWithCfg(codeInfo, excludedLocalNames).ast

  private def localVariableAsts(
    codeInfo: JimpleMethodCodeInfo,
    excludedLocalNames: Set[String],
    lineNumberTable: List[JimpleLineNumberInfo]
  ): List[Ast] = {
    codeInfo.localVariables
      .groupBy(local => (local.index, local.name))
      .values
      .flatMap(_.headOption)
      .filterNot(local => local.name == "this" || excludedLocalNames.contains(local.name))
      .toList
      .sortBy(_.index)
      .map { local =>
        Ast(
          NewLocal()
            .name(local.name)
            .code(s"${local.typeName.getOrElse(Defines.Any)} ${local.name}")
            .typeFullName(local.typeName.getOrElse(Defines.Any))
            .lineNumber(lineNumberFor(lineNumberTable, local.startPc.toLong))
        )
      }
  }

  private def syntheticLocalInfos(
    bodyIr: List[JimpleMethodBodyIrInfo],
    declaredLocals: Set[String]
  ): List[SyntheticLocal] = {
    bodyIr
      .flatMap { entry =>
        entry.result
          .filter(name => !declaredLocals.contains(name) && isSyntheticLocal(name, entry))
          .flatMap(name => entry.target.map(typeFullName => SyntheticLocal(name, typeFullName, entry.offset)))
      }
      .foldLeft(List.empty[SyntheticLocal]) { case (locals, local) =>
        if (locals.exists(_.name == local.name)) locals else locals :+ local
      }
  }

  private def syntheticLocalAsts(
    syntheticLocals: List[SyntheticLocal],
    excludedLocalNames: Set[String],
    lineNumberTable: List[JimpleLineNumberInfo]
  ): List[Ast] = {
    syntheticLocals
      .filterNot(local => excludedLocalNames.contains(local.name))
      .map { local =>
        Ast(
          NewLocal()
            .name(local.name)
            .code(s"${local.typeFullName} ${local.name}")
            .typeFullName(local.typeFullName)
            .lineNumber(lineNumberFor(lineNumberTable, local.offset))
        )
      }
  }

  private def localTypesFor(
    codeInfo: JimpleMethodCodeInfo,
    syntheticLocals: List[SyntheticLocal]
  ): Map[String, String] = {
    val declaredLocalTypes = codeInfo.localVariables
      .groupBy(_.name)
      .flatMap { case (name, locals) => locals.headOption.flatMap(_.typeName.map(name -> _)) }
    val bodyResultTypes = codeInfo.bodyIr.flatMap(entry => entry.result.zip(entry.target)).toMap
    declaredLocalTypes ++ bodyResultTypes ++ syntheticLocals.map(local => local.name -> local.typeFullName) +
      (CaughtExceptionRefCode -> CaughtExceptionRefTypeFullName)
  }

  private def astForBodyIrEntry(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String],
    lineNumberTable: List[JimpleLineNumberInfo],
    instructionsByOffset: Map[Long, JimpleBytecodeInstructionInfo]
  ): Ast = {
    entry.operation match {
      case "return" => returnAst(entry, lineNumber, localTypes)
      case "branch" => branchAst(entry, lineNumber, localTypes, lineNumberTable)
      case "switch" => switchAst(entry, lineNumber, localTypes, lineNumberTable, instructionsByOffset.get(entry.offset))
      case "constant"    => Ast()
      case "load"        => Ast()
      case "assignment"  => operatorCallAst(entry, Operators.assignment, lineNumber, localTypes)
      case "binary"      => operatorCallAst(entry, operatorForBinaryEntry(entry), lineNumber, localTypes)
      case "unary"       => operatorCallAst(entry, Operators.minus, lineNumber, localTypes)
      case "field_load"  => fieldLoadAst(entry, lineNumber, localTypes)
      case "field_store" => fieldStoreAst(entry, lineNumber, localTypes)
      case "array_load"  => arrayLoadAst(entry, lineNumber, localTypes)
      case "array_store" => arrayStoreAst(entry, lineNumber, localTypes)
      case "array_length" =>
        operatorCallAst(entry, Operators.lengthOf, lineNumber, localTypes)
      case "compare" =>
        operatorCallAst(
          entry.code,
          Operators.compare,
          entry.arguments.map(expressionAst(_, lineNumber, localTypes)),
          lineNumber,
          "int"
        )
      case "cast" =>
        castAst(entry, lineNumber, localTypes)
      case "type_check" =>
        typeCheckAst(entry, lineNumber, localTypes)
      case "alloc" | "alloc_array" =>
        allocAst(entry, lineNumber, localTypes)
      case "call" =>
        callAst(entry, lineNumber, localTypes)
      case "throw" =>
        throwAst(entry, lineNumber, localTypes)
      case "monitorenter" | "monitorexit" =>
        monitorAst(entry, lineNumber, localTypes)
      case _ =>
        unknownAst(entry, lineNumber, localTypes)
    }
  }

  private def unknownAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    val args    = entry.arguments.map(expressionAst(_, lineNumber, localTypes))
    val unknown = NewUnknown().code(entry.code).typeFullName(Defines.Any).lineNumber(lineNumber)
    Ast(unknown)
      .withChildren(args)
      .withArgEdges(unknown, args.flatMap(_.root), 1)
  }

  private def returnAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    val args = entry.arguments.map(expressionAst(_, lineNumber, localTypes))
    val returnCode = args.flatMap(_.root).map(_.properties(PropertyNames.Code)).toList match {
      case Nil      => "return;"
      case argCodes => s"return ${argCodes.mkString(" ")};"
    }
    val returnNode = NewReturn().code(returnCode).lineNumber(lineNumber)
    Ast(returnNode).withChildren(args).withArgEdges(returnNode, args.flatMap(_.root), 1)
  }

  private def branchAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String],
    lineNumberTable: List[JimpleLineNumberInfo]
  ): Ast = {
    if (isConditionalBranch(entry)) {
      operatorCallAst(entry, operatorForBranchCode(entry.code), lineNumber, localTypes)
    } else {
      gotoAst(entry, lineNumber, lineNumberTable)
    }
  }

  private def gotoAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    lineNumberTable: List[JimpleLineNumberInfo]
  ): Ast = {
    val targetLine = entry.targets.headOption.flatMap(target => lineNumberFor(lineNumberTable, target))
    Ast(NewUnknown().code(s"goto ${targetLine.map(_.toString).getOrElse("<unknown>")}").lineNumber(lineNumber))
  }

  private def switchAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String],
    lineNumberTable: List[JimpleLineNumberInfo],
    instruction: Option[JimpleBytecodeInstructionInfo]
  ): Ast = {
    val switchNode = NewControlStructure()
      .controlStructureType(ControlStructureTypes.SWITCH)
      .code(entry.code)
      .lineNumber(lineNumber)
    val conditionAsts = entry.arguments.headOption.toList.map(expressionAst(_, lineNumber, localTypes))
    val jumpTargets   = switchJumpTargetAsts(instruction, lineNumberTable)
    val ast           = Ast(switchNode).withChildren(conditionAsts ++ jumpTargets)
    conditionAsts.flatMap(_.root).headOption match {
      case Some(conditionRoot) => ast.withConditionEdge(switchNode, conditionRoot)
      case None                => ast
    }
  }

  private def switchJumpTargetAsts(
    instruction: Option[JimpleBytecodeInstructionInfo],
    lineNumberTable: List[JimpleLineNumberInfo]
  ): List[Ast] = {
    instruction match {
      case Some(instruction) if instruction.mnemonic == "tableswitch" =>
        tableSwitchCases(instruction).map { case (name, target) =>
          jumpTargetAst(name, target, lineNumberTable)
        }
      case Some(instruction) if instruction.mnemonic == "lookupswitch" =>
        lookupSwitchCases(instruction).map { case (name, target) =>
          jumpTargetAst(name, target, lineNumberTable)
        }
      case _ => Nil
    }
  }

  private def tableSwitchCases(instruction: JimpleBytecodeInstructionInfo): List[(String, Long)] = {
    val defaultTarget = operandValue(instruction, "default_target")
    val low           = operandValue(instruction, "low")
    val caseTargets   = instruction.operands.filter(_.name == "case_target").map(_.value.toLong)
    val ordinaryCases = low.toList.flatMap(start =>
      caseTargets.zipWithIndex.map { case (target, index) =>
        s"case ${start + index}" -> target
      }
    )
    defaultTarget.map(target => "default" -> target.toLong).toList ++ ordinaryCases
  }

  private def lookupSwitchCases(instruction: JimpleBytecodeInstructionInfo): List[(String, Long)] = {
    val defaultTarget = operandValue(instruction, "default_target").map(target => "default" -> target.toLong).toList
    val ordinaryCases = instruction.operands
      .foldLeft((List.empty[(String, Long)], Option.empty[Int])) {
        case ((cases, _), operand) if operand.name == "match" =>
          (cases, Some(operand.value))
        case ((cases, Some(matchValue)), operand) if operand.name == "target" =>
          (cases :+ (s"case $matchValue" -> operand.value.toLong), None)
        case (state, _) => state
      }
      ._1
    defaultTarget ++ ordinaryCases
  }

  private def operandValue(instruction: JimpleBytecodeInstructionInfo, name: String): Option[Int] =
    instruction.operands.find(_.name == name).map(_.value)

  private def jumpTargetAst(name: String, target: Long, lineNumberTable: List[JimpleLineNumberInfo]): Ast = {
    Ast(
      NewJumpTarget()
        .name(name)
        .code(s"$name:")
        .lineNumber(lineNumberFor(lineNumberTable, target))
    )
  }

  private def monitorAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    val opAsts     = entry.arguments.map(expressionAst(_, lineNumber, localTypes))
    val typeString = opAsts.flatMap(_.root).map(_.properties(PropertyNames.Code)).mkString
    val code = entry.operation match {
      case "monitorenter" => s"entermonitor $typeString"
      case "monitorexit"  => s"exitmonitor $typeString"
      case other          => s"${other}monitor $typeString"
    }
    val unknown = NewUnknown().code(code).lineNumber(lineNumber)
    Ast(unknown).withChildren(opAsts).withArgEdges(unknown, opAsts.flatMap(_.root), 1)
  }

  private def throwAst(entry: JimpleMethodBodyIrInfo, lineNumber: Option[Int], localTypes: Map[String, String]): Ast = {
    val args           = entry.arguments.map(expressionAst(_, lineNumber, localTypes))
    val thrownType     = entry.arguments.headOption.map(expressionTypeFullName(_, localTypes)).getOrElse(Defines.Any)
    val normalizedCode = s"throw new $thrownType()"
    operatorCallAst(normalizedCode, "<operator>.throw", args, lineNumber)
  }

  private def operatorCallAst(
    entry: JimpleMethodBodyIrInfo,
    operatorName: String,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    operatorCallAst(
      entry.code,
      operatorName,
      argumentsForOperator(entry).map(expressionAst(_, lineNumber, localTypes)),
      lineNumber,
      localTypes.get(entry.result.getOrElse("")).orElse(entry.target).getOrElse(Defines.Any)
    )
  }

  private def operatorCallAst(
    code: String,
    operatorName: String,
    args: Seq[Ast],
    lineNumber: Option[Int],
    typeFullName: String = Defines.Any
  ): Ast = {
    val callNode = NewCall()
      .name(operatorName)
      .methodFullName(operatorName)
      .dispatchType(DispatchTypes.STATIC_DISPATCH)
      .code(code)
      .typeFullName(typeFullName)
      .lineNumber(lineNumber)
    Ast(callNode).withChildren(args).withArgEdges(callNode, args.flatMap(_.root), 1)
  }

  private def allocAst(entry: JimpleMethodBodyIrInfo, lineNumber: Option[Int], localTypes: Map[String, String]): Ast = {
    val typeFullName = entry.target.getOrElse(Defines.Any)
    val callNode = NewCall()
      .name(Operators.alloc)
      .methodFullName(Operators.alloc)
      .dispatchType(DispatchTypes.STATIC_DISPATCH)
      .code(entry.code)
      .typeFullName(typeFullName)
      .lineNumber(lineNumber)
    val args     = entry.arguments.map(expressionAst(_, lineNumber, localTypes))
    val allocAst = Ast(callNode).withChildren(args).withArgEdges(callNode, args.flatMap(_.root), 1)
    entry.result.filter(isSyntheticStackLocal) match {
      case Some(result) =>
        operatorCallAst(
          s"$result = ${entry.code}",
          Operators.assignment,
          List(identifierAst(result, lineNumber, localTypes), allocAst),
          lineNumber,
          typeFullName
        )
      case None =>
        if (entry.operation == "alloc_array" && entry.result.contains(entry.code)) {
          Ast()
        } else {
          allocAst
        }
    }
  }

  private def arrayLoadAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    entry.arguments match {
      case array :: index :: _ =>
        indexAccessAst(
          entry.code,
          expressionAst(array, lineNumber, localTypes),
          expressionAst(index, lineNumber, localTypes),
          lineNumber,
          entry.target.getOrElse(expressionTypeFullName(entry.code, localTypes))
        )
      case _ => operatorCallAst(entry, Operators.indexAccess, lineNumber, localTypes)
    }
  }

  private def arrayStoreAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    entry.arguments match {
      case array :: index :: value :: _ =>
        val lhs =
          indexAccessAst(
            s"$array[$index]",
            expressionAst(array, lineNumber, localTypes),
            expressionAst(index, lineNumber, localTypes),
            lineNumber,
            entry.target.getOrElse(expressionTypeFullName(s"$array[$index]", localTypes))
          )
        val rhs = expressionAst(value, lineNumber, localTypes)
        operatorCallAst(
          entry.code,
          Operators.assignment,
          List(lhs, rhs),
          lineNumber,
          entry.target.getOrElse(Defines.Any)
        )
      case _ =>
        operatorCallAst(entry, Operators.assignment, lineNumber, localTypes)
    }
  }

  private def fieldLoadAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    fieldAccessAst(entry.result.getOrElse(entry.code), lineNumber, localTypes).getOrElse {
      operatorCallAst(entry, Operators.fieldAccess, lineNumber, localTypes)
    }
  }

  private def fieldStoreAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    val lhsCode = entry.result.getOrElse(entry.code.takeWhile(_ != '=')).trim
    val lhs     = fieldAccessAst(lhsCode, lineNumber, localTypes)
    val rhs     = entry.arguments.lastOption.map(expressionAst(_, lineNumber, localTypes))
    (lhs, rhs) match {
      case (Some(lhsAst), Some(rhsAst)) =>
        operatorCallAst(
          entry.code,
          Operators.assignment,
          List(lhsAst, rhsAst),
          lineNumber,
          entry.target.getOrElse(Defines.Any)
        )
      case _ =>
        operatorCallAst(entry, Operators.assignment, lineNumber, localTypes)
    }
  }

  private def castAst(entry: JimpleMethodBodyIrInfo, lineNumber: Option[Int], localTypes: Map[String, String]): Ast = {
    entry.target match {
      case Some(targetType) =>
        val args = typeRefAst(targetType, lineNumber) :: entry.arguments.map(expressionAst(_, lineNumber, localTypes))
        operatorCallAst(entry.code, Operators.cast, args, lineNumber, targetType)
      case None =>
        operatorCallAst(entry, Operators.cast, lineNumber, localTypes)
    }
  }

  private def typeCheckAst(
    entry: JimpleMethodBodyIrInfo,
    lineNumber: Option[Int],
    localTypes: Map[String, String]
  ): Ast = {
    entry.target match {
      case Some(targetType) =>
        val args = entry.arguments.map(expressionAst(_, lineNumber, localTypes)) :+ typeRefAst(targetType, lineNumber)
        operatorCallAst(entry.code, Operators.instanceOf, args, lineNumber, "boolean")
      case None =>
        operatorCallAst(entry, Operators.instanceOf, lineNumber, localTypes)
    }
  }

  private def callAst(entry: JimpleMethodBodyIrInfo, lineNumber: Option[Int], localTypes: Map[String, String]): Ast = {
    val methodFullName = entry.methodFullName.orElse(entry.target).getOrElse(entry.code.takeWhile(_ != '('))
    val argumentCodes = entry.receiver match {
      case Some(receiver) if entry.arguments.headOption.contains(receiver) => entry.arguments.drop(1)
      case _                                                               => entry.arguments
    }
    val callNodeBase = NewCall()
      .name(callName(entry, methodFullName))
      .methodFullName(methodFullName)
      .dispatchType(entry.dispatchType.getOrElse(DispatchTypes.DYNAMIC_DISPATCH))
      .code(callCode(entry, methodFullName, argumentCodes))
      .typeFullName(callTypeFullName(methodFullName, entry.signature))
      .lineNumber(lineNumber)
    val callNode      = entry.signature.fold(callNodeBase)(callNodeBase.signature)
    val receiverAsts  = entry.receiver.toList.map(expressionAst(_, lineNumber, localTypes))
    val args          = argumentCodes.map(expressionAst(_, lineNumber, localTypes))
    val bootstrapArgs = entry.bootstrapArguments.map(expressionAst(_, lineNumber, localTypes))
    val allArgs       = args ++ bootstrapArgs
    val ast = Ast(callNode)
      .withChildren(receiverAsts)
      .withChildren(allArgs)
      .withArgEdges(callNode, receiverAsts.flatMap(_.root), 0)
      .withArgEdges(callNode, allArgs.flatMap(_.root), 1)
    receiverAsts.flatMap(_.root).headOption match {
      case Some(receiver) => ast.withReceiverEdge(callNode, receiver)
      case None           => ast
    }
  }

  private def argumentsForOperator(entry: JimpleMethodBodyIrInfo): List[String] = {
    entry.operation match {
      case "assignment" | "field_store" =>
        entry.result.toList ++ entry.arguments
      case "field_load" if entry.arguments.nonEmpty =>
        entry.arguments ++ entry.result.toList
      case "branch" =>
        branchArguments(entry)
      case _ =>
        entry.arguments
    }
  }

  private def expressionAst(code: String, lineNumber: Option[Int], localTypes: Map[String, String]): Ast = {
    if (isLiteral(code)) {
      Ast(NewLiteral().code(code).typeFullName(expressionTypeFullName(code, localTypes)).lineNumber(lineNumber))
    } else if (isAllocationCode(code)) {
      inlineAllocAst(code, lineNumber, localTypes)
    } else {
      arrayAccessParts(code) match {
        case Some((array, index)) =>
          indexAccessAst(
            code,
            expressionAst(array, lineNumber, localTypes),
            expressionAst(index, lineNumber, localTypes),
            lineNumber,
            expressionTypeFullName(code, localTypes)
          )
        case None =>
          fieldAccessAst(code, lineNumber, localTypes).getOrElse(identifierAst(code, lineNumber, localTypes))
      }
    }
  }

  private def inlineAllocAst(code: String, lineNumber: Option[Int], localTypes: Map[String, String]): Ast = {
    val callNode = NewCall()
      .name(Operators.alloc)
      .methodFullName(Operators.alloc)
      .dispatchType(DispatchTypes.STATIC_DISPATCH)
      .code(code)
      .typeFullName(expressionTypeFullName(code, localTypes))
      .lineNumber(lineNumber)
    val args = allocationDimensionArgs(code).map(expressionAst(_, lineNumber, localTypes))
    Ast(callNode).withChildren(args).withArgEdges(callNode, args.flatMap(_.root), 1)
  }

  private def identifierAst(code: String, lineNumber: Option[Int], localTypes: Map[String, String]): Ast = {
    val name = code match {
      case IdentifierPattern() => code.split('.').lastOption.getOrElse(code)
      case _                   => code
    }
    Ast(
      NewIdentifier()
        .name(name)
        .code(code)
        .typeFullName(expressionTypeFullName(code, localTypes))
        .lineNumber(lineNumber)
    )
  }

  private def indexAccessAst(
    code: String,
    arrayAst: Ast,
    indexAst: Ast,
    lineNumber: Option[Int],
    typeFullName: String = Defines.Any
  ): Ast = {
    val indexAccess = NewCall()
      .name(Operators.indexAccess)
      .methodFullName(Operators.indexAccess)
      .dispatchType(DispatchTypes.STATIC_DISPATCH)
      .code(code)
      .typeFullName(typeFullName)
      .lineNumber(lineNumber)
    val args = List(arrayAst, indexAst)
    Ast(indexAccess).withChildren(args).withArgEdges(indexAccess, args.flatMap(_.root), 1)
  }

  private def typeRefAst(typeFullName: String, lineNumber: Option[Int]): Ast = {
    Ast(
      NewTypeRef()
        .code(typeRefCode(typeFullName))
        .typeFullName(typeFullName)
        .lineNumber(lineNumber)
    )
  }

  private def fieldAccessAst(code: String, lineNumber: Option[Int], localTypes: Map[String, String]): Option[Ast] = {
    fieldAccessParts(code).map { case (base, fieldName) =>
      val fieldAccess = NewCall()
        .name(Operators.fieldAccess)
        .methodFullName(Operators.fieldAccess)
        .dispatchType(DispatchTypes.STATIC_DISPATCH)
        .code(code)
        .typeFullName(expressionTypeFullName(code, localTypes))
        .lineNumber(lineNumber)
      val baseAst = Ast(
        NewIdentifier()
          .name(base)
          .code(base)
          .typeFullName(expressionTypeFullName(base, localTypes))
          .lineNumber(lineNumber)
      )
      val fieldIdentifierAst = Ast(
        NewFieldIdentifier()
          .canonicalName(fieldName)
          .code(fieldName)
          .lineNumber(lineNumber)
      )
      val args = List(baseAst, fieldIdentifierAst)
      Ast(fieldAccess).withChildren(args).withArgEdges(fieldAccess, args.flatMap(_.root), 1)
    }
  }

  private def arrayAccessParts(code: String): Option[(String, String)] = {
    val trimmed = code.trim
    if (trimmed.startsWith("new ") || !trimmed.endsWith("]")) {
      None
    } else {
      var depth     = 0
      var openIndex = -1
      var index     = trimmed.length - 1
      while (index >= 0 && openIndex == -1) {
        trimmed.charAt(index) match {
          case ']' => depth += 1
          case '[' =>
            depth -= 1
            if (depth == 0) openIndex = index
          case _ =>
        }
        index -= 1
      }
      Option.when(openIndex > 0 && openIndex < trimmed.length - 1) {
        trimmed.take(openIndex) -> trimmed.substring(openIndex + 1, trimmed.length - 1)
      }
    }
  }

  private def allocationDimensionArgs(code: String): List[String] = {
    val trimmed = code.trim
    if (!trimmed.startsWith("new ")) {
      Nil
    } else {
      val args  = List.newBuilder[String]
      var index = trimmed.indexOf('[')
      while (index >= 0 && index < trimmed.length) {
        val closeIndex = matchingCloseBracket(trimmed, index)
        if (closeIndex < 0) {
          index = -1
        } else {
          val size = trimmed.substring(index + 1, closeIndex).trim
          if (size.nonEmpty) {
            args += size
          }
          index = trimmed.indexOf('[', closeIndex + 1)
        }
      }
      args.result()
    }
  }

  private def matchingCloseBracket(code: String, openIndex: Int): Int = {
    var depth = 0
    var index = openIndex
    while (index < code.length) {
      code.charAt(index) match {
        case '[' => depth += 1
        case ']' =>
          depth -= 1
          if (depth == 0) {
            return index
          }
        case _ =>
      }
      index += 1
    }
    -1
  }

  private def fieldAccessParts(code: String): Option[(String, String)] = {
    val trimmed = code.trim
    if (trimmed.startsWith("new ")) {
      None
    } else {
      val dotIndex = trimmed.lastIndexOf('.')
      Option.when(dotIndex > 0 && dotIndex < trimmed.length - 1) {
        trimmed.take(dotIndex) -> trimmed.substring(dotIndex + 1)
      }
    }
  }

  private def callName(entry: JimpleMethodBodyIrInfo, methodFullName: String): String = {
    val beforeSignature = methodFullName.takeWhile(_ != ':')
    val fromFullName    = beforeSignature.split('.').lastOption.filter(_.nonEmpty)
    val fromTarget      = entry.target.flatMap(_.split('.').lastOption).filter(_.nonEmpty)
    fromFullName
      .orElse(fromTarget)
      .getOrElse(entry.code.takeWhile(ch => ch != '(' && ch != ' ').split('.').lastOption.getOrElse(entry.code))
  }

  private def callCode(entry: JimpleMethodBodyIrInfo, methodFullName: String, argumentCodes: List[String]): String = {
    if (isConstructorCall(methodFullName)) {
      entry.receiver match {
        case Some(receiver) if receiver.trim.startsWith("new ") =>
          s"${receiver.trim}(${argumentCodes.mkString(", ")})"
        case Some(receiver) =>
          s"$receiver.${constructorTypeName(methodFullName)}(${argumentCodes.mkString(", ")})"
        case None =>
          entry.code
      }
    } else {
      entry.code
    }
  }

  private def callTypeFullName(methodFullName: String, signature: Option[String]): String = {
    val returnTypeFromFullName = methodFullName.split(":", 2).lift(1).map(_.takeWhile(_ != '('))
    returnTypeFromFullName
      .orElse(signature.map(_.takeWhile(_ != '(')))
      .filter(_.nonEmpty)
      .getOrElse(Defines.Any)
  }

  private def isConstructorCall(methodFullName: String): Boolean =
    methodFullName.takeWhile(_ != ':').endsWith(".<init>")

  private def constructorTypeName(methodFullName: String): String = {
    val owner = methodFullName.takeWhile(_ != ':').stripSuffix(".<init>")
    owner.split('.').lastOption.filter(_.nonEmpty).getOrElse(owner)
  }

  private def typeRefCode(typeFullName: String): String = {
    if (typeFullName.contains('.')) {
      typeFullName.substring(typeFullName.lastIndexOf('.') + 1)
    } else {
      typeFullName
    }
  }

  private def expressionTypeFullName(code: String, localTypes: Map[String, String]): String = {
    val trimmed = code.trim
    if (trimmed.startsWith("new ")) {
      val newTarget = trimmed.stripPrefix("new ")
      val baseType  = newTarget.takeWhile(ch => ch != '[' && ch != '(').trim
      val dimensions = newTarget
        .drop(baseType.length)
        .sliding(2)
        .count(_ == "[]")
      val sizedDimensions = allocationDimensionArgs(trimmed).size
      val typeName        = baseType + "[]" * (dimensions + sizedDimensions)
      Option.when(typeName.nonEmpty)(typeName).getOrElse(Defines.Any)
    } else {
      localTypes.getOrElse(trimmed, Defines.Any)
    }
  }

  private def isAllocationCode(code: String): Boolean =
    code.trim.startsWith("new ")

  private def isSyntheticStackLocal(name: String): Boolean =
    name.matches("\\$stack\\d+")

  private def isSyntheticLocal(name: String, entry: JimpleMethodBodyIrInfo): Boolean =
    isSyntheticStackLocal(name) ||
      (entry.arguments.contains(CaughtExceptionRefCode) && JimpleCompilerLocalPattern.matches(name))

  private def lineNumberFor(lineNumberTable: List[JimpleLineNumberInfo], offset: Long): Option[Int] = {
    lineNumberTable.takeWhile(_.startPc <= offset).lastOption.map(_.lineNumber)
  }

  private def isLiteral(code: String): Boolean = {
    code == "null" ||
    code.endsWith(".class") ||
    code.matches("-?\\d+(?:\\.\\d+)?") ||
    (code.startsWith("\"") && code.endsWith("\""))
  }

  private def operatorForBinaryEntry(entry: JimpleMethodBodyIrInfo): String = {
    val operator = entry.arguments match {
      case List(left, right) =>
        val code  = entry.code.trim
        val inner = if (code.startsWith("(") && code.endsWith(")")) code.drop(1).dropRight(1) else code
        inner.stripPrefix(left).stripSuffix(right).trim
      case _ => ""
    }
    operatorForBinaryOperator(operator).getOrElse(operatorForBinaryCode(entry.code))
  }

  private def operatorForBinaryOperator(operator: String): Option[String] = {
    operator match {
      case "+"   => Some(Operators.addition)
      case "-"   => Some(Operators.subtraction)
      case "*"   => Some(Operators.multiplication)
      case "/"   => Some(Operators.division)
      case "%"   => Some(Operators.modulo)
      case "<<"  => Some(Operators.shiftLeft)
      case ">>>" => Some(Operators.arithmeticShiftRight)
      case ">>"  => Some(Operators.logicalShiftRight)
      case "&"   => Some(Operators.and)
      case "|"   => Some(Operators.or)
      case "^"   => Some(Operators.xor)
      case _     => None
    }
  }

  private def operatorForBinaryCode(code: String): String = {
    if (code.contains(" + ")) Operators.addition
    else if (code.contains(" - ")) Operators.subtraction
    else if (code.contains(" * ")) Operators.multiplication
    else if (code.contains(" / ")) Operators.division
    else if (code.contains(" % ")) Operators.modulo
    else if (code.contains(" << ")) Operators.shiftLeft
    else if (code.contains(" >>> ")) Operators.arithmeticShiftRight
    else if (code.contains(" >> ")) Operators.logicalShiftRight
    else if (code.contains(" & ")) Operators.and
    else if (code.contains(" | ")) Operators.or
    else if (code.contains(" ^ ")) Operators.xor
    else Operators.assignment
  }

  private def cfgEdgesFor(
    statementAsts: List[(JimpleMethodBodyIrInfo, Ast)],
    exceptionTable: List[JimpleExceptionHandlerInfo]
  ): List[(NewNode, NewNode)] = {
    val rootedStatements = statementAsts.flatMap { case (entry, ast) => ast.root.map(root => entry -> root) }
    val rootByOffset     = rootedStatements.map { case (entry, root) => entry.offset -> root }.toMap
    def rootAtOrAfterOffset(offset: Long): Option[NewNode] =
      rootByOffset.get(offset).orElse(rootedStatements.find { case (entry, _) => entry.offset >= offset }.map(_._2))
    val fallthroughEdges = rootedStatements
      .sliding(2)
      .collect { case List((entry, root), (_, nextRoot)) if hasFallthrough(entry) => root -> nextRoot }
      .toList
    val targetEdges = rootedStatements.flatMap { case (entry, root) =>
      entry.targets.flatMap(targetOffset => rootAtOrAfterOffset(targetOffset).map(root -> _))
    }
    val exceptionEdges = exceptionTable.flatMap { handler =>
      val handlerRoot = rootAtOrAfterOffset(handler.handlerPc.toLong)
      handlerRoot.toList.flatMap { targetRoot =>
        rootedStatements.collect {
          case (entry, sourceRoot) if entry.offset >= handler.startPc && entry.offset < handler.endPc =>
            sourceRoot -> targetRoot
        }
      }
    }
    (fallthroughEdges ++ targetEdges ++ exceptionEdges).distinct
  }

  private def hasFallthrough(entry: JimpleMethodBodyIrInfo): Boolean = {
    entry.operation match {
      case "return" => false
      case "switch" => false
      case "jsr"    => false
      case "ret"    => false
      case "branch" => isConditionalBranch(entry)
      case _        => true
    }
  }

  private def isConditionalBranch(entry: JimpleMethodBodyIrInfo): Boolean = {
    entry.operation == "branch" && !branchMnemonic(entry.code).startsWith("goto")
  }

  private def branchArguments(entry: JimpleMethodBodyIrInfo): List[String] = {
    val mnemonic = branchMnemonic(entry.code)
    val operands =
      if (mnemonic.startsWith("if_icmp") || mnemonic.startsWith("if_acmp")) entry.arguments.reverse
      else entry.arguments
    mnemonic match {
      case "ifeq" | "ifne" | "iflt" | "ifle" | "ifgt" | "ifge" => operands ++ List("0")
      case "ifnull" | "ifnonnull"                              => operands ++ List("null")
      case _                                                   => operands
    }
  }

  private def operatorForBranchCode(code: String): String = {
    branchMnemonic(code) match {
      case "ifeq" | "if_icmpeq" | "if_acmpeq" | "ifnull"    => Operators.equals
      case "ifne" | "if_icmpne" | "if_acmpne" | "ifnonnull" => Operators.notEquals
      case "iflt" | "if_icmplt"                             => Operators.lessThan
      case "ifle" | "if_icmple"                             => Operators.lessEqualsThan
      case "ifgt" | "if_icmpgt"                             => Operators.greaterThan
      case "ifge" | "if_icmpge"                             => Operators.greaterEqualsThan
      case _                                                => Operators.assignment
    }
  }

  private def branchMnemonic(code: String): String = {
    code.takeWhile(ch => ch != '(' && !ch.isWhitespace)
  }
}
