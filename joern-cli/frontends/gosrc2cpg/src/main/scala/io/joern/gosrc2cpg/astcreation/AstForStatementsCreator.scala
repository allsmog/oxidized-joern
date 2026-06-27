package io.joern.gosrc2cpg.astcreation

import io.joern.gosrc2cpg.parser.ParserAst.*
import io.joern.gosrc2cpg.parser.{ParserKeys, ParserNodeInfo}
import io.joern.gosrc2cpg.utils.Operator
import io.joern.x2cpg.{Ast, ValidationMode}
import io.joern.x2cpg.utils.AstPropertiesUtil.RootProperties
import io.shiftleft.codepropertygraph.generated.nodes.{ExpressionNew, NewBlock, NewIdentifier, NewLocal}
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, Operators, PropertyNames}
import ujson.Value

import scala.util.Try

trait AstForStatementsCreator(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>
  def astForBlockStatement(blockStmt: ParserNodeInfo, order: Int = -1): Ast = {
    val newBlockNode = blockNode(blockStmt, Defines.empty, Defines.voidTypeName).order(order).argumentIndex(order)
    scope.pushNewScope(newBlockNode)
    var currOrder = 1
    val childAsts =
      blockStmt.json(ParserKeys.List).arrOpt.getOrElse(List()).arr.flatMap { parserJsonValue =>
        val parserNode = createParserNodeInfo(parserJsonValue)
        val r          = astsForStatement(parserNode, currOrder)
        currOrder = currOrder + r.length
        r
      }
    scope.popScope()
    blockAst(newBlockNode, childAsts.toList)
  }

  protected def astsForStatement(statementJson: Value): Seq[Ast] = {
    astsForStatement(createParserNodeInfo(statementJson))
  }
  protected final def astsForStatement(statement: ParserNodeInfo, argIndex: Int = -1): Seq[Ast] = {
    statement.node match {
      case AssignStmt     => astForAssignStatement(statement)
      case BranchStmt     => Seq(astForBranchStatement(statement))
      case BlockStmt      => Seq(astForBlockStatement(statement, argIndex))
      case CaseClause     => astForCaseClause(statement)
      case CommClause     => astForCommClause(statement)
      case DeclStmt       => astForNode(statement.json(ParserKeys.Decl))
      case DeferStmt      => Seq(astForCallWrapperStatement(statement, Operator.deferCall))
      case ExprStmt       => astsForExpression(createParserNodeInfo(statement.json(ParserKeys.X)))
      case ForStmt        => Seq(astForForStatement(statement))
      case GoStmt         => Seq(astForCallWrapperStatement(statement, Operator.goRoutine))
      case IfStmt         => astForIfStatement(statement)
      case IncDecStmt     => Seq(astForIncDecStatement(statement))
      case RangeStmt      => Seq(astForRangeStatement(statement))
      case SelectStmt     => Seq(astForSelectStatement(statement))
      case SendStmt       => Seq(astForSendStatement(statement))
      case SwitchStmt     => Seq(astForSwitchStatement(statement))
      case TypeSwitchStmt => Seq(astForTypeSwitchStatement(statement))
      case ReturnStmt     => Seq(astForReturnStatement(statement))
      case Unknown        => Seq(Ast())
      case _: BaseStmt    => Seq(Ast())
      case _              => astForNode(statement.json)
    }
  }

  private def astForReturnStatement(returnStmt: ParserNodeInfo): Ast = {
    // TODO: Need to handle the tuple return node handling
    val cpgReturn = returnNode(returnStmt, returnStmt.code)
    val expast = returnStmt
      .json(ParserKeys.Results)
      .arrOpt
      .getOrElse(Seq.empty)
      .flatMap(x => astForNode(x))
      .toSeq

    returnAst(cpgReturn, expast)
  }

  private def astForAssignStatement(assignStmt: ParserNodeInfo): Seq[Ast] = {
    assignStmt.json(ParserKeys.Tok).value match {
      case "="   => astForOnlyAssignmentOperator(assignStmt, Operators.assignment)
      case ":="  => astForDeclrationAssignment(assignStmt, Operators.assignment)
      case "*="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentMultiplication)
      case "/="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentDivision)
      case "%="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentModulo)
      case "+="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentPlus)
      case "-="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentMinus)
      case "<<=" => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentShiftLeft)
      case ">>=" => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentArithmeticShiftRight)
      case "&="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentAnd)
      case "^="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentXor)
      case "|="  => astForOnlyAssignmentOperator(assignStmt, Operators.assignmentOr)
      case _     => astForOnlyAssignmentOperator(assignStmt, Operator.unknown)
    }
  }

  private def astForDeclrationAssignment(assignStmt: ParserNodeInfo, op: String): Seq[Ast] = {
    val lhsParserNodes = assignStmt.json(ParserKeys.Lhs).arr.toList.map(createParserNodeInfo)
    val rhsParserNodes = assignStmt.json(ParserKeys.Rhs).arr.toList.map(createParserNodeInfo)
    val (assCallAsts, localAsts) =
      astForAssignmentCallNodes(lhsParserNodes, rhsParserNodes, None, assignStmt.code)
    localAsts ++: assCallAsts
  }

  private def astForOnlyAssignmentOperator(assignStmt: ParserNodeInfo, op: String): Seq[Ast] = {
    val rhsAst = assignStmt
      .json(ParserKeys.Rhs)
      .arr
      .map(createParserNodeInfo)
      .flatMap(astForBooleanLiteral)
    val typeFullName = Some(getTypeFullNameFromAstNode(rhsAst.toSeq))
    val lhsAst = assignStmt
      .json(ParserKeys.Lhs)
      .arr
      .flatMap(astForNode)

    val arguments = lhsAst ++: rhsAst
    val cNode     = callNode(assignStmt, assignStmt.code, op, op, DispatchTypes.STATIC_DISPATCH, None, typeFullName)
    val callAst_  = Seq(callAst(cNode, arguments.toSeq))
    callAst_
  }

  private def astForIncDecStatement(incDecStatement: ParserNodeInfo): Ast = {
    val op = incDecStatement.json(ParserKeys.Tok).value match {
      case "++" => Operators.postIncrement
      case "--" => Operators.postDecrement
      case _    => Operator.unknown
    }
    val cNode   = callNode(incDecStatement, incDecStatement.code, op, op, DispatchTypes.STATIC_DISPATCH)
    val operand = astForNode(incDecStatement.json(ParserKeys.X))
    callAst(cNode, operand)
  }

  private def astForCallWrapperStatement(statement: ParserNodeInfo, operator: String): Ast = {
    val callAsts = Try(statement.json(ParserKeys.Call)).toOption.toSeq.flatMap(astForNode)
    val cNode = callNode(
      statement,
      statement.code,
      operator,
      operator,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(Defines.anyTypeName)
    )
    callAst(cNode, callAsts)
  }

  private def astForSendStatement(sendStmt: ParserNodeInfo): Ast = {
    val chanAst  = Try(sendStmt.json(ParserKeys.Chan)).toOption.toSeq.flatMap(astForNode)
    val valueAst = Try(sendStmt.json(ParserKeys.Value)).toOption.toSeq.flatMap(astForNode)
    val cNode = callNode(
      sendStmt,
      sendStmt.code,
      Operator.channelSend,
      Operator.channelSend,
      DispatchTypes.STATIC_DISPATCH,
      None,
      Some(Defines.anyTypeName)
    )
    callAst(cNode, chanAst ++ valueAst)
  }

  private def astForConditionExpression(condStmt: ParserNodeInfo, explicitArgumentIndex: Option[Int] = None): Ast = {
    val ast = condStmt.node match {
      case ParenExpr   => astForNode(condStmt.json(ParserKeys.X)).headOption.getOrElse(Ast())
      case _: BaseExpr => astsForExpression(condStmt).headOption.getOrElse(Ast())
      case _           => astsForStatement(condStmt).headOption.getOrElse(Ast())
    }
    explicitArgumentIndex.foreach { i =>
      ast.root.foreach { case expr: ExpressionNew => expr.argumentIndex = i }
    }
    ast
  }

  private def astForIfStatement(ifStmt: ParserNodeInfo): Seq[Ast] = {
    // handle init code before condition in if;
    val initParserNode = nullSafeCreateParserNodeInfo(ifStmt.json.obj.get(ParserKeys.Init))
    val initAstBlock   = blockNode(ifStmt, Defines.empty, Defines.voidTypeName)
    scope.pushNewScope(initAstBlock)
    val initAst = blockAst(initAstBlock, astsForStatement(initParserNode, 1).toList)
    scope.popScope()

    val conditionParserNode = createParserNodeInfo(ifStmt.json(ParserKeys.Cond))
    val conditionAst        = astForConditionExpression(conditionParserNode)

    val ifNode = controlStructureNode(ifStmt, ControlStructureTypes.IF, s"if ${conditionParserNode.code}")

    val thenAst = astForBlockStatement(createParserNodeInfo(ifStmt.json(ParserKeys.Body)))

    val elseAst = Try(ifStmt.json(ParserKeys.Else)).toOption match {
      case Some(elseStmt) if createParserNodeInfo(elseStmt).node == BlockStmt =>
        val elseParserNode = createParserNodeInfo(elseStmt)
        val elseNode       = controlStructureNode(elseParserNode, ControlStructureTypes.ELSE, "else")
        val elseAst        = astForBlockStatement(elseParserNode)
        Ast(elseNode).withChild(elseAst)
      case Some(elseStmt) =>
        val elseParserNode = createParserNodeInfo(elseStmt)
        val elseNode       = controlStructureNode(elseParserNode, ControlStructureTypes.ELSE, "else")
        val elseBlock      = blockNode(elseParserNode, Defines.empty, Defines.voidTypeName)
        scope.pushNewScope(elseBlock)
        val a = astsForStatement(elseParserNode)
        setArgumentIndices(a)
        scope.popScope()
        Ast(elseNode).withChild(blockAst(elseBlock, a.toList))
      case _ => Ast()
    }
    val ifAst = controlStructureAst(ifNode, Some(conditionAst), Seq(thenAst, elseAst))
    val withTrueBodyEdge = thenAst.root match {
      case Some(thenRoot) => ifAst.withTrueBodyEdge(ifNode, thenRoot)
      case None           => ifAst
    }
    val withFalseBodyEdge = elseAst.root match {
      case Some(elseRoot) => withTrueBodyEdge.withFalseBodyEdge(ifNode, elseRoot)
      case None           => withTrueBodyEdge
    }
    Seq(initAst, withFalseBodyEdge)
  }

  private def astForSwitchStatement(switchStmt: ParserNodeInfo): Ast = {

    val conditionParserNode = Try(createParserNodeInfo(switchStmt.json(ParserKeys.Tag)))
    val (code, conditionAst) = conditionParserNode.toOption match {
      case Some(node) => (node.code, Some(astForConditionExpression(node)))
      case _          => ("", Some(Ast(literalNode(switchStmt, "true", Defines.Bool))))
    }
    val switchNode = controlStructureNode(switchStmt, ControlStructureTypes.SWITCH, s"switch $code")
    val stmtAsts   = astsForStatement(createParserNodeInfo(switchStmt.json(ParserKeys.Body)))
    controlStructureAst(switchNode, conditionAst, stmtAsts)
  }

  private def astForSelectStatement(selectStmt: ParserNodeInfo): Ast = {
    val selectNode = controlStructureNode(selectStmt, ControlStructureTypes.SWITCH, "select")
    val stmtAsts   = astsForStatement(createParserNodeInfo(selectStmt.json(ParserKeys.Body)))
    controlStructureAst(selectNode, None, stmtAsts)
  }

  private def astForTypeSwitchStatement(typeSwitchStmt: ParserNodeInfo): Ast = {

    val conditionParserNode = Try(createParserNodeInfo(typeSwitchStmt.json(ParserKeys.Assign)))
    val (code, conditionAst) = conditionParserNode.toOption match {
      case Some(node) => (node.code, astForNode(node))
      case _          => ("", Seq.empty)
    }
    val switchNode = controlStructureNode(typeSwitchStmt, ControlStructureTypes.SWITCH, s"switch $code")
    val stmtAsts   = astsForStatement(createParserNodeInfo(typeSwitchStmt.json(ParserKeys.Body)))
    val id = conditionAst
      .flatMap(_.root)
      .collectFirst {
        case x: NewIdentifier => identifierNode(conditionParserNode.get, x.name, x.code, x.typeFullName)
        case x: NewLocal      => identifierNode(conditionParserNode.get, x.name, x.code, x.typeFullName)
      }
      .get
    val identifier = Ast(id)
    val isOp =
      callNode(conditionParserNode.get, s"${id.name}.(type)", Operators.is, Operators.is, DispatchTypes.STATIC_DISPATCH)
    val condition = Option(callAst(isOp, Seq(identifier)))

    val newStmtAst = stmtAsts // TODO: Push conditionAst to the front of the block
    controlStructureAst(switchNode, condition, newStmtAst)
  }

  private def astForCaseClause(caseStmt: ParserNodeInfo): Seq[Ast] = {
    val caseClauseAst = caseStmt.json(ParserKeys.List).arrOpt match {
      case Some(caseConditionList) =>
        caseConditionList.flatMap { caseConditionNode =>
          val caseConditionParserNode = createParserNodeInfo(caseConditionNode)
          val jumpTarget              = jumpTargetNode(caseStmt, "case", s"case ${caseConditionParserNode.code}")
          val labelAsts               = astForNode(caseConditionNode).toList
          Ast(jumpTarget) :: labelAsts
        }
      case _ =>
        val target = jumpTargetNode(caseStmt, "default", "default")
        Seq(Ast(target))
    }

    val caseBodyAst = caseStmt.json(ParserKeys.Body).arr.map(createParserNodeInfo).flatMap(astsForStatement(_)).toList
    caseClauseAst ++: caseBodyAst
  }

  private def astForCommClause(commStmt: ParserNodeInfo): Seq[Ast] = {
    val commAst = Try(commStmt.json(ParserKeys.Comm)).toOption
      .flatMap { comm =>
        val commNode = createParserNodeInfo(comm)
        val target   = jumpTargetNode(commStmt, "case", s"case ${commNode.code}")
        Some(Ast(target) +: astsForStatement(commNode))
      }
      .getOrElse {
        val target = jumpTargetNode(commStmt, "default", "default")
        Seq(Ast(target))
      }
    val bodyAst = commStmt.json(ParserKeys.Body).arrOpt.getOrElse(Seq.empty).flatMap(astsForStatement).toSeq
    commAst ++: bodyAst
  }

  private def astForForStatement(forStmt: ParserNodeInfo): Ast = {

    val initParserNode = nullSafeCreateParserNodeInfo(forStmt.json.obj.get(ParserKeys.Init))
    val condParserNode = nullSafeCreateParserNodeInfo(forStmt.json.obj.get(ParserKeys.Cond))
    val iterParserNode = nullSafeCreateParserNodeInfo(forStmt.json.obj.get(ParserKeys.Post))

    val code    = s"for ${initParserNode.code};${condParserNode.code};${iterParserNode.code}"
    val forNode = controlStructureNode(forStmt, ControlStructureTypes.FOR, code)

    val initAstBlock = blockNode(forStmt, Defines.empty, Defines.voidTypeName)
    scope.pushNewScope(initAstBlock)
    val initAst = blockAst(initAstBlock, astsForStatement(initParserNode, 1).toList)
    scope.popScope()

    val compareAst = astForConditionExpression(condParserNode, Some(2))
    val updateAst  = astsForStatement(iterParserNode, 3)
    val bodyAsts   = astsForStatement(createParserNodeInfo(forStmt.json(ParserKeys.Body)), 4)
    forAst(forNode, Seq(), Seq(initAst), Seq(compareAst), updateAst, bodyAsts)

  }

  private def astForRangeStatement(rangeStmt: ParserNodeInfo): Ast = {

    rangeStmt.json.obj.contains(ParserKeys.Key) match {
      case true =>
        val rangeTargetAsts = rangeTargets(rangeStmt)
        val rangeSourceAst  = withRootOrder(astForNode(rangeStmt.json(ParserKeys.X)).headOption.getOrElse(Ast()), 1)
        val localBlockAst = withRootOrder(
          blockAst(blockNode(rangeStmt, Defines.empty, Defines.voidTypeName), rangeTargetAsts.localAsts.toList),
          2
        )
        val assignmentAst = withRootOrder(astForRangeAssignment(rangeStmt, rangeTargetAsts.targetAsts), 3)
        val bodyAst       = astForBlockStatement(createParserNodeInfo(rangeStmt.json(ParserKeys.Body)), 4)
        val code =
          s"for ${rangeTargetAsts.code} ${rangeStmt.json(ParserKeys.Tok).str} range ${rangeSourceAst.rootCode.getOrElse("")}"
        val forNode  = controlStructureNode(rangeStmt, ControlStructureTypes.FOR, code)
        val rangeAst = controlStructureAst(forNode, None, Seq(rangeSourceAst, localBlockAst, assignmentAst, bodyAst))
        val withInitEdge = rangeSourceAst.root match {
          case Some(initRoot) => rangeAst.withForInitEdge(forNode, initRoot)
          case None           => rangeAst
        }
        val withConditionEdge = rangeSourceAst.root match {
          case Some(conditionRoot) => withInitEdge.withConditionEdges(forNode, List(conditionRoot))
          case None                => withInitEdge
        }
        val withUpdateEdge = assignmentAst.root match {
          case Some(updateRoot) => withConditionEdge.withForUpdateEdge(forNode, updateRoot)
          case None             => withConditionEdge
        }
        bodyAst.root match {
          case Some(bodyRoot) => withUpdateEdge.withForBodyEdge(forNode, bodyRoot)
          case None           => withUpdateEdge
        }
      case false =>
        val initAst  = astForNode(rangeStmt.json(ParserKeys.X))
        val code     = s"for range ${createParserNodeInfo(rangeStmt.json(ParserKeys.X)).code}"
        val forNode  = controlStructureNode(rangeStmt, ControlStructureTypes.FOR, code)
        val stmtAst  = astsForStatement(rangeStmt.json(ParserKeys.Body))
        val rangeAst = controlStructureAst(forNode, None, initAst ++ stmtAst)
        val withInitEdge = initAst.headOption.flatMap(_.root) match {
          case Some(initRoot) => rangeAst.withForInitEdge(forNode, initRoot)
          case None           => rangeAst
        }
        val withConditionEdge = initAst.headOption.flatMap(_.root) match {
          case Some(conditionRoot) => withInitEdge.withConditionEdges(forNode, List(conditionRoot))
          case None                => withInitEdge
        }
        stmtAst.headOption.flatMap(_.root) match {
          case Some(bodyRoot) => withConditionEdge.withForBodyEdge(forNode, bodyRoot)
          case None           => withConditionEdge
        }
    }
  }

  private def rangeTargets(rangeStmt: ParserNodeInfo): RangeTargetAsts = {
    val targetNodes = Seq(
      Some(createParserNodeInfo(rangeStmt.json(ParserKeys.Key))),
      rangeStmt.json.obj.get(ParserKeys.Value).filterNot(_.isNull).map(createParserNodeInfo)
    ).flatten
    val localAsts = targetNodes.flatMap(astForRangeLocal)
    val targetAsts = targetNodes.flatMap { targetNode =>
      if (targetNode.json(ParserKeys.Name).str == "_") Seq.empty else astForNode(targetNode)
    }
    RangeTargetAsts(localAsts, targetAsts, targetNodes.map(_.code).mkString(", "))
  }

  private def astForRangeLocal(rangeVariable: ParserNodeInfo): Option[Ast] = {
    val name = rangeVariable.json(ParserKeys.Name).str
    Option.when(name != "_") {
      val local = localNode(rangeVariable, name, rangeVariable.code, Defines.anyTypeName)
      scope.addToScope(name, (local, Defines.anyTypeName))
      Ast(local)
    }
  }

  private def astForRangeAssignment(rangeStmt: ParserNodeInfo, targetAsts: Seq[Ast]): Ast = {
    val rangeSourceAsts = astForNode(rangeStmt.json(ParserKeys.X))
    val assignmentCode =
      s"${targetAsts.flatMap(_.rootCode).mkString(", ")} ${rangeStmt.json(ParserKeys.Tok).str} range ${rangeSourceAsts.headOption.flatMap(_.rootCode).getOrElse("")}"
    val assignmentNode =
      callNode(rangeStmt, assignmentCode, Operators.assignment, Operators.assignment, DispatchTypes.STATIC_DISPATCH)
    callAst(assignmentNode, targetAsts ++ rangeSourceAsts)
  }

  private def withRootOrder(ast: Ast, order: Int): Ast = {
    ast.root.foreach {
      case block: NewBlock =>
        block.order(order).argumentIndex(order)
      case expression: ExpressionNew =>
        expression.order = order
        expression.argumentIndex = order
      case _ =>
    }
    ast
  }

  private def astForBranchStatement(branchStmt: ParserNodeInfo): Ast = {
    branchStmt.json(ParserKeys.Tok).str match {
      case "break"    => Ast(controlStructureNode(branchStmt, ControlStructureTypes.BREAK, branchStmt.code))
      case "continue" => Ast(controlStructureNode(branchStmt, ControlStructureTypes.CONTINUE, branchStmt.code))
      case "goto"     =>
        // To update the cache of parserNode with the labelled statement
        Try(createParserNodeInfo(branchStmt.json(ParserKeys.Label)(ParserKeys.Obj)(ParserKeys.Decl)))
        Ast(controlStructureNode(branchStmt, ControlStructureTypes.GOTO, branchStmt.code))
      case "fallthrough" =>
        Ast(controlStructureNode(branchStmt, "fallthrough", branchStmt.code))
    }
  }
}

private case class RangeTargetAsts(localAsts: Seq[Ast], targetAsts: Seq[Ast], code: String)
