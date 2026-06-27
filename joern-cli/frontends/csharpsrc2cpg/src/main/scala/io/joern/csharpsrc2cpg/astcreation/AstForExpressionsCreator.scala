package io.joern.csharpsrc2cpg.astcreation

import io.joern.csharpsrc2cpg.astcreation.BuiltinTypes.DotNetTypeMap
import io.joern.csharpsrc2cpg.datastructures.{CSharpMethod, FieldDecl}
import io.joern.csharpsrc2cpg.parser.DotNetJsonAst.*
import io.joern.csharpsrc2cpg.parser.{DotNetNodeInfo, ParserKeys}
import io.joern.csharpsrc2cpg.utils.Utils.{composeMethodFullName, composeMethodLikeSignature}
import io.joern.csharpsrc2cpg.{CSharpOperators, Constants}
import io.joern.x2cpg.utils.AstPropertiesUtil.{RootProperties, RootPropertiesOnSeq}
import io.joern.x2cpg.{Ast, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.{NewLiteral, NewTypeRef}
import io.shiftleft.codepropertygraph.generated.{DispatchTypes, Operators}
import ujson.Value

import scala.collection.mutable.ArrayBuffer
import scala.util.Try
trait AstForExpressionsCreator(implicit withSchemaValidation: ValidationMode) { this: AstCreator =>

  def astForExpressionStatement(expr: DotNetNodeInfo): Seq[Ast] = {
    val expressionNode = createDotNetNodeInfo(expr.json(ParserKeys.Expression))
    astForExpression(expressionNode)
  }

  def astForExpression(expr: DotNetNodeInfo): Seq[Ast] = {
    expr.node match {
      case _: UnaryExpr                      => astForUnaryExpression(expr)
      case _: BinaryExpr                     => astForBinaryExpression(expr)
      case _: LiteralExpr                    => astForLiteralExpression(expr)
      case InvocationExpression              => astForInvocationExpression(expr)
      case AwaitExpression                   => astForAwaitExpression(expr)
      case ObjectCreationExpression          => astForObjectCreationExpression(expr)
      case WithExpression                    => astForWithExpression(expr)
      case SimpleMemberAccessExpression      => astForSimpleMemberAccessExpression(expr)
      case ElementAccessExpression           => astForElementAccessExpression(expr)
      case ImplicitArrayCreationExpression   => astForImplicitArrayCreationExpression(expr)
      case QueryExpression                   => astForQueryExpression(expr)
      case StackAllocExpression              => astForStackAllocExpression(expr)
      case ConditionalExpression             => astForConditionalExpression(expr)
      case SwitchExpression                  => astForSwitchExpression(expr)
      case TupleExpression                   => astForTupleExpression(expr)
      case NameOfExpression                  => astForNameOfExpression(expr)
      case _: IdentifierNode                 => astForIdentifier(expr) :: Nil
      case ThisExpression                    => astForThisReceiver(expr) :: Nil
      case BaseExpression                    => astForBaseReceiver(expr) :: Nil
      case CastExpression                    => astForCastExpression(expr)
      case AsExpression                      => astForAsExpression(expr)
      case IsExpression                      => astForIsExpression(expr)
      case TypeOfExpression                  => astForTypeOfExpression(expr)
      case SizeOfExpression                  => astForSizeOfExpression(expr)
      case DefaultExpression                 => astForDefaultExpression(expr)
      case ThrowExpression                   => astForThrowExpression(expr)
      case RefExpression                     => astForRefExpression(expr)
      case MakeRefExpression                 => astForMakeRefExpression(expr)
      case RefTypeExpression                 => astForRefTypeExpression(expr)
      case RefValueExpression                => astForRefValueExpression(expr)
      case SpreadElement                     => astForSpreadElement(expr)
      case CheckedExpression                 => astForCheckedExpression(expr)
      case InterpolatedStringExpression      => astForInterpolatedStringExpression(expr)
      case ConditionalAccessExpression       => astForConditionalAccessExpression(expr)
      case SuppressNullableWarningExpression => astForSuppressNullableWarningExpression(expr)
      case _: BaseLambdaExpression           => astForSimpleLambdaExpression(expr)
      case ParenthesizedExpression           => astForParenthesizedExpression(expr)
      case PredefinedType | SimpleBaseType | PrimaryConstructorBaseType | PointerType | FunctionPointerType | RefType |
          ScopedType | TupleType | TupleElement =>
        Ast(identifierNode(expr, expr.code, expr.code, nodeTypeFullName(expr))) :: Nil
      case _ => notHandledYet(expr)
    }
  }

  /** Attempts to decide if [[expr]] denotes a setter property reference, in which case returns its corresponding
    * [[CSharpMethod]] meta-data and class full name it belongs to.
    */
  private def tryResolveSetterInvocation(expr: DotNetNodeInfo): Option[(CSharpMethod, String)] = {
    val baseType = expr.node match {
      case SimpleMemberAccessExpression =>
        val base = createDotNetNodeInfo(expr.json(ParserKeys.Expression))
        Some(nodeTypeFullName(base))
      case IdentifierName =>
        scope.surroundingTypeDeclFullName
      case _ =>
        None
    }

    val fieldName = nameFromNode(expr)
    baseType.flatMap(x => scope.tryResolveSetterInvocation(fieldName, Some(x)).map((_, x)))
  }

  private def stripAssignmentFromOperator(operatorName: String): Option[String] = operatorName match {
    case Operators.assignmentPlus                 => Some(Operators.plus)
    case Operators.assignmentMinus                => Some(Operators.minus)
    case Operators.assignmentMultiplication       => Some(Operators.multiplication)
    case Operators.assignmentDivision             => Some(Operators.division)
    case Operators.assignmentExponentiation       => Some(Operators.exponentiation)
    case Operators.assignmentModulo               => Some(Operators.modulo)
    case Operators.assignmentShiftLeft            => Some(Operators.shiftLeft)
    case Operators.assignmentLogicalShiftRight    => Some(Operators.logicalShiftRight)
    case Operators.assignmentArithmeticShiftRight => Some(Operators.arithmeticShiftRight)
    case Operators.assignmentAnd                  => Some(Operators.and)
    case Operators.assignmentOr                   => Some(Operators.or)
    case Operators.assignmentXor                  => Some(Operators.xor)
    case _                                        => None
  }

  /** Mainly to abstract the lowering of +=, *=, etc. assignments when the LHS is a property. Takes care of building the
    * RHS appropriately, e.g. by expanding `P += RHS` into `set_P(get_P() + RHS)`, etc.
    * @param expr
    *   the full assignment expression, for `code`, `line`, etc.
    * @param assignOp
    *   the assignment operator, cf. [[Operators]]
    * @param setterInfo
    *   the setter meta-data, cf. [[tryResolveSetterInvocation]]
    */
  private def lowerSetterAssignmentRhs(
    expr: DotNetNodeInfo,
    assignOp: String,
    setterInfo: (CSharpMethod, String),
    receiver: Option[Ast],
    rhs: DotNetNodeInfo
  ): Seq[Ast] = {
    val (setterMethod, setterBaseType) = setterInfo
    val propertyName                   = setterMethod.name.stripPrefix("set_")
    val originalRhs                    = astForOperand(rhs)

    assignOp match {
      case Operators.assignment => originalRhs
      case _ =>
        scope.tryResolveGetterInvocation(propertyName, Some(setterBaseType)) match {
          // Shouldn't happen, provided it is valid code. At any rate, log and emit the RHS verbatim.
          case None =>
            logger.warn(s"Couldn't find matching getter for $propertyName in ${code(expr)}")
            originalRhs
          case Some(getterMethod) =>
            stripAssignmentFromOperator(assignOp) match {
              case None =>
                logger.warn(s"Unrecognized assignment in ${code(expr)}")
                originalRhs
              case Some(opName) =>
                val getterInvocation = {
                  createInvocationAst(expr, getterMethod.name, Nil, receiver, Some(getterMethod), Some(setterBaseType))
                }
                val operatorCall =
                  operatorCallNode(expr, code(expr), opName, Some(setterMethod.returnType))
                callAst(operatorCall, getterInvocation +: originalRhs, None, None) :: Nil
            }
        }
    }
  }

  /** Lowers assignments such as `x.P = RHS` and `x.P += RHS` with `P` denoting a setter property into calls
    * `x.set_P(RHS)` and `x.set_P(x.get_P() + RHS)`.
    * @param assignExpr
    *   the full assignment expression, for `code`, `line` properties
    * @param assignOp
    *   the final assignment operator name, cf. [[Operators]]
    * @param setterInfo
    *   the setter meta-data, cf. [[tryResolveSetterInvocation]]
    */
  private def astForMemberAccessSetterAssignment(
    assignExpr: DotNetNodeInfo,
    lhs: DotNetNodeInfo,
    assignOp: String,
    rhs: DotNetNodeInfo,
    setterInfo: (CSharpMethod, String)
  ): Seq[Ast] = {

    val (setterMethod, setterBaseType) = setterInfo

    def createReceiver(): Option[Ast] = {
      if (setterMethod.isStatic) {
        None
      } else {
        val baseNode = createDotNetNodeInfo(lhs.json(ParserKeys.Expression))
        astForNode(baseNode).headOption
      }
    }

    val receiver      = createReceiver()
    val rhsAst        = lowerSetterAssignmentRhs(assignExpr, assignOp, setterInfo, receiver, rhs)
    val receiverClone = createReceiver()

    createInvocationAst(
      assignExpr,
      setterMethod.name,
      rhsAst,
      receiverClone,
      Some(setterMethod),
      Some(setterBaseType)
    ) :: Nil
  }

  /** Lowers assignments such as `P = RHS` and `P += RHS` with `P` an identifier denoting a setter property into calls
    * `set_P(RHS)` and `set_P(get_P() + RHS)`, respectively.
    * @param assignExpr
    *   the full assignment expression, for `code`, `line` properties.
    * @param assignOp
    *   the final assignment operator name, cf. [[Operators]]
    * @param setterInfo
    *   the setter meta-data, cf. [[tryResolveSetterInvocation]]
    */
  private def astForIdentifierSetterAssignment(
    assignExpr: DotNetNodeInfo,
    lhs: DotNetNodeInfo,
    assignOp: String,
    rhs: DotNetNodeInfo,
    setterInfo: (CSharpMethod, String)
  ): Seq[Ast] = {
    val (setterMethod, setterBaseType) = setterInfo
    val receiver      = Option.when(!setterMethod.isStatic)(astForThisReceiver(lhs, scope.surroundingTypeDeclFullName))
    val receiverClone = Option.when(!setterMethod.isStatic)(astForThisReceiver(lhs, scope.surroundingTypeDeclFullName))
    val rhsAst        = lowerSetterAssignmentRhs(assignExpr, assignOp, setterInfo, receiver, rhs)

    createInvocationAst(
      assignExpr,
      setterMethod.name,
      rhsAst,
      receiverClone,
      Some(setterMethod),
      Some(setterBaseType)
    ) :: Nil
  }

  /** Lowers assignments such as `x.P = RHS` and `P += RHS` where `P` denotes a setter property into a call
    * `x.set_P(RHS)` and `set_P(get_P() + RHS)`, respectively.
    *
    * @param assignExpr
    *   the full assignment expr, for `code`, `line` properties
    * @param setterInfo
    *   the setter meta-data, cf. [[tryResolveSetterInvocation]]
    * @param assignOp
    *   the final assignment operator name, cf. [[Operators]]
    */
  private def astForSetterAssignmentExpression(
    assignExpr: DotNetNodeInfo,
    setterInfo: (CSharpMethod, String),
    lhs: DotNetNodeInfo,
    assignOp: String,
    rhs: DotNetNodeInfo
  ): Seq[Ast] = {
    lhs.node match {
      case SimpleMemberAccessExpression =>
        astForMemberAccessSetterAssignment(assignExpr, lhs, assignOp, rhs, setterInfo)
      case IdentifierName => astForIdentifierSetterAssignment(assignExpr, lhs, assignOp, rhs, setterInfo)
      case _ =>
        logger.warn(s"Unsupported setter assignment: ${code(assignExpr)}")
        Nil
    }
  }

  /** Lowers arbitrary assignment `LHS = RHS`, `LHS += RHS`, etc. expressions.
    * @param assignExpr
    *   the full assignment, for `code`, `line` properties
    * @param assignOp
    *   the final assignment operator name, cf. [[Operators]]
    */
  private def astForAssignmentExpression(
    assignExpr: DotNetNodeInfo,
    lhs: DotNetNodeInfo,
    assignOp: String,
    rhs: DotNetNodeInfo
  ): Seq[Ast] = {
    tryResolveSetterInvocation(lhs) match {
      case Some(setterInfo) => astForSetterAssignmentExpression(assignExpr, setterInfo, lhs, assignOp, rhs)
      case None             => astForRegularAssignmentExpression(assignExpr, lhs, assignOp, rhs)
    }
  }

  private def astForRegularAssignmentExpression(
    assignExpr: DotNetNodeInfo,
    lhs: DotNetNodeInfo,
    assignOp: String,
    rhs: DotNetNodeInfo
  ): Seq[Ast] = {
    astForRegularBinaryExpression(assignExpr, lhs, assignOp, rhs)
  }

  private def astForParenthesizedExpression(parenExpr: DotNetNodeInfo): Seq[Ast] = {
    astForNode(parenExpr.json(ParserKeys.Expression))
  }

  private def astForCheckedExpression(checkedExpr: DotNetNodeInfo): Seq[Ast] = {
    astForNode(checkedExpr.json(ParserKeys.Expression))
  }

  private def astForAwaitExpression(awaitExpr: DotNetNodeInfo): Seq[Ast] = {
    /* fullName is the name in case of STATIC_DISPATCH */
    val node =
      callNode(awaitExpr, awaitExpr.code, CSharpOperators.await, CSharpOperators.await, DispatchTypes.STATIC_DISPATCH)
    val argAsts = astForNode(awaitExpr.json(ParserKeys.Expression))
    Seq(callAst(node, argAsts))
  }

  protected def astForExpressionElement(expressionElement: DotNetNodeInfo): Seq[Ast] = {
    astForNode(expressionElement.json(ParserKeys.Expression))
  }

  protected def astForLiteralExpression(_literalNode: DotNetNodeInfo): Seq[Ast] = {
    Seq(Ast(literalNode(_literalNode, code(_literalNode), nodeTypeFullName(_literalNode))))
  }

  protected def astForOperand(operandNode: DotNetNodeInfo): Seq[Ast] = {
    operandNode.node match {
      case IdentifierName =>
        (scope.findFieldInScope(nameFromNode(operandNode)), scope.lookupVariable(nameFromNode(operandNode))) match {
          case (Some(field), None) => createImplicitBaseFieldAccess(operandNode, field)
          case _                   => astForNode(operandNode)
        }
      case _ => astForNode(operandNode)
    }
  }

  private def createImplicitBaseFieldAccess(fieldNode: DotNetNodeInfo, field: FieldDecl): Seq[Ast] = {
    // TODO: Maybe this should be a TypeRef, like we recently started doing for javasrc?
    val baseNode = if (field.isStatic) {
      val name = scope.surroundingTypeDeclFullName.getOrElse(Defines.Any)
      identifierNode(fieldNode, name, name, field.typeFullName)
    } else {
      identifierNode(fieldNode, Constants.This, Constants.This, field.typeFullName)
    }

    fieldAccessAst(
      fieldNode,
      fieldNode,
      base = Ast(baseNode),
      code = s"${baseNode.code}.${field.name}",
      fieldName = field.name,
      fieldTypeFullName = field.typeFullName
    ) :: Nil
  }

  protected def astForUnaryExpression(unaryExpr: DotNetNodeInfo): Seq[Ast] = {
    val operatorToken = unaryExpr.json(ParserKeys.OperatorToken)(ParserKeys.Value).str
    val operatorName = operatorToken match {
      case "+" => Operators.plus
      case "-" => Operators.minus
      case "++" =>
        if (unaryExpr.node.getClass == PostIncrementExpression.getClass) Operators.postIncrement
        else Operators.preIncrement
      case "--" =>
        if (unaryExpr.node.getClass == PostDecrementExpression.getClass) Operators.postDecrement
        else Operators.preDecrement
      case "~" => Operators.not
      case "!" => Operators.logicalNot
      case "&" => Operators.addressOf
      case "*" => Operators.indirection
      case "^" => CSharpOperators.indexFromEnd
    }

    val args     = createDotNetNodeInfo(unaryExpr.json(ParserKeys.Operand))
    val argsAst  = astForOperand(args)
    val callNode = operatorCallNode(unaryExpr, code(unaryExpr), operatorName, Some(nodeTypeFullName(args)))

    callAst(callNode, argsAst) :: Nil
  }

  private def astForBinaryExpression(binaryExpr: DotNetNodeInfo): Seq[Ast] = {
    val lhsNode       = createDotNetNodeInfo(binaryExpr.json(ParserKeys.Left))
    val rhsNode       = createDotNetNodeInfo(binaryExpr.json(ParserKeys.Right))
    val operatorToken = binaryExpr.json(ParserKeys.OperatorToken)(ParserKeys.Value).str
    val operatorName = binaryOperatorsMap.getOrElse(
      operatorToken, {
        logger.warn(s"Unhandled operator '$operatorToken' for ${code(binaryExpr)}")
        CSharpOperators.unknown
      }
    )
    binaryExpr.node match {
      case _: AssignmentExpr => astForAssignmentExpression(binaryExpr, lhsNode, operatorName, rhsNode)
      case _                 => astForRegularBinaryExpression(binaryExpr, lhsNode, operatorName, rhsNode)
    }
  }

  /** @param binaryExpr
    *   the full binary expression, for `code`, `line`, etc.
    * @param operatorName
    *   the final operator name, cf. [[Operators]]
    */
  private def astForRegularBinaryExpression(
    binaryExpr: DotNetNodeInfo,
    lhs: DotNetNodeInfo,
    operatorName: String,
    rhs: DotNetNodeInfo
  ): Seq[Ast] = {
    val args         = astForOperand(lhs) ++: astForOperand(rhs)
    val typeFullName = fixedTypeOperators.get(operatorName).orElse(Some(getTypeFullNameFromAstNode(args)))
    val callNode     = operatorCallNode(binaryExpr, code(binaryExpr), operatorName, typeFullName)
    callAst(callNode, args) :: Nil
  }

  /** Handles the `= ...` part of the equals value clause, thus this only contains an RHS.
    */
  protected def astForEqualsValueClause(clause: DotNetNodeInfo): Seq[Ast] = {
    val rhsNode = createDotNetNodeInfo(clause.json(ParserKeys.Value))
    astForNode(rhsNode)
  }

  protected def astForArrayInitializerExpression(arrayInitializerExpression: DotNetNodeInfo): Seq[Ast] = {
    astForCollectionStaticInitializer(arrayInitializerExpression, ParserKeys.Expressions)
  }

  protected def astForCollectionExpression(collectionExpression: DotNetNodeInfo): Seq[Ast] = {
    astForCollectionStaticInitializer(collectionExpression, ParserKeys.Elements)
  }

  private def astForTupleExpression(tupleExpr: DotNetNodeInfo): Seq[Ast] = {
    val arguments = tupleExpr
      .json(ParserKeys.Arguments)
      .arr
      .map(createDotNetNodeInfo)
      .flatMap { arg =>
        val argExpression = createDotNetNodeInfo(arg.json(ParserKeys.Expression))
        astForExpression(argExpression)
      }
      .toSeq
    val callNode = operatorCallNode(tupleExpr, code(tupleExpr), CSharpOperators.tuple, Some(Defines.Any))
    callAst(callNode, arguments) :: Nil
  }

  private def astForCollectionStaticInitializer(
    arrayInitializerExpression: DotNetNodeInfo,
    elementParserKey: String
  ): Seq[Ast] = {
    val MAX_INITIALIZERS = 1000

    val elements = arrayInitializerExpression.json(elementParserKey).arr

    val nestedExpressionsExists =
      Try(elements.map(createDotNetNodeInfo).map(_.json(elementParserKey))).getOrElse(ArrayBuffer.empty).nonEmpty

    // We have more expressions in our expressions, which means we have a 2+D array, parse these
    val args: Seq[Ast] =
      if (nestedExpressionsExists)
        elements.map(createDotNetNodeInfo).flatMap(astForCollectionStaticInitializer(_, elementParserKey)).toSeq
      else elements.slice(0, MAX_INITIALIZERS).map(createDotNetNodeInfo).flatMap(astForNode).toSeq

    val typeFullName = elementParserKey match {
      case ParserKeys.Expressions => s"${getTypeFullNameFromAstNode(args)}[]"
      case ParserKeys.Elements    => "System.List"
    }

    val callNode = operatorCallNode(
      arrayInitializerExpression,
      arrayInitializerExpression.json(ParserKeys.MetaData)(ParserKeys.Code).str,
      Operators.arrayInitializer,
      typeFullName = Some(typeFullName)
    )

    val ast = callAst(callNode, args)

    // TODO: This will work as expected for 1D collections, but is going to require some thinking for 2+D arrays since we
    //  will have to keep track of the number of elements in each sub-array
    if (elements.size > MAX_INITIALIZERS) {
      val placeholder = NewLiteral()
        .typeFullName(Defines.Any)
        .code("<too-many-initializers>")
        .lineNumber(arrayInitializerExpression.lineNumber)
        .columnNumber(arrayInitializerExpression.columnNumber)

      Seq(ast.withChild(Ast(placeholder)).withArgEdge(callNode, placeholder))
    } else {
      Seq(ast)
    }
  }

  private def createInvocationAst(
    invocationExpr: DotNetNodeInfo,
    callName: String,
    arguments: Seq[Ast],
    baseAst: Option[Ast],
    methodMetaData: Option[CSharpMethod],
    baseTypeFullName: Option[String]
  ): Ast = {
    val methodSignature = methodMetaData match {
      case Some(m) =>
        val returnType = DotNetTypeMap.getOrElse(m.returnType, m.returnType)
        composeMethodLikeSignature(returnType, m.parameterTypes.filterNot(_._1 == Constants.This).map(_._2))
      case None => Defines.UnresolvedSignature
    }

    val methodFullName = methodMetaData
      .flatMap(_.fullName)
      .filter(_.contains("<"))
      .getOrElse {
        baseTypeFullName match {
          case Some(typeFullName) => composeMethodFullName(typeFullName, callName, methodSignature)
          case _                  => composeMethodFullName(Defines.UnresolvedNamespace, callName, methodSignature)
        }
      }
    val dispatchType = methodMetaData
      .map(_.isStatic)
      .map {
        case true  => DispatchTypes.STATIC_DISPATCH
        case false => DispatchTypes.DYNAMIC_DISPATCH
      }
      .getOrElse(DispatchTypes.STATIC_DISPATCH)

    val _callAst = callAst(
      callNode(
        invocationExpr,
        code(invocationExpr),
        callName,
        methodFullName,
        dispatchType,
        Option(methodSignature),
        methodMetaData.map(_.returnType)
      ),
      arguments,
      baseAst
    )

    _callAst
  }

  /** Handles expressions like `foo.Bar()`. If `Bar` can't be found inside `foo`'s class, attempts to find a compatible
    * extension method. If all fails, an AST is still produced.
    */
  private def astForMemberAccessInvocation(
    invocationExpr: DotNetNodeInfo,
    baseAst: Option[Ast],
    argumentList: DotNetNodeInfo,
    callName: String
  ): Seq[Ast] = {

    val baseTypeFullName = Some(getTypeFullNameFromAstNode(baseAst.toList)).filterNot(_ == Defines.Any)
    val arguments        = astForArgumentList(argumentList, baseTypeFullName)
    val argTypes         = arguments.map(getTypeFullNameFromAstNode).toList

    val byMethod         = scope.tryResolveMethodInvocation(callName, argTypes, baseTypeFullName)
    lazy val byExtMethod = scope.tryResolveExtensionMethodInvocation(baseTypeFullName, callName, argTypes)

    val (method, baseType) = byMethod
      .map(x => (Some(x), baseTypeFullName))
      .orElse(byExtMethod.map(x => (Some(x._1), Some(x._2))))
      .getOrElse((None, baseTypeFullName))

    createInvocationAst(invocationExpr, callName, arguments, baseAst, method, baseType) :: Nil
  }

  private def astForIdentifierInvocation(
    invocationExpr: DotNetNodeInfo,
    argumentList: DotNetNodeInfo,
    callName: String
  ): Seq[Ast] = {
    // This is when a call is made directly, which could also be made from a static import
    val argTypes = astForArgumentList(argumentList).map(getTypeFullNameFromAstNode).toList
    val (receiver, baseType, method, args) = scope
      .tryResolveMethodInvocation(callName, argTypes)
      .orElse(scope.tryResolveMethodInvocation(callName, argTypes, scope.surroundingTypeDeclFullName)) match {
      case Some(methodMetaData) if methodMetaData.isStatic =>
        // If static, create implicit type identifier explicitly
        val typeMetaData = scope.typeForMethod(methodMetaData)
        val typeName     = typeMetaData.flatMap(_.name.split("[.]").lastOption).getOrElse(Defines.Any)
        val typeFullName = typeMetaData.map(_.name)
        val receiverNode = Ast(identifierNode(invocationExpr, typeName, typeName, typeFullName.getOrElse(Defines.Any)))
        val arguments    = astForArgumentList(argumentList, typeFullName)
        (Option(receiverNode), typeFullName, Option(methodMetaData), arguments)
      case Some(methodMetaData) =>
        // If dynamic, create implicit `this` identifier explicitly
        val typeMetaData = scope.typeForMethod(methodMetaData)
        val typeFullName = typeMetaData.map(_.name)
        val thisAst      = astForThisReceiver(invocationExpr, typeFullName)
        val arguments    = astForArgumentList(argumentList, typeFullName)
        (Option(thisAst), typeMetaData.map(_.name), Option(methodMetaData), arguments)
      case None =>
        (None, None, None, Seq.empty[Ast])
    }

    createInvocationAst(invocationExpr, callName, args, receiver, method, baseType) :: Nil
  }

  private def astForInvocationExpression(invocationExpr: DotNetNodeInfo): Seq[Ast] = {
    val expression   = createDotNetNodeInfo(invocationExpr.json(ParserKeys.Expression))
    val callName     = nameFromNode(expression)
    val argumentList = createDotNetNodeInfo(invocationExpr.json(ParserKeys.ArgumentList))

    expression.node match {
      case SimpleMemberAccessExpression | SuppressNullableWarningExpression =>
        val baseAst = astForNode(createDotNetNodeInfo(expression.json(ParserKeys.Expression)))
        astForMemberAccessInvocation(invocationExpr, baseAst.headOption, argumentList, callName)
      case IdentifierName | MemberBindingExpression =>
        astForIdentifierInvocation(invocationExpr, argumentList, callName)
      case ConditionalAccessExpression =>
        astForConditionalAccessExpression(
          moveConditionalAccessInvocationToMemberBinding(invocationExpr, expression, argumentList)
        )
      case x =>
        logger.warn(s"Unhandled LHS $x for InvocationExpression")
        Nil
    }
  }

  /** Handles expressions like `foo.MyField`, where `MyField` is known to be a getter property. Getters are lowered into
    * calls, e.g. (a) System.Console.Out becomes System.Console.get_Out(), because it's a static method; (b) x.KeyChar
    * becomes System.ConsoleKeyInfo.get_KeyChar(x), because it's a dynamic method.
    */
  private def astForMemberAccessGetterExpression(
    getter: CSharpMethod,
    baseAst: Ast,
    baseTypeFullName: String,
    accessExpr: DotNetNodeInfo
  ): Seq[Ast] = {
    if (getter.isStatic) {
      val signature      = composeMethodLikeSignature(getter.returnType, Nil)
      val methodFullName = composeMethodFullName(baseTypeFullName, getter.name, signature)
      callAst(
        callNode(
          accessExpr,
          code(accessExpr),
          getter.name,
          methodFullName,
          DispatchTypes.STATIC_DISPATCH,
          Option(signature),
          Option(getter.returnType)
        )
      ) :: Nil
    } else {
      val signature      = composeMethodLikeSignature(getter.returnType, baseTypeFullName :: Nil)
      val methodFullName = composeMethodFullName(baseTypeFullName, getter.name, signature)
      callAst(
        callNode(
          accessExpr,
          code(accessExpr),
          getter.name,
          methodFullName,
          DispatchTypes.DYNAMIC_DISPATCH,
          Option(signature),
          Option(getter.returnType)
        ),
        base = Some(baseAst)
      ) :: Nil
    }
  }

  private def astForSimpleMemberAccessExpression(accessExpr: DotNetNodeInfo): Seq[Ast] = {
    val fieldIdentifierName = nameFromNode(accessExpr)
    val baseAst             = astForNode(createDotNetNodeInfo(accessExpr.json(ParserKeys.Expression))).head
    val baseTypeFullName    = getTypeFullNameFromAstNode(baseAst)

    // The typical field access resolving mechanism
    lazy val byFieldAccess = scope.tryResolveFieldAccess(fieldIdentifierName, Some(baseTypeFullName))

    // Getters look like fields, but are underneath `get_`-prefixed methods
    lazy val byPropertyName = scope.tryResolveGetterInvocation(fieldIdentifierName, Some(baseTypeFullName))

    // accessExpr might be a qualified name e.g. `System.Console`, in which case `System` (baseAst) is not a type
    // but a namespace. In this scenario, we look up the entire expression
    lazy val byQualifiedName = scope.tryResolveTypeReference(accessExpr.code)

    val (typeFullName, isGetter) = byPropertyName
      .map(x => (x.returnType, true))
      .orElse(byFieldAccess.map(x => (x.typeName, false)))
      .orElse(byQualifiedName.map(x => (x.name, false)))
      .map((typeName, isGetter) => (scope.tryResolveTypeReference(typeName).map(_.name).getOrElse(typeName), isGetter))
      .getOrElse((Defines.Any, false))

    if (isGetter) {
      val resolvedGetter = byPropertyName.get.copy(returnType = typeFullName)
      astForMemberAccessGetterExpression(resolvedGetter, baseAst, baseTypeFullName, accessExpr)
    } else fieldAccessAst(accessExpr, accessExpr, baseAst, code(accessExpr), fieldIdentifierName, typeFullName) :: Nil
  }

  protected def astForElementAccessExpression(elementAccessExpression: DotNetNodeInfo): Seq[Ast] = {
    val exprAst = astForExpression(createDotNetNodeInfo(elementAccessExpression.json(ParserKeys.Expression)))

    createDotNetNodeInfo(elementAccessExpression.json(ParserKeys.ArgumentList))
      .json(ParserKeys.Arguments)
      .arr
      .map { x =>
        val argDotNetInfo = createDotNetNodeInfo(x)
        val argExpression = createDotNetNodeInfo(argDotNetInfo.json(ParserKeys.Expression))
        val argAst        = astForExpression(argExpression)
        val callNode = operatorCallNode(
          elementAccessExpression,
          elementAccessExpression.code,
          Operators.indexAccess,
          typeFullName = elementAccessTypeFullName(exprAst, argExpression)
        )

        callAst(callNode, exprAst ++ argAst)
      }
      .toSeq
  }

  private def elementAccessTypeFullName(exprAst: Seq[Ast], argExpression: DotNetNodeInfo): Option[String] = {
    val baseTypeFullName = getTypeFullNameFromAstNode(exprAst)
    Option.when(baseTypeFullName.endsWith("[]") && argExpression.node != RangeExpression) {
      baseTypeFullName.stripSuffix("[]")
    }
  }

  def astForObjectCreationExpression(objectCreation: DotNetNodeInfo): Seq[Ast] = {
    val dispatchType = DispatchTypes.STATIC_DISPATCH
    val typeFullName = Try(createDotNetNodeInfo(objectCreation.json(ParserKeys.Type))).toOption
      .map(nodeTypeFullName)
      .getOrElse(Defines.Any)

    val arguments = Try(astForArgumentList(createDotNetNodeInfo(objectCreation.json(ParserKeys.ArgumentList))))
      .getOrElse(Seq.empty)
    val initializerAst = Try(objectCreation.json(ParserKeys.Initializer)).toOption
      .collect { case initializer: ujson.Obj => createDotNetNodeInfo(initializer) }
      .map(astForObjectInitializerExpression)
      .getOrElse(Seq.empty)
    // TODO: Handle signature
    val signature      = None
    val name           = Defines.ConstructorMethodName
    val methodFullName = s"$typeFullName.$name"
    val _callNode = callNode(
      objectCreation,
      code(objectCreation),
      name,
      methodFullName,
      dispatchType,
      signature,
      Option(typeFullName)
    )

    Seq(callAst(_callNode, arguments ++ initializerAst, Option(Ast(thisNode))))
  }

  private def astForWithExpression(withExpr: DotNetNodeInfo): Seq[Ast] = {
    val expressionNode = createDotNetNodeInfo(withExpr.json(ParserKeys.Expression))
    val expressionAst  = astForExpression(expressionNode)
    val initializerAst = Try(withExpr.json(ParserKeys.Initializer)).toOption
      .collect { case initializer: ujson.Obj => createDotNetNodeInfo(initializer) }
      .map(astForObjectInitializerExpression)
      .getOrElse(Seq.empty)
    val callNode = operatorCallNode(
      withExpr,
      code(withExpr),
      CSharpOperators.withExpression,
      Some(getTypeFullNameFromAstNode(expressionAst))
    )

    Seq(callAst(callNode, expressionAst ++ initializerAst))
  }

  private def astForObjectInitializerExpression(initializer: DotNetNodeInfo): Seq[Ast] = {
    initializer
      .json(ParserKeys.Expressions)
      .arr
      .map(createDotNetNodeInfo)
      .flatMap(astForExpression)
      .toSeq
  }

  protected def astForArgumentList(argumentList: DotNetNodeInfo, baseTypeHint: Option[String] = None): Seq[Ast] = {
    argumentList
      .json(ParserKeys.Arguments)
      .arr
      .map(createDotNetNodeInfo)
      .flatMap { x =>
        val argExpression = createDotNetNodeInfo(x.json(ParserKeys.Expression))
        argExpression.node match {
          case _: BaseLambdaExpression =>
            astForSimpleLambdaExpression(argExpression, baseTypeHint)
          case _ => astForExpressionStatement(x)
        }
      }
      .toSeq
  }

  private def astForConditionalExpression(condExpr: DotNetNodeInfo): Seq[Ast] = {
    val conditionAst = astForNode(condExpr.json(ParserKeys.Condition))
    val whenTrue     = astForNode(condExpr.json(ParserKeys.WhenTrue))
    val whenFalse    = astForNode(condExpr.json(ParserKeys.WhenFalse))

    val typeFullName =
      Option(getTypeFullNameFromAstNode(whenTrue))
        .orElse(Option(getTypeFullNameFromAstNode(whenFalse)))
        .orElse(Option(Defines.Any))
    val callNode =
      operatorCallNode(condExpr, code(condExpr), Operators.conditional, typeFullName)

    Seq(callAst(callNode, conditionAst ++ whenTrue ++ whenFalse))
  }

  private def astForSwitchExpression(switchExpr: DotNetNodeInfo): Seq[Ast] = {
    val governingExpr = createDotNetNodeInfo(switchExpr.json(ParserKeys.GoverningExpression))
    val governingAst  = astForExpression(governingExpr)
    val armAstResults = switchExpr
      .json(ParserKeys.Arms)
      .arr
      .map(createDotNetNodeInfo)
      .map(astsForSwitchExpressionArm(governingExpr, _))
      .toSeq
    val armAsts    = armAstResults.flatMap(_._1)
    val resultAsts = armAstResults.flatMap(_._2)
    val callNode = operatorCallNode(
      switchExpr,
      code(switchExpr),
      CSharpOperators.switchExpression,
      Some(getTypeFullNameFromAstNode(resultAsts))
    )

    Seq(callAst(callNode, governingAst ++ armAsts))
  }

  private def astsForSwitchExpressionArm(governingExpr: DotNetNodeInfo, arm: DotNetNodeInfo): (Seq[Ast], Seq[Ast]) = {
    val pattern      = createDotNetNodeInfo(arm.json(ParserKeys.Pattern))
    val conditionAst = astForSwitchExpressionPatternCondition(governingExpr, pattern)
    val guardedConditionAst =
      Try(arm.json(ParserKeys.WhenClause)).toOption
        .collect { case whenClause: ujson.Obj => createDotNetNodeInfo(whenClause) }
        .flatMap { whenClause =>
          val guardNode = createDotNetNodeInfo(whenClause.json(ParserKeys.Condition))
          astForExpression(guardNode).headOption.map { guardAst =>
            val callNode = operatorCallNode(
              arm,
              s"${pattern.code} when ${guardNode.code}",
              Operators.logicalAnd,
              Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool))
            )
            callAst(callNode, Seq(conditionAst, guardAst))
          }
        }
        .getOrElse(conditionAst)
    val resultAsts = astForExpression(createDotNetNodeInfo(arm.json(ParserKeys.Expression)))

    (Seq(guardedConditionAst) ++ resultAsts, resultAsts)
  }

  private case class PatternSubject(code: String, asts: () => Seq[Ast])

  private def patternSubject(governingExpr: DotNetNodeInfo): PatternSubject =
    PatternSubject(governingExpr.code, () => astForExpression(governingExpr))

  private def astForSwitchExpressionPatternCondition(governingExpr: DotNetNodeInfo, pattern: DotNetNodeInfo): Ast =
    astForPatternCondition(patternSubject(governingExpr), pattern)

  private def astForPatternCondition(subject: PatternSubject, pattern: DotNetNodeInfo): Ast = {
    pattern.node match {
      case ConstantPattern =>
        val valueNode = createDotNetNodeInfo(pattern.json(ParserKeys.Expression))
        astForSwitchExpressionPatternComparison(subject, pattern, Operators.equals, valueNode)
      case DeclarationPattern =>
        astForDeclarationPatternCondition(subject, pattern)
      case TypePattern =>
        val typeInfo = createDotNetNodeInfo(pattern.json(ParserKeys.Type))
        astForRecursivePatternTypeCondition(subject, pattern, typeInfo)
      case VarPattern | TuplePattern =>
        Ast(literalNode(pattern, "true", BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)))
      case RelationalPattern =>
        val valueNode     = createDotNetNodeInfo(pattern.json(ParserKeys.Expression))
        val operatorToken = pattern.json(ParserKeys.OperatorToken)(ParserKeys.Value).str
        val operatorName  = binaryOperatorsMap.getOrElse(operatorToken, CSharpOperators.unknown)
        astForSwitchExpressionPatternComparison(subject, pattern, operatorName, valueNode)
      case DiscardPattern =>
        Ast(literalNode(pattern, "true", BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)))
      case NegatedPattern =>
        val innerPattern = createDotNetNodeInfo(pattern.json(ParserKeys.Pattern))
        val innerAst     = astForPatternCondition(subject, innerPattern)
        val callNode =
          operatorCallNode(
            pattern,
            code(pattern),
            Operators.logicalNot,
            Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool))
          )
        callAst(callNode, Seq(innerAst))
      case AndPattern | OrPattern =>
        val leftPattern  = createDotNetNodeInfo(pattern.json(ParserKeys.Left))
        val rightPattern = createDotNetNodeInfo(pattern.json(ParserKeys.Right))
        val operatorName = pattern.node match {
          case AndPattern => Operators.logicalAnd
          case OrPattern  => Operators.logicalOr
          case _          => CSharpOperators.unknown
        }
        val callNode =
          operatorCallNode(pattern, code(pattern), operatorName, Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)))
        callAst(
          callNode,
          Seq(astForPatternCondition(subject, leftPattern), astForPatternCondition(subject, rightPattern))
        )
      case ParenthesizedPattern =>
        val innerPattern = createDotNetNodeInfo(pattern.json(ParserKeys.Pattern))
        astForPatternCondition(subject, innerPattern)
      case ListPattern =>
        astForListPatternCondition(subject, pattern)
      case RecursivePattern =>
        astForRecursivePatternCondition(subject, pattern)
      case _ =>
        val patternAst = Ast(literalNode(pattern, code(pattern), Defines.Any))
        val callNode = operatorCallNode(
          pattern,
          s"${subject.code} is ${pattern.code}",
          CSharpOperators.unknown,
          Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool))
        )
        callAst(callNode, subject.asts() :+ patternAst)
    }
  }

  private def astForDeclarationPatternCondition(subject: PatternSubject, pattern: DotNetNodeInfo): Ast = {
    val typeInfo = createDotNetNodeInfo(pattern.json(ParserKeys.Type))
    if (typeInfo.code == "var") {
      Ast(literalNode(pattern, "true", BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)))
    } else {
      astForRecursivePatternTypeCondition(subject, pattern, typeInfo)
    }
  }

  private def astForSwitchExpressionPatternComparison(
    subject: PatternSubject,
    pattern: DotNetNodeInfo,
    operatorName: String,
    valueNode: DotNetNodeInfo
  ): Ast = {
    val callCode =
      if (operatorName == Operators.equals) s"${subject.code} == ${valueNode.code}"
      else s"${subject.code} ${pattern.code}"
    val callNode =
      operatorCallNode(pattern, callCode, operatorName, Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)))
    callAst(callNode, subject.asts() ++ astForExpression(valueNode))
  }

  private def astForListPatternCondition(subject: PatternSubject, pattern: DotNetNodeInfo): Ast = {
    val patterns = pattern.json(ParserKeys.Patterns).arr.map(createDotNetNodeInfo).toSeq
    val hasSlice = Try(pattern.json(ParserKeys.HasSlice).bool).getOrElse(false)
    val sliceIndex = Try(pattern.json(ParserKeys.SliceIndex).num.toInt)
      .getOrElse(patterns.length)
    val lengthCondition = astForListPatternLengthCondition(subject, pattern, patterns.length, hasSlice)
    val elementConditions = patterns.zipWithIndex.flatMap { case (elementPattern, idx) =>
      astForListPatternElementCondition(subject, pattern, elementPattern, idx, hasSlice, sliceIndex)
    }
    combinePatternConditions(pattern, lengthCondition +: elementConditions)
  }

  private def astForListPatternLengthCondition(
    subject: PatternSubject,
    pattern: DotNetNodeInfo,
    expectedLength: Int,
    hasSlice: Boolean
  ): Ast = {
    val operatorName = if (hasSlice) Operators.greaterEqualsThan else Operators.equals
    val operatorCode = if (hasSlice) ">=" else "=="
    val lengthAst    = astForListPatternLengthAccess(subject, pattern)
    val lengthLiteralAst =
      Ast(literalNode(pattern, expectedLength.toString, BuiltinTypes.DotNetTypeMap(BuiltinTypes.Int)))
    val callNode = operatorCallNode(
      pattern,
      s"${subject.code}.Length $operatorCode $expectedLength",
      operatorName,
      Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool))
    )
    callAst(callNode, Seq(lengthAst, lengthLiteralAst))
  }

  private def astForListPatternElementCondition(
    subject: PatternSubject,
    listPattern: DotNetNodeInfo,
    elementPattern: DotNetNodeInfo,
    index: Int,
    hasSlice: Boolean,
    sliceIndex: Int
  ): Option[Ast] = {
    val fromEnd = hasSlice && index >= sliceIndex
    elementPattern.node match {
      case DiscardPattern => None
      case _ =>
        Some(astForPatternCondition(listPatternElementSubject(subject, listPattern, index, fromEnd), elementPattern))
    }
  }

  private def astForListPatternLengthAccess(subject: PatternSubject, pattern: DotNetNodeInfo): Ast = {
    fieldAccessAst(
      pattern,
      pattern,
      subject.asts().headOption.getOrElse(Ast()),
      s"${subject.code}.Length",
      "Length",
      BuiltinTypes.DotNetTypeMap(BuiltinTypes.Int)
    )
  }

  private def listPatternElementSubject(
    subject: PatternSubject,
    pattern: DotNetNodeInfo,
    index: Int,
    fromEnd: Boolean
  ): PatternSubject = {
    val indexCode = listPatternElementIndexCode(pattern, index, fromEnd)
    PatternSubject(
      s"${subject.code}[$indexCode]",
      () => Seq(astForListPatternElementAccess(subject, pattern, index, fromEnd))
    )
  }

  private def astForListPatternElementAccess(
    subject: PatternSubject,
    pattern: DotNetNodeInfo,
    index: Int,
    fromEnd: Boolean
  ): Ast = {
    val indexAst =
      if (fromEnd) {
        val distance = pattern.json(ParserKeys.Patterns).arr.length - index
        val literal  = Ast(literalNode(pattern, distance.toString, BuiltinTypes.DotNetTypeMap(BuiltinTypes.Int)))
        val callNode = operatorCallNode(
          pattern,
          s"^$distance",
          CSharpOperators.indexFromEnd,
          Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Int))
        )
        callAst(callNode, Seq(literal))
      } else {
        Ast(literalNode(pattern, index.toString, BuiltinTypes.DotNetTypeMap(BuiltinTypes.Int)))
      }
    val indexCode = indexAst.rootCodeOrEmpty
    val callNode  = operatorCallNode(pattern, s"${subject.code}[$indexCode]", Operators.indexAccess, None)
    callAst(callNode, subject.asts() :+ indexAst)
  }

  private def listPatternElementIndexCode(pattern: DotNetNodeInfo, index: Int, fromEnd: Boolean): String =
    if (fromEnd) {
      val distance = pattern.json(ParserKeys.Patterns).arr.length - index
      s"^$distance"
    } else {
      index.toString
    }

  private def astForRecursivePatternCondition(subject: PatternSubject, pattern: DotNetNodeInfo): Ast = {
    val typeCondition = Try(pattern.json(ParserKeys.Type)).toOption.collect { case typ: ujson.Obj =>
      astForRecursivePatternTypeCondition(subject, pattern, createDotNetNodeInfo(typ))
    }
    val positionalConditions =
      Try(pattern.json(ParserKeys.PositionalPatterns).arr)
        .getOrElse(ArrayBuffer.empty)
        .map(createDotNetNodeInfo)
        .zipWithIndex
        .flatMap { case (subpattern, idx) =>
          astForRecursivePositionalSubpatternCondition(subject, subpattern, idx)
        }
        .toSeq
    val propertyConditions =
      Try(pattern.json(ParserKeys.PropertyPatterns).arr)
        .getOrElse(ArrayBuffer.empty)
        .map(createDotNetNodeInfo)
        .flatMap(subpattern => astForRecursivePropertySubpatternCondition(subject, subpattern))
        .toSeq

    combinePatternConditions(pattern, typeCondition.toSeq ++ positionalConditions ++ propertyConditions)
  }

  private def astForRecursivePatternTypeCondition(
    subject: PatternSubject,
    pattern: DotNetNodeInfo,
    typeInfo: DotNetNodeInfo
  ): Ast = {
    val typeFullName = nodeTypeFullName(typeInfo)
    val callNode =
      operatorCallNode(pattern, s"${subject.code} is ${typeInfo.code}", Operators.instanceOf, Option(BuiltinTypes.Bool))
    val typeNode = NewTypeRef()
      .code(typeFullName)
      .lineNumber(line(pattern))
      .columnNumber(column(pattern))
      .typeFullName(typeFullName)
    callAst(callNode, subject.asts() :+ Ast(typeNode))
  }

  private def astForRecursivePositionalSubpatternCondition(
    subject: PatternSubject,
    subpattern: DotNetNodeInfo,
    index: Int
  ): Option[Ast] =
    Try(subpattern.json(ParserKeys.Pattern)).toOption.collect { case patternJson: ujson.Obj =>
      val elementSubject = memberPatternSubject(subject, subpattern, Seq(s"Item${index + 1}"))
      astForPatternCondition(elementSubject, createDotNetNodeInfo(patternJson))
    }

  private def astForRecursivePropertySubpatternCondition(
    subject: PatternSubject,
    subpattern: DotNetNodeInfo
  ): Option[Ast] =
    for {
      nameJson    <- Try(subpattern.json(ParserKeys.Name)).toOption.collect { case name: ujson.Obj => name }
      patternJson <- Try(subpattern.json(ParserKeys.Pattern)).toOption.collect { case pattern: ujson.Obj => pattern }
      memberPath = propertyPatternMemberPath(createDotNetNodeInfo(nameJson))
      if memberPath.nonEmpty
    } yield astForPatternCondition(
      memberPatternSubject(subject, subpattern, memberPath),
      createDotNetNodeInfo(patternJson)
    )

  private def propertyPatternMemberPath(name: DotNetNodeInfo): Seq[String] =
    name.code.split('.').toSeq.map(_.trim).filter(_.nonEmpty)

  private def memberPatternSubject(
    subject: PatternSubject,
    origin: DotNetNodeInfo,
    memberPath: Seq[String]
  ): PatternSubject = {
    val memberCode = s"${subject.code}.${memberPath.mkString(".")}"
    PatternSubject(memberCode, () => Seq(astForPatternMemberAccess(subject, origin, memberPath)))
  }

  private def astForPatternMemberAccess(
    subject: PatternSubject,
    origin: DotNetNodeInfo,
    memberPath: Seq[String]
  ): Ast = {
    memberPath
      .foldLeft((subject.asts().headOption.getOrElse(Ast()), subject.code)) { case ((baseAst, baseCode), memberName) =>
        val accessCode = s"$baseCode.$memberName"
        (fieldAccessAst(origin, origin, baseAst, accessCode, memberName, Defines.Any), accessCode)
      }
      ._1
  }

  private def combinePatternConditions(pattern: DotNetNodeInfo, conditions: Seq[Ast]): Ast = {
    conditions match {
      case Seq() =>
        Ast(literalNode(pattern, "true", BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)))
      case Seq(condition) => condition
      case head +: tail =>
        tail.foldLeft(head) { case (lhs, rhs) =>
          val callNode = operatorCallNode(
            pattern,
            s"${lhs.rootCodeOrEmpty} && ${rhs.rootCodeOrEmpty}",
            Operators.logicalAnd,
            Some(BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool))
          )
          callAst(callNode, Seq(lhs, rhs))
        }
    }
  }

  private def astForCastExpression(castExpr: DotNetNodeInfo): Seq[Ast] = {
    val typeInfo     = createDotNetNodeInfo(castExpr.json(ParserKeys.Type))
    val typeFullName = nodeTypeFullName(typeInfo)

    val callNode = operatorCallNode(castExpr, castExpr.code, Operators.cast, Some(typeFullName))

    // We can guarantee that there is an expression on the RHS
    val exprAst = astForExpression(createDotNetNodeInfo(castExpr.json(ParserKeys.Expression)))
    Seq(callAst(callNode, Seq(typeRefAst(typeInfo, castExpr)) ++ exprAst))
  }

  private def astForAsExpression(asExpr: DotNetNodeInfo): Seq[Ast] = {
    val typeInfo     = createDotNetNodeInfo(asExpr.json(ParserKeys.Type))
    val typeFullName = nodeTypeFullName(typeInfo)
    val callNode     = operatorCallNode(asExpr, code(asExpr), Operators.cast, Some(typeFullName))
    val expression   = astForExpression(createDotNetNodeInfo(asExpr.json(ParserKeys.Expression)))

    Seq(callAst(callNode, Seq(typeRefAst(typeInfo, asExpr)) ++ expression))
  }

  private def astForIsExpression(isExpr: DotNetNodeInfo): Seq[Ast] = {
    val expression = astForExpression(createDotNetNodeInfo(isExpr.json(ParserKeys.Expression)))
    val typeInfo   = createDotNetNodeInfo(isExpr.json(ParserKeys.Type))
    val callNode = operatorCallNode(isExpr, code(isExpr), Operators.instanceOf, Some(DotNetTypeMap(BuiltinTypes.Bool)))

    Seq(callAst(callNode, expression :+ typeRefAst(typeInfo, isExpr)))
  }

  private def astForTypeOfExpression(typeOfExpr: DotNetNodeInfo): Seq[Ast] =
    astForTypeOperatorExpression(typeOfExpr, CSharpOperators.typeOf, "System.Type")

  private def astForSizeOfExpression(sizeOfExpr: DotNetNodeInfo): Seq[Ast] =
    astForTypeOperatorExpression(sizeOfExpr, Operators.sizeOf, DotNetTypeMap(BuiltinTypes.Int))

  private def astForNameOfExpression(nameOfExpr: DotNetNodeInfo): Seq[Ast] = {
    val arguments = Try(nameOfExpr.json(ParserKeys.ArgumentList)(ParserKeys.Arguments).arr)
      .getOrElse(ArrayBuffer.empty)
      .flatMap { argument =>
        Try(argument(ParserKeys.Expression)).toOption
          .collect { case expression: ujson.Obj => astForExpression(createDotNetNodeInfo(expression)) }
          .getOrElse(Seq.empty)
      }
      .toSeq
    val callNode =
      operatorCallNode(nameOfExpr, code(nameOfExpr), CSharpOperators.nameOf, Some(DotNetTypeMap(BuiltinTypes.String)))

    Seq(callAst(callNode, arguments))
  }

  private def astForTypeOperatorExpression(
    typeOperatorExpr: DotNetNodeInfo,
    operatorName: String,
    typeFullName: String
  ): Seq[Ast] = {
    val typeInfo = createDotNetNodeInfo(typeOperatorExpr.json(ParserKeys.Type))
    val callNode = operatorCallNode(typeOperatorExpr, code(typeOperatorExpr), operatorName, Some(typeFullName))

    Seq(callAst(callNode, Seq(typeRefAst(typeInfo, typeOperatorExpr))))
  }

  private def astForDefaultExpression(defaultExpr: DotNetNodeInfo): Seq[Ast] = {
    val typeInfo = Try(defaultExpr.json(ParserKeys.Type)).toOption.collect { case typ: ujson.Obj =>
      createDotNetNodeInfo(typ)
    }
    val typeFullName = typeInfo.map(nodeTypeFullName).getOrElse(Defines.Any)
    val callNode = operatorCallNode(defaultExpr, code(defaultExpr), CSharpOperators.defaultValue, Some(typeFullName))

    Seq(callAst(callNode, typeInfo.map(typeRefAst(_, defaultExpr)).toSeq))
  }

  private def astForThrowExpression(throwExpr: DotNetNodeInfo): Seq[Ast] = {
    val argument = Try(throwExpr.json(ParserKeys.Expression)).toOption.collect { case expression: ujson.Obj =>
      createDotNetNodeInfo(expression)
    }
    val argumentAsts = argument.map(astForExpression).getOrElse(Seq.empty)
    val callNode = operatorCallNode(
      throwExpr,
      code(throwExpr),
      CSharpOperators.throws,
      Some(getTypeFullNameFromAstNode(argumentAsts))
    )

    Seq(callAst(callNode, argumentAsts))
  }

  private def astForRefExpression(refExpr: DotNetNodeInfo): Seq[Ast] = {
    val expression = astForExpression(createDotNetNodeInfo(refExpr.json(ParserKeys.Expression)))
    val callNode =
      operatorCallNode(refExpr, code(refExpr), CSharpOperators.ref, Some(getTypeFullNameFromAstNode(expression)))

    Seq(callAst(callNode, expression))
  }

  private def astForMakeRefExpression(makeRefExpr: DotNetNodeInfo): Seq[Ast] = {
    val expression = astForExpression(createDotNetNodeInfo(makeRefExpr.json(ParserKeys.Expression)))
    val callNode =
      operatorCallNode(makeRefExpr, code(makeRefExpr), CSharpOperators.makeRef, Some("System.TypedReference"))

    Seq(callAst(callNode, expression))
  }

  private def astForRefTypeExpression(refTypeExpr: DotNetNodeInfo): Seq[Ast] = {
    val expression = astForExpression(createDotNetNodeInfo(refTypeExpr.json(ParserKeys.Expression)))
    val callNode =
      operatorCallNode(refTypeExpr, code(refTypeExpr), CSharpOperators.refType, Some("System.Type"))

    Seq(callAst(callNode, expression))
  }

  private def astForRefValueExpression(refValueExpr: DotNetNodeInfo): Seq[Ast] = {
    val expression = astForExpression(createDotNetNodeInfo(refValueExpr.json(ParserKeys.Expression)))
    val typeInfo   = createDotNetNodeInfo(refValueExpr.json(ParserKeys.Type))
    val callNode =
      operatorCallNode(refValueExpr, code(refValueExpr), CSharpOperators.refValue, Some(nodeTypeFullName(typeInfo)))

    Seq(callAst(callNode, expression :+ typeRefAst(typeInfo, refValueExpr)))
  }

  private def astForSpreadElement(spreadElement: DotNetNodeInfo): Seq[Ast] = {
    val expression = astForExpression(createDotNetNodeInfo(spreadElement.json(ParserKeys.Expression)))
    val callNode =
      operatorCallNode(
        spreadElement,
        code(spreadElement),
        CSharpOperators.spread,
        Some(getTypeFullNameFromAstNode(expression))
      )

    Seq(callAst(callNode, expression))
  }

  private def typeRefAst(typeInfo: DotNetNodeInfo, origin: DotNetNodeInfo): Ast = {
    Ast(
      NewTypeRef()
        .code(typeInfo.code)
        .lineNumber(line(origin))
        .columnNumber(column(origin))
        .typeFullName(nodeTypeFullName(typeInfo))
    )
  }

  private def astForImplicitArrayCreationExpression(implArrExpr: DotNetNodeInfo): Seq[Ast] = {
    astForArrayInitializerExpression(createDotNetNodeInfo(implArrExpr.json(ParserKeys.Initializer)))
  }

  private def astForQueryExpression(queryExpr: DotNetNodeInfo): Seq[Ast] = {
    val fromClauseAst = Try(queryExpr.json(ParserKeys.FromClause)).toOption
      .collect { case fromClause: ujson.Obj => createDotNetNodeInfo(fromClause) }
      .map(astForQueryClause)
      .getOrElse(Seq.empty)
    val clauseAsts = Try(queryExpr.json(ParserKeys.Clauses).arr)
      .getOrElse(ArrayBuffer.empty)
      .map(createDotNetNodeInfo)
      .flatMap(astForQueryClause)
      .toSeq
    val callNode = operatorCallNode(queryExpr, code(queryExpr), CSharpOperators.queryExpression, Some(Defines.Any))

    Seq(callAst(callNode, fromClauseAst ++ clauseAsts))
  }

  private def astForQueryClause(clause: DotNetNodeInfo): Seq[Ast] = {
    clause.node match {
      case FromClause =>
        astForQueryClauseType(clause) ++ astForOptionalQueryExpression(clause, ParserKeys.Expression)
      case JoinClause =>
        astForQueryClauseType(clause) ++ Seq(
          ParserKeys.InExpression,
          ParserKeys.LeftExpression,
          ParserKeys.RightExpression
        ).flatMap(astForOptionalQueryExpression(clause, _))
      case LetClause =>
        astForOptionalQueryExpression(clause, ParserKeys.Expression)
      case OrderByClause =>
        val directions = Try(clause.json(ParserKeys.Directions).arr).getOrElse(ArrayBuffer.empty)
        Try(clause.json(ParserKeys.Expressions).arr)
          .getOrElse(ArrayBuffer.empty)
          .zipWithIndex
          .flatMap { case (expression, idx) =>
            val expressionAsts = astForExpression(createDotNetNodeInfo(expression))
            val directionAst = directions
              .lift(idx)
              .flatMap(direction => Try(direction.str).toOption)
              .filter(_.nonEmpty)
              .map(direction => Ast(literalNode(clause, direction, BuiltinTypes.DotNetTypeMap(BuiltinTypes.String))))
              .toSeq
            expressionAsts ++ directionAst
          }
          .toSeq
      case WhereClause =>
        astForOptionalQueryExpression(clause, ParserKeys.Condition)
      case SelectClause =>
        astForOptionalQueryExpression(clause, ParserKeys.Expression)
      case GroupClause =>
        astForOptionalQueryExpression(clause, ParserKeys.Expression) ++
          astForOptionalQueryExpression(clause, ParserKeys.ByExpression)
      case _: IdentifierNode =>
        astForExpression(clause)
      case _ =>
        Seq.empty
    }
  }

  private def astForQueryClauseType(clause: DotNetNodeInfo): Seq[Ast] = {
    Try(clause.json(ParserKeys.Type)).toOption.collect { case typ: ujson.Obj =>
      val typeNode     = createDotNetNodeInfo(typ)
      val typeFullName = nodeTypeFullName(typeNode)
      Ast(
        NewTypeRef()
          .code(typeNode.code)
          .lineNumber(line(typeNode))
          .columnNumber(column(typeNode))
          .typeFullName(typeFullName)
      )
    }.toSeq
  }

  private def astForOptionalQueryExpression(clause: DotNetNodeInfo, key: String): Seq[Ast] = {
    Try(clause.json(key)).toOption
      .collect { case expression: ujson.Obj =>
        createDotNetNodeInfo(expression)
      }
      .map(astForExpression)
      .getOrElse(Seq.empty)
  }

  private def astForStackAllocExpression(stackAllocExpr: DotNetNodeInfo): Seq[Ast] = {
    val typeNode = Try(stackAllocExpr.json(ParserKeys.Type)).toOption.collect { case typ: ujson.Obj =>
      createDotNetNodeInfo(typ)
    }
    val typeFullName = typeNode.map(nodeTypeFullName).getOrElse(Defines.Any)
    val typeAst = typeNode.map { typ =>
      Ast(
        NewTypeRef()
          .code(typ.code)
          .lineNumber(line(typ))
          .columnNumber(column(typ))
          .typeFullName(typeFullName)
      )
    }.toSeq
    val rankAsts = typeNode.toSeq.flatMap { typ =>
      Try(typ.json(ParserKeys.Rank)).toOption
        .collect { case rank: ujson.Obj => createDotNetNodeInfo(rank) }
        .toSeq
        .flatMap { rank =>
          Try(rank.json(ParserKeys.Expressions).arr)
            .getOrElse(ArrayBuffer.empty)
            .map(createDotNetNodeInfo)
            .flatMap(astForExpression)
            .toSeq
        }
    }
    val initializerAst = Try(stackAllocExpr.json(ParserKeys.Initializer)).toOption
      .collect { case initializer: ujson.Obj => createDotNetNodeInfo(initializer) }
      .map(astForArrayInitializerExpression)
      .getOrElse(Seq.empty)
    val callNode =
      operatorCallNode(stackAllocExpr, code(stackAllocExpr), CSharpOperators.stackAlloc, Some(typeFullName))

    Seq(callAst(callNode, typeAst ++ rankAsts ++ initializerAst))
  }

  private def astForInterpolatedStringExpression(strExpr: DotNetNodeInfo): Seq[Ast] = {
    val contentAsts = strExpr
      .json(ParserKeys.Contents)
      .arr
      .map(createDotNetNodeInfo)
      .flatMap { expr =>
        expr.node match {
          case InterpolatedStringText => astForInterpolatedStringText(expr)
          case Interpolation          => astForInterpolation(expr)
          case _                      => Nil
        }
      }
      .toSeq

    val _callNode = operatorCallNode(
      strExpr,
      code(strExpr),
      Operators.formatString,
      Option(BuiltinTypes.DotNetTypeMap(BuiltinTypes.String))
    )

    Seq(callAst(_callNode, contentAsts))
  }

  private def astForInterpolation(interpolationExpr: DotNetNodeInfo): Seq[Ast] = {
    val expressionAst = astForNode(interpolationExpr.json(ParserKeys.Expression))
    val alignmentAst =
      Try(interpolationExpr.json(ParserKeys.AlignmentClause)).toOption
        .collect { case clause: ujson.Obj => createDotNetNodeInfo(clause) }
        .toSeq
        .flatMap(astForInterpolationAlignmentClause)
    val formatAst =
      Try(interpolationExpr.json(ParserKeys.FormatClause)).toOption
        .collect { case clause: ujson.Obj => createDotNetNodeInfo(clause) }
        .toSeq
        .flatMap(astForInterpolationFormatClause)
    expressionAst ++ alignmentAst ++ formatAst
  }

  private def astForInterpolationAlignmentClause(alignmentClause: DotNetNodeInfo): Seq[Ast] = {
    Try(alignmentClause.json(ParserKeys.Expression)).toOption
      .collect { case expression: ujson.Obj => astForNode(createDotNetNodeInfo(expression)) }
      .getOrElse(Seq.empty)
  }

  private def astForInterpolationFormatClause(formatClause: DotNetNodeInfo): Seq[Ast] = {
    Try(formatClause.json(ParserKeys.FormatStringToken)(ParserKeys.Value).str)
      .map(format => Ast(literalNode(formatClause, format, BuiltinTypes.DotNetTypeMap(BuiltinTypes.String))))
      .toOption
      .toSeq
  }

  private def astForInterpolatedStringText(interpolatedTextExpr: DotNetNodeInfo): Seq[Ast] = {
    Seq(
      Ast(
        literalNode(interpolatedTextExpr, code(interpolatedTextExpr), BuiltinTypes.DotNetTypeMap(BuiltinTypes.String))
      )
    )
  }

  private def makeMemberAccess(expression: DotNetNodeInfo, name: DotNetNodeInfo): DotNetNodeInfo = {
    val json = ujson.Obj()
    json(ParserKeys.Expression) = expression.json.transform(ujson.Value)
    json(ParserKeys.Name) = name.json.transform(ujson.Value)
    json(ParserKeys.MetaData) = expression.json(ParserKeys.MetaData).transform(ujson.Value)
    json(ParserKeys.MetaData)(ParserKeys.Kind) = "ast.SimpleMemberAccessExpression"

    expression.copy(node = SimpleMemberAccessExpression, json = json)
  }

  private def makeInvocation(expression: DotNetNodeInfo, args: DotNetNodeInfo): DotNetNodeInfo = {
    val json = ujson.Obj()
    json(ParserKeys.Expression) = expression.json.transform(ujson.Value)
    json(ParserKeys.ArgumentList) = args.json.transform(ujson.Value)
    json(ParserKeys.MetaData) = expression.json(ParserKeys.MetaData).transform(ujson.Value)
    json(ParserKeys.MetaData)(ParserKeys.Kind) = "ast.InvocationExpression"

    expression.copy(node = InvocationExpression, json = json)
  }

  private def makeElementAccess(expression: DotNetNodeInfo, elementBinding: DotNetNodeInfo): DotNetNodeInfo = {
    val accessCode = elementAccessCode(expression, elementBinding)
    val json       = ujson.Obj()
    json(ParserKeys.Expression) = expression.json.transform(ujson.Value)
    json(ParserKeys.ArgumentList) = elementBinding.json(ParserKeys.ArgumentList).transform(ujson.Value)
    json(ParserKeys.MetaData) = expression.json(ParserKeys.MetaData).transform(ujson.Value)
    json(ParserKeys.MetaData)(ParserKeys.Kind) = "ast.ElementAccessExpression"
    json(ParserKeys.MetaData)(ParserKeys.Code) = accessCode

    expression.copy(node = ElementAccessExpression, json = json, code = accessCode)
  }

  private def elementAccessCode(expression: DotNetNodeInfo, elementBinding: DotNetNodeInfo): String = {
    val argumentCodes =
      Try(elementBinding.json(ParserKeys.ArgumentList)(ParserKeys.Arguments).arr.map(createDotNetNodeInfo).map(_.code))
        .getOrElse(ArrayBuffer.empty)
    s"${expression.code}[${argumentCodes.mkString(", ")}]"
  }

  private def moveConditionalAccessInvocationToMemberBinding(
    invocationExpr: DotNetNodeInfo,
    conditionalAccessExpr: DotNetNodeInfo,
    argumentList: DotNetNodeInfo
  ): DotNetNodeInfo = {
    val whenNotNull        = createDotNetNodeInfo(conditionalAccessExpr.json(ParserKeys.WhenNotNull))
    val invokedWhenNotNull = makeInvocation(whenNotNull, argumentList)
    val json               = conditionalAccessExpr.json.transform(ujson.Value)

    json(ParserKeys.WhenNotNull) = invokedWhenNotNull.json.transform(ujson.Value)
    json(ParserKeys.MetaData) = invocationExpr.json(ParserKeys.MetaData).transform(ujson.Value)
    json(ParserKeys.MetaData)(ParserKeys.Kind) = "ast.ConditionalAccessExpression"

    invocationExpr.copy(node = ConditionalAccessExpression, json = json)
  }

  /** Traverses the "spine" of a chained `?.`/`.` expression. For instance, `x?.y.z?.w` becomes [x, y, z, w]. Notice
    * that, whereas `.` is left-associative, `?.` is right-associative.
    */
  private def traverseConditionalAccessSpine(expr: DotNetNodeInfo): Seq[DotNetNodeInfo] = {
    expr.node match {
      case ConditionalAccessExpression =>
        val lhs = createDotNetNodeInfo(expr.json(ParserKeys.Expression))
        val rhs = createDotNetNodeInfo(expr.json(ParserKeys.WhenNotNull))
        lhs +: traverseConditionalAccessSpine(rhs)
      case SimpleMemberAccessExpression =>
        val lhs = createDotNetNodeInfo(expr.json(ParserKeys.Expression))
        val rhs = createDotNetNodeInfo(expr.json(ParserKeys.Name))
        traverseConditionalAccessSpine(lhs) :+ rhs
      case _ =>
        expr :: Nil
    }
  }

  /** Given a sequence of nodes [x, y, z, w], creates the corresponding [[DotNetNodeInfo]] for `x.y.z.w`.
    */
  private def rebuildSpineAsMemberAccesses(spine: Seq[DotNetNodeInfo]): Option[DotNetNodeInfo] = {
    def combine(lhs: DotNetNodeInfo, rhs: DotNetNodeInfo): DotNetNodeInfo = rhs.node match {
      case MemberBindingExpression =>
        val name = createDotNetNodeInfo(rhs.json(ParserKeys.Name))
        makeMemberAccess(lhs, name)
      case InvocationExpression =>
        val name = createDotNetNodeInfo(rhs.json(ParserKeys.Expression)(ParserKeys.Name))
        val args = createDotNetNodeInfo(rhs.json(ParserKeys.ArgumentList))
        makeInvocation(makeMemberAccess(lhs, name), args)
      case SimpleMemberAccessExpression =>
        val expr = createDotNetNodeInfo(rhs.json(ParserKeys.Expression))
        val name = createDotNetNodeInfo(rhs.json(ParserKeys.Name))
        makeMemberAccess(makeMemberAccess(lhs, expr), name)
      case ElementAccessExpression =>
        makeElementAccess(lhs, rhs)
      case _ =>
        makeMemberAccess(lhs, rhs)
    }

    spine.foldLeft(None: Option[DotNetNodeInfo]) { case (lhsOpt, rhs) => lhsOpt.map(combine(_, rhs)).orElse(Some(rhs)) }
  }

  /** Handles `x?.y` expressions, by rewriting ConditionalAccessExpressions into SimpleMemberAccessExpresions, i.e.
    * handling them as if they were `x.y`.
    */
  private def astForConditionalAccessExpression(condAccExpr: DotNetNodeInfo): Seq[Ast] =
    rebuildSpineAsMemberAccesses(traverseConditionalAccessSpine(condAccExpr)) match {
      case None =>
        logger.warn(s"Failed to rewrite ${code(condAccExpr)}. Skipping")
        Nil
      case Some(rewritten) => astForNode(rewritten)
    }

  private def astForSuppressNullableWarningExpression(suppressNullableExpr: DotNetNodeInfo): Seq[Ast] = {
    val _identifierNode = createDotNetNodeInfo(suppressNullableExpr.json(ParserKeys.Operand))
    Seq(astForIdentifier(_identifierNode))
  }

  protected def astForAttributeLists(attributeList: DotNetNodeInfo): Seq[Ast] = {
    val target = attributeTargetName(attributeList)
    attributeList.json(ParserKeys.Attributes).arr.map(createDotNetNodeInfo).map(astForAttribute(_, target)).toSeq
  }

  protected def astForGlobalAttribute(globalAttribute: DotNetNodeInfo): Seq[Ast] = {
    Try(globalAttribute.json(ParserKeys.AttributeLists))
      .map(_.arr.map(createDotNetNodeInfo).flatMap(astForAttributeLists).toSeq)
      .getOrElse(Seq.empty)
  }

  private def astForAttribute(attribute: DotNetNodeInfo, target: Option[String] = None): Ast = {
    val attributeName = nameFromNode(attribute)
    val fullName      = nodeTypeFullName(attribute)
    val argumentAsts  = astsForAttributeArguments(attribute)

    val annotationCode  = target.filter(_.nonEmpty).map(t => s"$t: ${attribute.code}").getOrElse(attribute.code)
    val _annotationNode = annotationNode(attribute, annotationCode, attributeName, fullName)
    annotationAst(_annotationNode, argumentAsts)
  }

  private def astsForAttributeArguments(attribute: DotNetNodeInfo): Seq[Ast] = {
    Try(attribute.json(ParserKeys.ArgumentList)(ParserKeys.Arguments).arr)
      .map(_.map(createDotNetNodeInfo).flatMap(astForAttributeArgument).toSeq)
      .getOrElse(Seq.empty)
  }

  private def astForAttributeArgument(argument: DotNetNodeInfo): Seq[Ast] = {
    val expressionAsts = Try(astForNode(argument.json(ParserKeys.Expression))).getOrElse(Seq.empty)
    attributeArgumentName(argument) match {
      case Some(name) =>
        expressionAsts.headOption.map(annotationAssignmentAst(name, argument.code, _)).toSeq ++ expressionAsts.drop(1)
      case None => expressionAsts
    }
  }

  private def attributeArgumentName(argument: DotNetNodeInfo): Option[String] = {
    Seq(ParserKeys.NameColon, ParserKeys.NameEquals).iterator
      .flatMap { key =>
        Try(argument.json(key)(ParserKeys.Name)).toOption.collect { case name: ujson.Obj =>
          nameFromNode(createDotNetNodeInfo(name))
        }
      }
      .nextOption()
  }

  private def attributeTargetName(attributeList: DotNetNodeInfo): Option[String] = {
    Try(attributeList.json(ParserKeys.Target)).toOption.collect { case target: ujson.Obj => target }.flatMap { target =>
      Try(target(ParserKeys.Identifier)(ParserKeys.Value).str).toOption
    }
  }

  /** Lowers a pattern expression into a condition and then a declaration if one occurs.
    * @param isPatternExpression
    *   a pattern expression which may include a declaration.
    * @return
    *   a condition and then (potentially) declaration.
    */
  protected def astsForIsPatternExpression(isPatternExpression: DotNetNodeInfo): List[Ast] = {
    val pattern = createDotNetNodeInfo(isPatternExpression.json(ParserKeys.Pattern))

    val expressionNode = createDotNetNodeInfo(isPatternExpression.json(ParserKeys.Expression))
    val expression     = astForExpression(expressionNode)

    pattern.node match {
      case DeclarationPattern =>
        val designation = createDotNetNodeInfo(pattern.json(ParserKeys.Designation))
        val typeInfo    = createDotNetNodeInfo(pattern.json(ParserKeys.Type))
        val patternTypeFullName =
          if (typeInfo.code == "var") getTypeFullNameFromAstNode(expression)
          else nodeTypeFullName(typeInfo)

        val instanceOfCallNode =
          operatorCallNode(expressionNode, code(pattern), Operators.instanceOf, Option(BuiltinTypes.Bool))

        val assignmentAst = operatorCallNode(
          expressionNode,
          s"${typeInfo.code} ${designation.code} = ${expressionNode.code}",
          Operators.assignment,
          Option(patternTypeFullName)
        )

        val designationAst = astForIdentifier(designation, patternTypeFullName)

        val typeNode = NewTypeRef()
          .code(patternTypeFullName)
          .lineNumber(line(expressionNode))
          .columnNumber(column(expressionNode))
          .typeFullName(patternTypeFullName)

        val conditionAst =
          if (typeInfo.code == "var") Ast(literalNode(pattern, "true", BuiltinTypes.DotNetTypeMap(BuiltinTypes.Bool)))
          else callAst(instanceOfCallNode, expression :+ Ast(typeNode))
        val expressionClone   = astForExpression(expressionNode)
        val assignmentCallAst = callAst(assignmentAst, designationAst +: expressionClone)

        List(conditionAst, assignmentCallAst)
      case ConstantPattern =>
        val expr    = createDotNetNodeInfo(pattern.json(ParserKeys.Expression))
        val exprAst = astForExpression(expr)

        val typeFullName = nodeTypeFullName(expr)

        val equalCallNode =
          operatorCallNode(expr, code(pattern), Operators.equals, Option(BuiltinTypes.Bool))
        val equalCallAst = callAst(equalCallNode, expression ++ exprAst)

        List(equalCallAst)
      case RelationalPattern | DiscardPattern | AndPattern | OrPattern | ParenthesizedPattern | ListPattern |
          RecursivePattern | TypePattern | VarPattern | TuplePattern =>
        List(astForSwitchExpressionPatternCondition(expressionNode, pattern))
      case NegatedPattern =>
        val negatedPattern = createDotNetNodeInfo(pattern.json(ParserKeys.Pattern))
        negatedPattern.node match {
          case ConstantPattern =>
            val expr    = createDotNetNodeInfo(negatedPattern.json(ParserKeys.Expression))
            val exprAst = astForExpression(expr)

            val notEqualCallNode =
              operatorCallNode(expr, code(pattern), Operators.notEquals, Option(BuiltinTypes.Bool))
            val notEqualCallAst = callAst(notEqualCallNode, expression ++ exprAst)

            List(notEqualCallAst)
          case DeclarationPattern =>
            val typeInfo = createDotNetNodeInfo(negatedPattern.json(ParserKeys.Type))

            val instanceOfCallNode =
              operatorCallNode(expressionNode, code(negatedPattern), Operators.instanceOf, Option(BuiltinTypes.Bool))

            val typeNode = NewTypeRef()
              .code(nodeTypeFullName(typeInfo))
              .lineNumber(line(expressionNode))
              .columnNumber(column(expressionNode))
              .typeFullName(nodeTypeFullName(typeInfo))

            val instanceOfAst = callAst(instanceOfCallNode, expression :+ Ast(typeNode))
            val notCallNode =
              operatorCallNode(isPatternExpression, code(pattern), Operators.logicalNot, Option(BuiltinTypes.Bool))

            List(callAst(notCallNode, List(instanceOfAst)))
          case RelationalPattern | DiscardPattern | AndPattern | OrPattern | ParenthesizedPattern | ListPattern |
              RecursivePattern | TypePattern | VarPattern | TuplePattern =>
            List(astForSwitchExpressionPatternCondition(expressionNode, pattern))
          case x =>
            logger.warn(s"Unsupported negated pattern in pattern expression, $x")
            astForExpression(negatedPattern).toList
        }
      case x =>
        logger.warn(s"Unsupported pattern in pattern expression, $x")
        astForExpression(pattern).toList
    }
  }

}
