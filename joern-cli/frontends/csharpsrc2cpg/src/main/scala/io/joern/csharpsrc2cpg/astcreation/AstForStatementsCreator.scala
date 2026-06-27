package io.joern.csharpsrc2cpg.astcreation

import io.joern.csharpsrc2cpg.CSharpOperators
import io.joern.csharpsrc2cpg.parser.DotNetJsonAst.*
import io.joern.csharpsrc2cpg.parser.{DotNetNodeInfo, ParserKeys}
import io.joern.x2cpg.{Ast, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.{NewFieldIdentifier, NewJumpLabel, NewLiteral, NewLocal}
import io.shiftleft.codepropertygraph.generated.{ControlStructureTypes, DispatchTypes, ModifierTypes, Operators}

import scala.::
import scala.util.{Try, Success, Failure}

trait AstForStatementsCreator(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>

  def astForStatement(statement: ujson.Value): Seq[Ast] = {
    astForStatement(createDotNetNodeInfo(statement))
  }

  /** Separates the `AST` result of a conditional expression into the condition as well as any declared variables to
    * prepend.
    * @param conditionAst
    *   the condition.
    * @param prependIfBody
    *   statements to prepend to the `if`/`then` body.
    */
  final case class ConditionAstResult(conditionAst: Ast, prependIfBody: List[Ast])

  // TODO: Use this method elsewhere on other control structures
  protected def astForConditionNode(condNode: DotNetNodeInfo): ConditionAstResult = {
    lazy val default = ConditionAstResult(astForNode(condNode).headOption.getOrElse(Ast()), List.empty)
    condNode.node match {
      case x: PatternExpr =>
        astsForIsPatternExpression(condNode) match {
          case head :: tail => ConditionAstResult(head, tail)
          case Nil =>
            logger.warn(
              s"Unable to handle pattern expression $x in condition expression, resorting to default behaviour"
            )
            default
        }
      case _ => default
    }
  }

  private def astForIfStatement(ifStmt: DotNetNodeInfo): Seq[Ast] = {
    val conditionNode                                   = createDotNetNodeInfo(ifStmt.json(ParserKeys.Condition))
    val ConditionAstResult(conditionAst, prependIfBody) = astForConditionNode(conditionNode)

    val thenNode     = createDotNetNodeInfo(ifStmt.json(ParserKeys.Statement))
    val thenAst: Ast = astForBlock(thenNode, prefixAsts = prependIfBody)
    val ifNode =
      controlStructureNode(ifStmt, ControlStructureTypes.IF, s"if (${conditionNode.code})")
    val elseAst = ifStmt.json(ParserKeys.Else) match {
      case elseStmt: ujson.Obj => astForElseStatement(createDotNetNodeInfo(elseStmt))
      case _                   => Ast()
    }

    val ifAst       = controlStructureAst(ifNode, Some(conditionAst), Seq(thenAst, elseAst))
    val astWithThen = thenAst.root.map(ifAst.withTrueBodyEdge(ifNode, _)).getOrElse(ifAst)
    val astWithElse = elseAst.root.map(astWithThen.withFalseBodyEdge(ifNode, _)).getOrElse(astWithThen)
    Seq(astWithElse)
  }

  protected def astForStatement(nodeInfo: DotNetNodeInfo): Seq[Ast] = {
    nodeInfo.node match {
      case ExpressionStatement    => astForExpressionStatement(nodeInfo)
      case EmptyStatement         => Nil
      case GlobalStatement        => astForGlobalStatement(nodeInfo)
      case LabeledStatement       => astForLabeledStatement(nodeInfo)
      case LockStatement          => astForLockStatement(nodeInfo)
      case CheckedStatement       => astForCheckedStatement(nodeInfo)
      case UnsafeStatement        => astForUnsafeStatement(nodeInfo)
      case FixedStatement         => astForFixedStatement(nodeInfo)
      case IfStatement            => astForIfStatement(nodeInfo)
      case ThrowStatement         => astForThrowStatement(nodeInfo)
      case TryStatement           => astForTryStatement(nodeInfo)
      case ForEachStatement       => astForForEachStatement(nodeInfo)
      case ForStatement           => astForForStatement(nodeInfo)
      case DoStatement            => astForDoStatement(nodeInfo)
      case WhileStatement         => astForWhileStatement(nodeInfo)
      case SwitchStatement        => astForSwitchStatement(nodeInfo)
      case UsingStatement         => astForUsingStatement(nodeInfo)
      case LocalFunctionStatement => astForLocalFunctionStatement(nodeInfo)
      case _: JumpStatement       => astForJumpStatement(nodeInfo)
      case _                      => notHandledYet(nodeInfo)
    }
  }

  private def astForLockStatement(lockStmt: DotNetNodeInfo): Seq[Ast] = {
    val lockBlock = blockNode(lockStmt)
    val modifier  = Ast(modifierNode(lockStmt, "SYNCHRONIZED"))
    val expressionAst = Try(lockStmt.json(ParserKeys.Expression)).toOption
      .collect { case expression: ujson.Obj => createDotNetNodeInfo(expression) }
      .map(astForNode)
      .getOrElse(Seq.empty)
    val bodyAst = Try(lockStmt.json(ParserKeys.Statement)).toOption
      .collect { case statement: ujson.Obj => createDotNetNodeInfo(statement) }
      .map(statementNode => astForBlock(statementNode))
      .getOrElse(Ast(blockNode(lockStmt)))

    Seq(Ast(lockBlock).withChild(modifier).withChildren(expressionAst).withChild(bodyAst))
  }

  private def astForCheckedStatement(checkedStmt: DotNetNodeInfo): Seq[Ast] = {
    val keyword = Try(checkedStmt.json(ParserKeys.Keyword)(ParserKeys.Value).str).getOrElse("checked")
    val modifierType = keyword match {
      case "unchecked" => "UNCHECKED"
      case _           => "CHECKED"
    }
    val checkedBlock = blockNode(checkedStmt)
    val modifier     = Ast(modifierNode(checkedStmt, modifierType))
    val bodyAst = Try(checkedStmt.json(ParserKeys.Statement)).toOption
      .collect { case statement: ujson.Obj => createDotNetNodeInfo(statement) }
      .map(statementNode => astForBlock(statementNode))
      .getOrElse(Ast(blockNode(checkedStmt)))

    Seq(Ast(checkedBlock).withChild(modifier).withChild(bodyAst))
  }

  private def astForUnsafeStatement(unsafeStmt: DotNetNodeInfo): Seq[Ast] = {
    val unsafeBlock = blockNode(unsafeStmt)
    val modifier    = Ast(modifierNode(unsafeStmt, "UNSAFE"))
    val bodyAst = Try(unsafeStmt.json(ParserKeys.Statement)).toOption
      .collect { case statement: ujson.Obj => createDotNetNodeInfo(statement) }
      .map(statementNode => astForBlock(statementNode))
      .getOrElse(Ast(blockNode(unsafeStmt)))

    Seq(Ast(unsafeBlock).withChild(modifier).withChild(bodyAst))
  }

  private def astForFixedStatement(fixedStmt: DotNetNodeInfo): Seq[Ast] = {
    val fixedBlock = blockNode(fixedStmt)
    val modifier   = Ast(modifierNode(fixedStmt, "FIXED"))
    val declarationAsts = Try(fixedStmt.json(ParserKeys.Declaration)).toOption
      .collect { case declaration: ujson.Obj => createDotNetNodeInfo(declaration) }
      .map(astForNode)
      .getOrElse(Seq.empty)
    val bodyAst = Try(fixedStmt.json(ParserKeys.Statement)).toOption
      .collect { case statement: ujson.Obj => createDotNetNodeInfo(statement) }
      .map(statementNode => astForBlock(statementNode))
      .getOrElse(Ast(blockNode(fixedStmt)))

    Seq(Ast(fixedBlock).withChild(modifier).withChildren(declarationAsts).withChild(bodyAst))
  }

  private def astForLocalFunctionStatement(nodeInfo: DotNetNodeInfo): Seq[Ast] = {
    astForMethodDeclaration(nodeInfo)
  }

  private def astForSwitchLabel(labelNode: DotNetNodeInfo): Seq[Ast] = {
    val caseNode = jumpTargetNode(labelNode, "case", labelNode.code)
    labelNode.node match {
      case CasePatternSwitchLabel =>
        val patternNode = createDotNetNodeInfo(labelNode.json(ParserKeys.Pattern))
        val patternAsts = Try(patternNode.json(ParserKeys.Expression)).toOption
          .collect { case expression: ujson.Obj =>
            astForNode(expression)
          }
          .getOrElse {
            Seq(Ast(literalNode(patternNode, patternNode.code, Defines.Any)))
          }
        Ast(caseNode) +: patternAsts
      case CaseSwitchLabel =>
        val valueNode = createDotNetNodeInfo(labelNode.json(ParserKeys.Value))
        val valueAsts = valueNode.node match {
          case ConstantPattern | RelationalPattern =>
            astForNode(valueNode.json(ParserKeys.Expression))
          case _ => astForNode(valueNode)
        }
        Ast(caseNode) +: valueAsts
      case DefaultSwitchLabel => Seq(Ast(caseNode))
      case _                  => Seq(Ast())
    }
  }

  private def astForSwitchStatement(switchStmt: DotNetNodeInfo): Seq[Ast] = {
    val comparatorNode    = createDotNetNodeInfo(switchStmt.json(ParserKeys.Expression))
    val comparatorNodeAst = astForExpression(comparatorNode).headOption

    val switchBodyAsts: Seq[Ast] = switchStmt
      .json(ParserKeys.Sections)
      .arr
      .flatMap { section =>
        val sectionNode = section match {
          case value: ujson.Obj   => createDotNetNodeInfo(value)
          case value: ujson.Value => nullSafeCreateParserNodeInfo(Option(value))
        }

        val labelNodes = sectionNode.json(ParserKeys.Labels).arr
        labelNodes.flatMap(labelNode => astForSwitchLabel(createDotNetNodeInfo(labelNode))) :+ astForBlock(sectionNode)
      }
      .toSeq

    val switchNode = controlStructureNode(switchStmt, ControlStructureTypes.SWITCH, s"switch (${comparatorNode.code})")
    val switchBody = Ast(blockNode(switchStmt)).withChildren(switchBodyAsts)
    val switchAst  = controlStructureAst(switchNode, comparatorNodeAst, switchBody :: Nil)
    Seq(switchBody.root.map(switchAst.withTrueBodyEdge(switchNode, _)).getOrElse(switchAst))
  }

  private def astForWhileStatement(whileStmt: DotNetNodeInfo): Seq[Ast] = {
    val condition                                     = createDotNetNodeInfo(whileStmt.json(ParserKeys.Condition))
    val ConditionAstResult(conditionAst, prependBody) = astForConditionNode(condition)
    val whileBlock                                    = createDotNetNodeInfo(whileStmt.json(ParserKeys.Statement))
    val whileBlockAst                                 = astForBlock(whileBlock, prefixAsts = prependBody)

    val code = s"while (${condition.code})"

    val whileNode = controlStructureNode(whileStmt, ControlStructureTypes.WHILE, code)

    val whileAst =
      Ast(whileNode)
        .withChild(whileBlockAst)
        .withChild(conditionAst)
        .withConditionEdges(whileNode, conditionAst.root.toList)

    Seq(whileBlockAst.root.map(whileAst.withTrueBodyEdge(whileNode, _)).getOrElse(whileAst))
  }

  private def astForDoStatement(doStmt: DotNetNodeInfo): Seq[Ast] = {
    val condition                                     = createDotNetNodeInfo(doStmt.json(ParserKeys.Condition))
    val ConditionAstResult(conditionAst, prependBody) = astForConditionNode(condition)
    val doBlock                                       = createDotNetNodeInfo(doStmt.json(ParserKeys.Statement))
    val doBlockAst                                    = astForBlock(doBlock, prefixAsts = prependBody)

    val code        = s"do {...} while (${condition.code})"
    val doBlockNode = controlStructureNode(doStmt, ControlStructureTypes.DO, code)

    val doAst =
      Ast(doBlockNode)
        .withChild(doBlockAst)
        .withChild(conditionAst)
        .withConditionEdges(doBlockNode, conditionAst.root.toList)

    Seq(doBlockAst.root.map(doAst.withDoBodyEdge(doBlockNode, _)).getOrElse(doAst))
  }

  private def astForForStatement(forStmt: DotNetNodeInfo): Seq[Ast] = {
    val initNode = forStmt.json.obj.get(ParserKeys.Declaration).collect { case declaration: ujson.Obj =>
      createDotNetNodeInfo(declaration)
    }
    val conditionNode = forStmt.json.obj.get(ParserKeys.Condition).collect { case condition: ujson.Obj =>
      createDotNetNodeInfo(condition)
    }
    val incrementorNodes = forStmt
      .json(ParserKeys.Incrementors)
      .arr
      .collect { case incrementor: ujson.Obj =>
        createDotNetNodeInfo(incrementor)
      }
      .toSeq

    val ConditionAstResult(conditionAst, prependBody) =
      conditionNode.map(astForConditionNode).getOrElse(ConditionAstResult(Ast(), List.empty))

    val forBodyAst = astForBlock(createDotNetNodeInfo(forStmt.json(ParserKeys.Statement)), prefixAsts = prependBody)

    val initCode        = initNode.map(_.code).getOrElse("")
    val conditionCode   = conditionNode.map(_.code).getOrElse("")
    val incrementorCode = incrementorNodes.map(_.code).mkString(", ")
    val code            = s"for ($initCode;$conditionCode;$incrementorCode)"
    val forNode         = controlStructureNode(forStmt, ControlStructureTypes.FOR, code)

    val initNodeAst    = initNode.toSeq.flatMap(astForNode)
    val incrementorAst = incrementorNodes.flatMap(astForNode)

    val _forAst = Ast(forNode)
      .withChildren(initNodeAst)
      .withChild(conditionAst)
      .withChildren(incrementorAst)
      .withChild(forBodyAst)
      .withConditionEdges(forNode, conditionAst.root.toList)

    val astWithInit =
      initNodeAst.flatMap(_.root).headOption.map(_forAst.withForInitEdge(forNode, _)).getOrElse(_forAst)
    val astWithUpdate =
      incrementorAst.flatMap(_.root).lastOption.map(astWithInit.withForUpdateEdge(forNode, _)).getOrElse(astWithInit)

    Seq(forBodyAst.root.map(astWithUpdate.withForBodyEdge(forNode, _)).getOrElse(astWithUpdate))
  }

  private def astForForEachStatement(forEachStmt: DotNetNodeInfo): Seq[Ast] = {
    val int32Tfn    = BuiltinTypes.DotNetTypeMap(BuiltinTypes.Int)
    val forEachNode = controlStructureNode(forEachStmt, ControlStructureTypes.FOR, forEachStmt.code)
    // Create the collection AST
    def newCollectionAst = astForNode(forEachStmt.json(ParserKeys.Expression))
    val collectionNode   = createDotNetNodeInfo(forEachStmt.json(ParserKeys.Expression))
    val collectionCode   = code(collectionNode)
    // Create the iterator variable
    val iterName    = forEachStmt.json(ParserKeys.Identifier)(ParserKeys.Value).str
    val iterNode    = forEachStmt.json(ParserKeys.Type)
    val iterNodeTfn = nodeTypeFullName(createDotNetNodeInfo(iterNode))
    val iterIdentifier =
      identifierNode(
        node = createDotNetNodeInfo(iterNode),
        name = iterName,
        code = iterName,
        typeFullName = iterNodeTfn
      )
    val iterVarLocal = NewLocal().name(iterName).code(iterName).typeFullName(iterNodeTfn)
    scope.addToScope(iterName, iterVarLocal)
    // Create a de-sugared `idx` variable, i.e., var _idx_ = 0
    val idxName         = "_idx_"
    val idxLocal        = NewLocal().name(idxName).code(idxName).typeFullName(int32Tfn)
    val idxIdenAtAssign = identifierNode(node = collectionNode, name = idxName, code = idxName, typeFullName = int32Tfn)
    val idxAssignment =
      callNode(forEachStmt, s"$idxName = 0", Operators.assignment, Operators.assignment, DispatchTypes.STATIC_DISPATCH)
    val idxAssigmentArgs =
      List(Ast(idxIdenAtAssign), Ast(NewLiteral().code("0").typeFullName(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Int))))
    val idxAssignmentAst = callAst(idxAssignment, idxAssigmentArgs)
    // Create condition based on `idx` variable, i.e., _idx_ < $collection.Count
    val idxIdAtCond = idxIdenAtAssign.copy
    val collectCountAccess = callNode(
      forEachStmt,
      s"$collectionCode.Count",
      Operators.fieldAccess,
      Operators.fieldAccess,
      DispatchTypes.STATIC_DISPATCH
    )
    val fieldAccessAst =
      callAst(collectCountAccess, newCollectionAst :+ Ast(NewFieldIdentifier().canonicalName("Count").code("Count")))
    val idxLt =
      callNode(
        forEachStmt,
        s"$idxName < $collectionCode.Count",
        Operators.lessThan,
        Operators.lessThan,
        DispatchTypes.STATIC_DISPATCH
      )
    val idxLtArgs =
      List(Ast(idxIdAtCond), fieldAccessAst)
    val ltCallCond = callAst(idxLt, idxLtArgs)
    // Create the assignment from $element = $collection[_idx_++]
    val idxIdAtCollAccess = idxIdenAtAssign.copy
    val collectIdxAccess = callNode(
      forEachStmt,
      s"$collectionCode[$idxName++]",
      Operators.indexAccess,
      Operators.indexAccess,
      DispatchTypes.STATIC_DISPATCH
    )
    val postIncrAst = callAst(
      callNode(
        forEachStmt,
        s"$idxName++",
        Operators.postIncrement,
        Operators.postIncrement,
        DispatchTypes.STATIC_DISPATCH
      ),
      Ast(idxIdAtCollAccess) :: Nil
    )
    val indexAccessAst = callAst(collectIdxAccess, newCollectionAst :+ postIncrAst)
    val iteratorAssignmentNode =
      callNode(
        forEachStmt,
        s"$iterName = $collectionCode[$idxName++]",
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH
      )
    val iteratorAssignmentArgs = List(Ast(iterIdentifier), indexAccessAst)
    val iteratorAssignmentAst  = callAst(iteratorAssignmentNode, iteratorAssignmentArgs)

    val forEachBlockAst = astForBlock(createDotNetNodeInfo(forEachStmt.json(ParserKeys.Statement)))

    val forEachAst = forAst(
      forNode = forEachNode,
      locals = Ast(idxLocal)
        .withRefEdge(idxIdenAtAssign, idxLocal)
        .withRefEdge(idxIdAtCond, idxLocal)
        .withRefEdge(idxIdAtCollAccess, idxLocal) :: Ast(iterVarLocal).withRefEdge(iterIdentifier, iterVarLocal) :: Nil,
      conditionAsts = ltCallCond :: Nil,
      initAsts = idxAssignmentAst :: Nil,
      updateAsts = iteratorAssignmentAst :: Nil,
      bodyAst = forEachBlockAst
    )

    val isAwaitForEach =
      Try(forEachStmt.json(ParserKeys.Await).bool).getOrElse(false) || forEachStmt.code.trim.startsWith(
        "await foreach "
      )
    (if (isAwaitForEach) forEachAst.withChild(Ast(modifierNode(forEachStmt, "AWAIT"))) else forEachAst) :: Nil
  }

  private def astForElseStatement(elseParserNode: DotNetNodeInfo): Ast = {
    val elseNode = controlStructureNode(elseParserNode, ControlStructureTypes.ELSE, "else")

    Option(elseParserNode.json(ParserKeys.Statement)) match {
      case Some(elseStmt: ujson.Value) if createDotNetNodeInfo(elseStmt).node == Block =>
        val blockAst: Ast = astForBlock(createDotNetNodeInfo(elseParserNode.json(ParserKeys.Statement)))
        Ast(elseNode).withChild(blockAst)
      case Some(elseStmt) =>
        astForNode(createDotNetNodeInfo(elseParserNode.json(ParserKeys.Statement))).headOption.getOrElse(Ast())
      case None => Ast()
    }
  }

  private def astForGlobalStatement(globalStatement: DotNetNodeInfo): Seq[Ast] = {
    val stmtNodeInfo = createDotNetNodeInfo(globalStatement.json(ParserKeys.Statement))
    stmtNodeInfo.node match {
      // Denotes a top-level method declaration. These shall be added to the fictitious "main" created
      // by `astForTopLevelStatements`.
      case LocalFunctionStatement =>
        astForMethodDeclaration(stmtNodeInfo, extraModifiers = modifierNode(stmtNodeInfo, ModifierTypes.STATIC) :: Nil)
      case _ => astForNode(stmtNodeInfo)
    }
  }

  private def astForLabeledStatement(labeledStmt: DotNetNodeInfo): Seq[Ast] = {
    val labelName = labeledStmt.json(ParserKeys.Identifier).obj(ParserKeys.Value).str
    val labelAst  = Ast(jumpTargetNode(labeledStmt, labelName, s"$labelName:"))
    val statementAsts = Try(labeledStmt.json(ParserKeys.Statement)).toOption match {
      case Some(statement: ujson.Obj) => astForNode(createDotNetNodeInfo(statement))
      case _                          => Seq.empty
    }
    labelAst +: statementAsts
  }

  private def astForJumpStatement(jumpStmt: DotNetNodeInfo): Seq[Ast] = {
    jumpStmt.node match {
      case BreakStatement    => Seq(Ast(controlStructureNode(jumpStmt, ControlStructureTypes.BREAK, jumpStmt.code)))
      case ContinueStatement => Seq(Ast(controlStructureNode(jumpStmt, ControlStructureTypes.CONTINUE, jumpStmt.code)))
      case GotoStatement     => astForGotoStatement(jumpStmt)
      case ReturnStatement   => astForReturnStatement(jumpStmt)
      case YieldStatement    => astForYieldStatement(jumpStmt)
      case _                 => Seq.empty
    }
  }

  private def astForGotoStatement(gotoStmt: DotNetNodeInfo): Seq[Ast] = {
    val gotoNode = controlStructureNode(gotoStmt, ControlStructureTypes.GOTO, gotoStmt.code)
    val labelAst = Option(gotoStmt.json(ParserKeys.Expression)) match {
      case Some(value: ujson.Obj) =>
        val expressionNode = createDotNetNodeInfo(value)
        val labelName      = nameFromNode(expressionNode)
        Some(
          Ast(
            NewJumpLabel()
              .parserTypeName(expressionNode.node.toString)
              .name(labelName)
              .code(labelName)
              .lineNumber(expressionNode.lineNumber)
              .columnNumber(expressionNode.columnNumber)
              .order(1)
          )
        )
      case _ => None
    }

    val gotoAst = Ast(gotoNode).withChildren(labelAst.toSeq)
    Seq(labelAst.flatMap(_.root).map(gotoAst.withJumpArgumentEdge(gotoNode, _)).getOrElse(gotoAst))
  }

  private def astForReturnStatement(returnStmt: DotNetNodeInfo): Seq[Ast] = {
    val identifierAst = Option(returnStmt.json(ParserKeys.Expression)) match {
      case Some(value: ujson.Obj) => astForNode(createDotNetNodeInfo(value))
      case _                      => Seq.empty
    }
    val _returnNode = returnNode(returnStmt, returnStmt.code)
    Seq(returnAst(_returnNode, identifierAst))
  }

  private def astForYieldStatement(yieldStmt: DotNetNodeInfo): Seq[Ast] = {
    val valueAst = Option(yieldStmt.json(ParserKeys.Expression)) match {
      case Some(value: ujson.Obj) => astForNode(createDotNetNodeInfo(value))
      case _                      => Seq.empty
    }
    val yieldNode = returnNode(yieldStmt, yieldStmt.code)
    Seq(returnAst(yieldNode, valueAst))
  }

  protected def astForThrowStatement(throwStmt: DotNetNodeInfo): Seq[Ast] = {
    val argsAst = Try(throwStmt.json(ParserKeys.Expression)).toOption match {
      case Some(_expr: ujson.Obj) => astForNode(createDotNetNodeInfo(_expr))
      case _                      => Seq.empty[Ast]
    }
    val throwCall = operatorCallNode(throwStmt, CSharpOperators.throws, Some(getTypeFullNameFromAstNode(argsAst)))
    Seq(callAst(throwCall, argsAst))
  }

  protected def astForTryStatement(tryStmt: DotNetNodeInfo): Seq[Ast] = {
    val tryNode          = controlStructureNode(tryStmt, ControlStructureTypes.TRY, code(tryStmt))
    val tryBlockNodeInfo = createDotNetNodeInfo(tryStmt.json(ParserKeys.Block))
    val tryAst           = astForBlock(tryBlockNodeInfo, Option(code(tryBlockNodeInfo)))

    val catchAsts = Try(tryStmt.json(ParserKeys.Catches))
      .map(_.arr.toSeq)
      .map { c =>
        c.map { value =>
          val nodeInfo  = createDotNetNodeInfo(value)
          val catchNode = controlStructureNode(nodeInfo, ControlStructureTypes.CATCH, code(nodeInfo))
          val children  = astForNode(nodeInfo)
          Ast(catchNode).withChildren(children)
        }
      }
      .getOrElse(Seq.empty)

    val finallyAst = Try(createDotNetNodeInfo(tryStmt.json(ParserKeys.Finally))).toOption.map { finallyNodeInfo =>
      val finallyNode      = controlStructureNode(finallyNodeInfo, ControlStructureTypes.FINALLY, code(finallyNodeInfo))
      val finallyClauseAst = astForFinallyClause(finallyNodeInfo)
      Ast(finallyNode).withChildren(finallyClauseAst)
    }

    val controlStructureAst = tryCatchAst(tryNode, tryAst, catchAsts, finallyAst)
    Seq(controlStructureAst)
  }

  protected def astForFinallyClause(finallyClause: DotNetNodeInfo): Seq[Ast] = {
    Seq(astForBlock(createDotNetNodeInfo(finallyClause.json(ParserKeys.Block)), code = Option(code(finallyClause))))
  }

  /** Variables using the <a
    * href="https://learn.microsoft.com/en-us/dotnet/api/system.idisposable?view=net-8.0">IDisposable</a> interface may
    * be used in `using`, where a call to `Dispose` is guaranteed.
    *
    * Thus, this is lowered as a try-finally, with finally making a call to `Dispose` on the declared variable.
    */
  private def astForUsingStatement(usingStmt: DotNetNodeInfo): Seq[Ast] = {
    val tryNode = controlStructureNode(usingStmt, ControlStructureTypes.TRY, code(usingStmt))
    val declAst =
      Try(createDotNetNodeInfo(usingStmt.json(ParserKeys.Declaration))).map(astForNode).getOrElse(scala.Seq.empty[Ast])
    val tryNodeInfo = createDotNetNodeInfo(usingStmt.json(ParserKeys.Statement))
    val tryAst      = astForBlock(tryNodeInfo, Option("try"))

    val finallyAst = finallyAstForUsingDisposals(
      usingStmt,
      declAst,
      isAwaitUsing = Try(usingStmt.json(ParserKeys.Await).bool).getOrElse(false)
    )

    declAst :+ tryCatchAst(tryNode, tryAst, Seq.empty, finallyAst)
  }

  protected def astForCatchClause(catchClause: DotNetNodeInfo): Seq[Ast] = {
    val declAst = astForNode(catchClause.json(ParserKeys.Declaration)).toList
    val filterAst = Try(catchClause.json(ParserKeys.Filter)).toOption
      .filterNot(_.isNull)
      .map(createDotNetNodeInfo)
      .flatMap(astForCatchFilterClause(_).headOption)
      .toList
    val blockAst = astForBlock(
      createDotNetNodeInfo(catchClause.json(ParserKeys.Block)),
      code = Option(code(catchClause)),
      prefixAsts = declAst ++ filterAst
    )
    Seq(blockAst)
  }

  protected def astForCatchFilterClause(catchFilterClause: DotNetNodeInfo): Seq[Ast] = {
    astForNode(catchFilterClause.json(ParserKeys.Condition))
  }

  protected def astForCatchDeclaration(catchDeclaration: DotNetNodeInfo): Seq[Ast] = {
    Try(catchDeclaration.json(ParserKeys.Identifier)).toOption
      .filterNot(_.isNull)
      .flatMap(identifier => Try(identifier.obj(ParserKeys.Value).str).toOption)
      .filter(_.nonEmpty)
      .map { name =>
        val typeFullName = Try(catchDeclaration.json(ParserKeys.Type)).toOption
          .collect { case typeJson: ujson.Obj => nodeTypeFullName(createDotNetNodeInfo(typeJson)) }
          .getOrElse(Defines.Any)
        val local = localNode(catchDeclaration, name, name, typeFullName)
        scope.addToScope(name, local)
        Ast(local)
      }
      .toSeq
  }

}
