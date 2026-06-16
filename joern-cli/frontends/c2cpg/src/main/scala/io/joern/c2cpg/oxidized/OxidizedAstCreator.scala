package io.joern.c2cpg.oxidized

import io.joern.c2cpg.Config
import io.joern.c2cpg.astcreation.Defines
import io.joern.x2cpg.{Ast, AstCreatorBase, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.codepropertygraph.generated.{
  ControlStructureTypes,
  DiffGraphBuilder,
  DispatchTypes,
  EdgeTypes,
  EvaluationStrategies,
  ModifierTypes,
  NodeTypes,
  Operators
}
import io.shiftleft.semanticcpg.language.types.structure.NamespaceTraversal

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Paths}
import scala.collection.mutable
import scala.util.Try

private final case class OxOrigin(code: String, line: Option[Int])

private object OxOrigin {
  def apply(declaration: OxDeclaration): OxOrigin = OxOrigin(declaration.code, Option(declaration.line))
  def apply(statement: OxStatement): OxOrigin     = OxOrigin(statement.code, Option(statement.line))
  def apply(expression: OxExpression): OxOrigin   = OxOrigin(expression.code, Option(expression.line))
}

final class OxidizedAstCreator(filename: String, document: OxDocument, config: Config)
    extends AstCreatorBase[OxOrigin, OxidizedAstCreator](filename)(config.schemaValidation) {

  private implicit val schemaValidation: ValidationMode = config.schemaValidation

  private final case class ScopeEntry(typeFullName: String, declaration: NewNode)
  private final case class CapturedGlobal(scopeEntry: ScopeEntry, binding: NewClosureBinding, globalEntry: ScopeEntry)
  private final case class FunctionCaptureContext(
    function: OxFunctionDecl,
    methodRef: NewMethodRef,
    capturedGlobals: mutable.LinkedHashMap[String, CapturedGlobal] = mutable.LinkedHashMap.empty
  )

  private val usedTypes: mutable.Set[String] = mutable.Set(Defines.Any, Defines.Void)
  private val functionsByName: Map[String, OxFunctionDecl] =
    document.declarations.collect { case function: OxFunctionDecl => function.name -> function }.toMap
  private val macroDeclarations: Seq[OxMacroDecl] =
    document.declarations.collect { case macroDecl: OxMacroDecl => macroDecl }
  private lazy val aggregateDeclarations: Seq[(OxStructDecl, Option[String])] =
    collectAggregateDeclarations(document.declarations, None)
  private lazy val aggregateFieldsByType: Map[String, Map[String, String]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      Seq(localName, fullName).distinct.map { typeName =>
        typeName -> structDecl.fields
          .map(field => field.name -> normalizeType(field.typeName))
          .toMap
      }
    }.toMap
  private val IntegerLiteralPattern = """[+-]?(?:0[xX][0-9a-fA-F]+|\d+)[uUlL]*""".r

  private var scope: Map[String, ScopeEntry]                            = Map.empty
  private var globalLocalEntries: Map[OxGlobalVariableDecl, ScopeEntry] = Map.empty
  private var globalScopeByName: Map[String, ScopeEntry]                = Map.empty
  private var functionCaptureContext: Option[FunctionCaptureContext]    = None
  private var typeAliases: Map[String, String]                          = Map.empty

  def typesSeen(): Set[String] = usedTypes.toSet

  override def createAst(): DiffGraphBuilder = {
    val fileNode = NewFile().name(filename).order(0)
    if (!config.disableFileContent) fileContent.foreach(fileNode.content)

    Ast.storeInDiffGraph(Ast(fileNode).withChild(astForTranslationUnit()), diffGraph)
    diffGraph
  }

  private def astForTranslationUnit(): Ast = {
    initializeGlobalScope()
    initializeTypeAliases()
    val namespaceBlock = globalNamespaceBlock()
    val origin         = OxOrigin(NamespaceTraversal.globalNamespaceName, Option(1))
    val globalTypeDecl =
      typeDeclNode(
        origin,
        NamespaceTraversal.globalNamespaceName,
        namespaceBlock.fullName,
        filename,
        NamespaceTraversal.globalNamespaceName,
        NodeTypes.NAMESPACE_BLOCK,
        namespaceBlock.fullName
      )
    val globalMethod =
      methodNode(
        origin,
        NamespaceTraversal.globalNamespaceName,
        NamespaceTraversal.globalNamespaceName,
        namespaceBlock.fullName,
        None,
        filename,
        Option(NodeTypes.TYPE_DECL),
        Option(namespaceBlock.fullName)
      )
    val globalBlock     = blockNode(origin, NamespaceTraversal.globalNamespaceName, Defines.Any)
    val declarationAsts = document.declarations.flatMap(astForDeclaration)
    val globalMethodAst =
      methodAst(
        globalMethod,
        Seq.empty,
        blockAst(globalBlock, declarationAsts.toList),
        methodReturnNode(origin, Defines.Any)
      )

    val includeAsts = document.declarations.collect { case includeDecl: OxIncludeDecl => astForInclude(includeDecl) }
    Ast(namespaceBlock).withChildren(includeAsts :+ Ast(globalTypeDecl).withChild(globalMethodAst))
  }

  private def fileContent: Option[String] = {
    Try(Files.readString(Paths.get(document.path), StandardCharsets.UTF_8)).toOption
  }

  private def astForDeclaration(declaration: OxDeclaration): Seq[Ast] = {
    declaration match {
      case macroDecl: OxMacroDecl   => Seq(astForMacro(macroDecl))
      case _: OxIncludeDecl         => Seq.empty
      case structDecl: OxStructDecl => Seq(astForStruct(structDecl))
      case enumDecl: OxEnumDecl     => Seq(astForEnum(enumDecl))
      case global: OxGlobalVariableDecl =>
        astsForGlobalVariable(global)
      case typedef: OxTypedefDecl   => Seq(astForTypedef(typedef))
      case function: OxFunctionDecl => astsForFunction(function)
    }
  }

  private def astForInclude(includeDecl: OxIncludeDecl): Ast = {
    val dependency = NewDependency()
      .name(includeDecl.name)
      .dependencyGroupId(includeDecl.name)
      .version("include")
    val importNode = NewImport()
      .code(includeDecl.code)
      .importedEntity(includeDecl.name)
      .importedAs(includeDecl.name)
      .lineNumber(includeDecl.line)
    diffGraph.addNode(dependency)
    diffGraph.addEdge(importNode, dependency, EdgeTypes.IMPORTS)
    Ast(importNode)
  }

  private def astForMacro(macroDecl: OxMacroDecl): Ast = {
    val origin = OxOrigin(macroDecl)
    val params = macroDecl.parameters.zipWithIndex.map { case (name, index) =>
      Ast(
        parameterInNode(origin, name, name, index + 1, isVariadic = false, EvaluationStrategies.BY_VALUE, Defines.Any)
      )
    }
    val returnType = registerType(macroReturnTypeFullName(macroDecl))
    val signature  = macroSignature(macroDecl)
    val method =
      methodNode(origin, macroDecl.name, macroDecl.name, macroFullName(macroDecl), Option(signature), filename)
    val body         = blockAst(blockNode(origin, macroDecl.body, returnType))
    val methodReturn = methodReturnNode(origin, returnType)
    methodAst(method, params, body, methodReturn)
  }

  private def collectAggregateDeclarations(
    declarations: Seq[OxDeclaration],
    parentFullName: Option[String]
  ): Seq[(OxStructDecl, Option[String])] = {
    declarations.flatMap {
      case structDecl: OxStructDecl =>
        val localName = normalizeType(structDecl.name)
        val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
        (structDecl -> parentFullName) +: collectAggregateDeclarations(structDecl.nestedDeclarations, Option(fullName))
      case _ =>
        Seq.empty
    }
  }

  private def astForStruct(structDecl: OxStructDecl): Ast = {
    astForStruct(structDecl, parentTypeFullName = None, parentAstFullName = globalNamespaceBlock().fullName)
  }

  private def astForStruct(
    structDecl: OxStructDecl,
    parentTypeFullName: Option[String],
    parentAstFullName: String
  ): Ast = {
    val origin        = OxOrigin(structDecl)
    val localTypeName = normalizeType(structDecl.name)
    val typeName = registerType(parentTypeFullName.map(parent => s"$parent.$localTypeName").getOrElse(localTypeName))
    val typeDecl =
      typeDeclNode(
        origin,
        structDecl.name,
        typeName,
        filename,
        structDecl.code,
        NodeTypes.NAMESPACE_BLOCK,
        parentAstFullName,
        alias = aggregateAlias(typeName)
      )
    val fieldAsts = structDecl.fields.map { field =>
      Ast(
        memberNode(origin.copy(code = field.code), field.name, field.code, registerType(normalizeType(field.typeName)))
      )
    }
    val nestedAsts = structDecl.nestedDeclarations.flatMap {
      case nestedStruct: OxStructDecl => Seq(astForStruct(nestedStruct, Option(typeName), typeName))
      case nestedEnum: OxEnumDecl     => Seq(astForEnum(nestedEnum, Option(typeName), typeName))
      case _                          => Seq.empty
    }
    Ast(typeDecl).withChildren(fieldAsts ++ nestedAsts)
  }

  private def astForEnum(enumDecl: OxEnumDecl): Ast = {
    astForEnum(enumDecl, parentTypeFullName = None, parentAstFullName = globalNamespaceBlock().fullName)
  }

  private def astForEnum(enumDecl: OxEnumDecl, parentTypeFullName: Option[String], parentAstFullName: String): Ast = {
    val origin        = OxOrigin(enumDecl)
    val localTypeName = normalizeType(enumDecl.name)
    val typeName = registerType(parentTypeFullName.map(parent => s"$parent.$localTypeName").getOrElse(localTypeName))
    val typeDecl =
      typeDeclNode(
        origin,
        enumDecl.name,
        typeName,
        filename,
        enumDecl.code,
        NodeTypes.NAMESPACE_BLOCK,
        parentAstFullName,
        alias = aggregateAlias(typeName)
      )
    val variantAsts = enumDecl.variants.map { variant =>
      Ast(memberNode(OxOrigin(variant.code, Option(variant.line)), variant.name, variant.code, registerType("int")))
    }
    val staticConstructorAst = enumStaticConstructorAst(enumDecl, typeName)
    Ast(typeDecl).withChildren(variantAsts ++ staticConstructorAst.toSeq)
  }

  private def enumStaticConstructorAst(enumDecl: OxEnumDecl, typeFullName: String): Option[Ast] = {
    val initializedVariants = enumDecl.variants.filter(_.value.isDefined)
    Option.when(initializedVariants.nonEmpty) {
      val constructorName = io.joern.x2cpg.Defines.StaticInitMethodName
      val origin          = OxOrigin(constructorName, Option(enumDecl.line))
      val method =
        methodNode(
          origin,
          constructorName,
          constructorName,
          s"$typeFullName.$constructorName:$typeFullName()",
          None,
          filename,
          Option(NodeTypes.TYPE_DECL),
          Option(typeFullName)
        )
      val intType = registerType("int")
      val locals = initializedVariants.map { variant =>
        variant -> localNode(OxOrigin(variant.name, Option(variant.line)), variant.name, variant.name, intType)
      }
      val localAsts = locals.map { case (_, local) => Ast(local) }
      val assignmentAsts = locals.map { case (variant, local) =>
        val identifier =
          identifierNode(OxOrigin(variant.name, Option(variant.line)), variant.name, variant.name, intType)
        val left  = Ast(identifier).withRefEdge(identifier, local)
        val value = variant.value.getOrElse("")
        val right = Ast(literalNode(OxOrigin(value, Option(variant.line)), value, literalType(value)))
        assignmentAst(OxOrigin(variant.code, Option(variant.line)), left, right, variant.code)
      }
      val body = blockAst(blockNode(origin, constructorName, Defines.Any), (localAsts ++ assignmentAsts).toList)
      methodAst(
        method,
        Seq.empty,
        body,
        methodReturnNode(origin, typeFullName),
        Seq(NewModifier().modifierType(ModifierTypes.CONSTRUCTOR), NewModifier().modifierType(ModifierTypes.STATIC))
      )
    }
  }

  private def astForTypedef(typedef: OxTypedefDecl): Ast = {
    val origin    = OxOrigin(typedef)
    val name      = registerType(normalizeType(typedef.name))
    val aliasType = registerType(resolveAliasType(typedef.typeName))
    Ast(
      typeDeclNode(
        origin,
        typedef.name,
        name,
        filename,
        typedef.code,
        NodeTypes.NAMESPACE_BLOCK,
        globalNamespaceBlock().fullName,
        alias = Option(aliasType)
      )
    )
  }

  private def initializeTypeAliases(): Unit = {
    var aliases = Map.empty[String, String]
    document.declarations.foreach {
      case typedef: OxTypedefDecl =>
        aliases = aliases.updated(typedef.name, resolveAliasType(typedef.typeName, aliases))
      case _ =>
    }
    typeAliases = aliases
  }

  private def initializeGlobalScope(): Unit = {
    val globalEntries = document.declarations.collect { case global: OxGlobalVariableDecl =>
      val localCode = localCodeForGlobal(global)
      val typeName  = registerType(normalizeType(global.typeName))
      val node      = localNode(OxOrigin(global).copy(code = localCode), global.name, localCode, typeName)
      global -> (typeName, node)
    }
    globalLocalEntries = globalEntries.map { case (global, (typeName, node)) =>
      global -> ScopeEntry(typeName, node)
    }.toMap
    globalScopeByName = globalLocalEntries.map { case (global, scopeEntry) => global.name -> scopeEntry }
  }

  private def astsForGlobalVariable(global: OxGlobalVariableDecl): Seq[Ast] = {
    val origin    = OxOrigin(global)
    val localCode = localCodeForGlobal(global)
    val scopeEntry = globalLocalEntries.getOrElse(
      global, {
        val typeName = registerType(normalizeType(global.typeName))
        ScopeEntry(typeName, this.localNode(origin.copy(code = localCode), global.name, localCode, typeName))
      }
    )
    val localAst = Ast(scopeEntry.declaration)
    global.initializer match {
      case Some(initializer) =>
        val assignmentCode = s"${global.name} = ${initializer.code}"
        val left           = identifierAstForScopeEntry(global.name, global.name, global.line, scopeEntry)
        val assignment =
          assignmentAst(origin.copy(code = assignmentCode), left, expressionAst(initializer), assignmentCode)
        Seq(localAst, assignment)
      case None =>
        Seq(localAst)
    }
  }

  private def localCodeForGlobal(global: OxGlobalVariableDecl): String = {
    global.initializer.fold(global.code)(_ => global.code.takeWhile(_ != '=').trim)
  }

  private def astsForFunction(function: OxFunctionDecl): Seq[Ast] = {
    val origin     = OxOrigin(function)
    val returnType = registerType(normalizeType(function.returnType))
    val method =
      methodNode(origin, function.name, function.name, function.name, Option(function.signature), filename)
        .isExternal(!function.isDefinition)
    val parameters = function.parameters.zipWithIndex.map { case (parameter, index) =>
      val parameterType = registerType(normalizeType(parameter.typeName))
      val parameterNode =
        parameterInNode(
          OxOrigin(parameter.code, Option(parameter.line)),
          parameter.name,
          parameter.code,
          index + 1,
          isVariadic = false,
          EvaluationStrategies.BY_VALUE,
          parameterType
        )
      parameter.name -> (parameterType, Ast(parameterNode), parameterNode)
    }

    val previousScope          = scope
    val previousCaptureContext = functionCaptureContext
    val captureContext =
      FunctionCaptureContext(function, methodRefNode(origin, function.name, function.name, function.name))
    scope = parameters.map { case (name, (typeName, _, node)) => name -> ScopeEntry(typeName, node) }.toMap
    functionCaptureContext = Option(captureContext)
    val bodyAsts =
      try function.body.flatMap(astsForStatement)
      finally {
        functionCaptureContext = previousCaptureContext
        scope = previousScope
      }
    val captureLocalAsts =
      captureContext.capturedGlobals.values.map(capture => Ast(capture.scopeEntry.declaration)).toSeq
    val body         = blockAst(blockNode(origin, function.code, Defines.Any), (captureLocalAsts ++ bodyAsts).toList)
    val methodReturn = methodReturnNode(origin, returnType)
    val ast          = methodAst(method, parameters.map(_._2._2), body, methodReturn)

    captureAstForFunction(captureContext).fold(Seq(ast))(captureAst => Seq(ast, captureAst))
  }

  private def astsForStatement(statement: OxStatement): Seq[Ast] = {
    statement match {
      case local: OxLocalDecl =>
        val origin    = OxOrigin(local)
        val typeName  = registerType(normalizeType(local.typeName))
        val localCode = local.initializer.fold(local.code)(_ => local.code.takeWhile(_ != '=').trim)
        val localNode = this.localNode(origin.copy(code = localCode), local.name, localCode, typeName)
        scope = scope.updated(local.name, ScopeEntry(typeName, localNode))
        val localAst = Ast(localNode)
        local.initializer match {
          case Some(initializer) =>
            val assignmentCode = s"${local.name} = ${initializer.code}"
            val left           = identifierAst(local.name, local.name, local.line)
            val assignment =
              assignmentAst(origin.copy(code = assignmentCode), left, expressionAst(initializer), assignmentCode)
            Seq(localAst, assignment)
          case None =>
            Seq(localAst)
        }
      case assignment: OxAssignment =>
        val left  = expressionAst(assignment.left)
        val right = expressionAst(assignment.right)
        Seq {
          if (assignment.operator == "=") {
            assignmentAst(OxOrigin(assignment), left, right, assignment.code)
          } else {
            operatorCallAst(OxOrigin(assignment), assignment.code, operatorFor(assignment.operator), Seq(left, right))
          }
        }
      case ret: OxReturn =>
        Seq(returnAst(returnNode(OxOrigin(ret), ret.code), ret.expression.toSeq.map(expressionAst)))
      case ifStmt: OxIf =>
        val ifNode       = controlStructureNode(OxOrigin(ifStmt), ControlStructureTypes.IF, ifStmt.code)
        val conditionAst = expressionAst(ifStmt.condition)
        val thenAst      = statementBlockAst(ifStmt.thenBody, "then", ifStmt.line)
        val elseAst =
          Option.when(ifStmt.elseBody.nonEmpty) {
            Ast(controlStructureNode(OxOrigin("else", Option(ifStmt.line)), ControlStructureTypes.ELSE, "else"))
              .withChild(statementBlockAst(ifStmt.elseBody, "else", ifStmt.line))
          }
        Seq(ifThenElseAst(ifNode, Option(conditionAst), thenAst, elseAst))
      case whileStmt: OxWhile =>
        val bodyAst = statementBlockAst(whileStmt.body, "while", whileStmt.line)
        Seq(
          whileAst(
            Option(expressionAst(whileStmt.condition)),
            Seq(bodyAst),
            code = Option(whileStmt.code),
            lineNumber = Option(whileStmt.line)
          )
        )
      case doWhileStmt: OxDoWhile =>
        Seq(
          doWhileAst(
            Option(expressionAst(doWhileStmt.condition)),
            Seq(statementBlockAst(doWhileStmt.body, "do", doWhileStmt.line)),
            code = Option(doWhileStmt.code),
            lineNumber = Option(doWhileStmt.line)
          )
        )
      case forStmt: OxFor =>
        inNestedScope {
          val forNode               = controlStructureNode(OxOrigin(forStmt), ControlStructureTypes.FOR, forStmt.code)
          val initializerAsts       = forStmt.initializer.flatMap(astsForStatement)
          val (localAsts, initAsts) = initializerAsts.partition(_.root.exists(_.isInstanceOf[NewLocal]))
          Seq(
            forAst(
              forNode,
              localAsts,
              initAsts,
              forStmt.condition.toSeq.map(expressionAst),
              forStmt.update.toSeq.map(expressionAst),
              statementBlockAst(forStmt.body, "for", forStmt.line)
            )
          )
        }
      case breakStmt: OxBreak =>
        Seq(Ast(controlStructureNode(OxOrigin(breakStmt), ControlStructureTypes.BREAK, breakStmt.code)))
      case continueStmt: OxContinue =>
        Seq(Ast(controlStructureNode(OxOrigin(continueStmt), ControlStructureTypes.CONTINUE, continueStmt.code)))
      case gotoStmt: OxGoto =>
        Seq(Ast(controlStructureNode(OxOrigin(gotoStmt), ControlStructureTypes.GOTO, gotoStmt.code)))
      case labelStmt: OxLabel =>
        Ast(jumpTargetNode(OxOrigin(labelStmt), labelStmt.label, labelStmt.code)) +:
          labelStmt.body.flatMap(astsForStatement)
      case switchStmt: OxSwitch =>
        val switchNode = controlStructureNode(OxOrigin(switchStmt), ControlStructureTypes.SWITCH, switchStmt.code)
        Seq(
          switchAst(
            switchNode,
            expressionAst(switchStmt.condition),
            inNestedScope {
              switchStmt.body.flatMap(astsForStatement)
            }
          )
        )
      case caseStmt: OxCase =>
        val name = if (caseStmt.value.isDefined) "case" else "default"
        Ast(jumpTargetNode(OxOrigin(caseStmt), name, caseStmt.code)) +:
          (caseStmt.value.toSeq.map(expressionAst) ++ caseStmt.body.flatMap(astsForStatement))
      case expressionStatement: OxExpressionStatement =>
        Seq(expressionAst(expressionStatement.expression))
    }
  }

  private def statementBlockAst(statements: Seq[OxStatement], code: String, line: Int): Ast = {
    inNestedScope {
      blockAst(blockNode(OxOrigin(code, Option(line)), code, Defines.Any), statements.flatMap(astsForStatement).toList)
    }
  }

  private def inNestedScope[T](body: => T): T = {
    val outerScope = scope
    try body
    finally scope = outerScope
  }

  private def expressionAst(expression: OxExpression): Ast = {
    expression match {
      case identifier: OxIdentifier =>
        objectLikeMacroAst(identifier).getOrElse(identifierAst(identifier.name, identifier.code, identifier.line))
      case literal: OxLiteral =>
        Ast(literalNode(OxOrigin(literal), literal.code, literalType(literal.value)))
      case binary: OxBinary =>
        operatorCallAst(
          OxOrigin(binary),
          binary.code,
          operatorFor(binary.operator),
          Seq(expressionAst(binary.left), expressionAst(binary.right))
        )
      case unary: OxUnary =>
        operatorCallAst(
          OxOrigin(unary),
          unary.code,
          unaryOperatorFor(unary.operator, unary.prefix),
          Seq(expressionAst(unary.argument))
        )
      case conditional: OxConditional =>
        operatorCallAst(
          OxOrigin(conditional),
          conditional.code,
          Operators.conditional,
          Seq(expressionAst(conditional.condition)) ++
            conditional.consequence.toSeq.map(expressionAst) ++
            Seq(expressionAst(conditional.alternative))
        )
      case cast: OxCast =>
        operatorCallAst(
          OxOrigin(cast),
          cast.code,
          Operators.cast,
          Seq(expressionAst(cast.value)),
          typeFullName = registerType(normalizeType(cast.typeName))
        )
      case sizeOf: OxSizeOf =>
        val operand = sizeOf.value.map(expressionAst).orElse {
          sizeOf.typeName.map { typeName =>
            Ast(literalNode(OxOrigin(typeName, Option(sizeOf.line)), typeName, registerType(Defines.Any)))
          }
        }
        operatorCallAst(OxOrigin(sizeOf), sizeOf.code, Operators.sizeOf, operand.toSeq)
      case call: OxCall =>
        astForCallExpression(call)
      case fieldAccess: OxFieldAccess =>
        fieldAccessAst(
          OxOrigin(fieldAccess),
          OxOrigin(fieldIdentifierCode(fieldAccess), Option(fieldAccess.line)),
          expressionAst(fieldAccess.base),
          fieldAccess.code,
          fieldAccess.field,
          registerType(expressionTypeFullName(fieldAccess).getOrElse(Defines.Any))
        )
      case indexAccess: OxIndexAccess =>
        val operatorName = Operators.indirectIndexAccess
        operatorCallAst(
          OxOrigin(indexAccess),
          indexAccess.code,
          operatorName,
          Seq(expressionAst(indexAccess.base), expressionAst(indexAccess.index))
        )
      case initializerList: OxInitializerList =>
        operatorCallAst(
          OxOrigin(initializerList),
          initializerList.code,
          Operators.arrayInitializer,
          initializerList.elements.map(expressionAst)
        )
      case designatedInitializer: OxDesignatedInitializer =>
        assignmentAst(
          OxOrigin(designatedInitializer),
          expressionAst(designatedInitializer.designator),
          expressionAst(designatedInitializer.value),
          designatedInitializer.code
        )
      case designator: OxDesignator =>
        Ast(identifierNode(OxOrigin(designator), designator.name, designator.code, registerType(Defines.Any)))
    }
  }

  private def astForCallExpression(call: OxCall): Ast = {
    if (isPointerCall(call)) pointerCallAst(call) else directCallAst(call)
  }

  private def directCallAst(call: OxCall): Ast = {
    val (methodFullName, signature, typeFullName, dispatchType) = callTargetInfo(call.name, call.line)
    val callNode_ =
      callNode(
        OxOrigin(call),
        call.code,
        call.name,
        methodFullName,
        dispatchType,
        signature,
        Option(registerType(typeFullName))
      )
    callAst(callNode_, call.arguments.map(expressionAst))
  }

  private def pointerCallAst(call: OxCall): Ast = {
    val callNode_ =
      callNode(
        OxOrigin(call),
        call.code,
        Defines.OperatorPointerCall,
        Defines.OperatorPointerCall,
        DispatchTypes.DYNAMIC_DISPATCH,
        None,
        Option(registerType(callReturnTypeFullName(call).getOrElse(Defines.Any)))
      )
    callAst(callNode_, call.arguments.map(expressionAst), receiver = Option(expressionAst(call.callee)))
  }

  private def isPointerCall(call: OxCall): Boolean = {
    call.callee match {
      case _: OxFieldAccess             => true
      case OxUnary("*", _, _, _, _)     => true
      case _: OxUnary                   => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxIdentifier | _: OxCast  => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxCall | _: OxIndexAccess => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxInitializerList         => false
      case _: OxDesignatedInitializer   => false
      case _: OxDesignator              => false
      case _: OxLiteral                 => false
      case _: OxBinary | _: OxConditional | _: OxSizeOf => false
    }
  }

  private def callReturnTypeFullName(call: OxCall): Option[String] = {
    expressionTypeFullName(call.callee).flatMap(returnTypeFromFunctionPointer)
  }

  private def expressionTypeFullName(expression: OxExpression): Option[String] = {
    expression match {
      case OxIdentifier(name, _, _) =>
        scope.get(name).orElse(globalScopeByName.get(name)).map(entry => resolveAliasType(entry.typeFullName))
      case OxFieldAccess(field, _, _, base) =>
        expressionTypeFullName(base).flatMap(typeName => fieldTypeFullName(typeName, field))
      case OxUnary("*", _, _, _, argument) =>
        expressionTypeFullName(argument)
      case OxCast(typeName, _, _, _) =>
        Option(resolveAliasType(typeName))
      case OxIndexAccess(_, _, base, _) =>
        expressionTypeFullName(base).map(_.stripSuffix("[]"))
      case call: OxCall =>
        callReturnTypeFullName(call)
      case _ =>
        None
    }
  }

  private def fieldTypeFullName(baseTypeFullName: String, field: String): Option[String] = {
    val normalized = resolveAliasType(baseTypeFullName)
    val candidates = Seq(normalized, normalized.stripSuffix("*"), normalized.stripSuffix("[]")).distinct
    candidates.collectFirst(Function.unlift { typeName =>
      aggregateFieldsByType
        .get(normalizeType(typeName))
        .flatMap(_.get(field))
        .map(fieldType => resolveAliasType(fieldType))
    })
  }

  private def returnTypeFromFunctionPointer(typeFullName: String): Option[String] = {
    val markerIndex = typeFullName.indexOf("(*")
    Option.when(markerIndex > 0)(typeFullName.take(markerIndex))
  }

  private def isFunctionPointerType(typeFullName: String): Boolean = {
    returnTypeFromFunctionPointer(typeFullName).isDefined
  }

  private def fieldIdentifierCode(fieldAccess: OxFieldAccess): String = {
    if (fieldAccess.code.contains("->")) fieldAccess.code.split("->").lastOption.getOrElse(fieldAccess.field)
    else fieldAccess.code.split('.').lastOption.getOrElse(fieldAccess.field)
  }

  private def operatorCallAst(
    origin: OxOrigin,
    code: String,
    operatorName: String,
    arguments: Seq[Ast],
    typeFullName: String = registerType(Defines.Any)
  ): Ast = {
    val call =
      callNode(
        origin.copy(code = code),
        code,
        operatorName,
        operatorName,
        DispatchTypes.STATIC_DISPATCH,
        Option(""),
        Option(typeFullName)
      )
    callAst(call, arguments)
  }

  private def assignmentAst(origin: OxOrigin, left: Ast, right: Ast, code: String): Ast = {
    val call =
      callNode(
        origin.copy(code = code),
        code,
        Operators.assignment,
        Operators.assignment,
        DispatchTypes.STATIC_DISPATCH,
        Option(""),
        Option(registerType(Defines.Void))
      )
    callAst(call, Seq(left, right))
  }

  private def identifierAst(name: String, code: String, line: Int): Ast = {
    scope.get(name) match {
      case Some(entry) =>
        val typeName   = registerType(entry.typeFullName)
        val identifier = identifierNode(OxOrigin(code, Option(line)), name, code, typeName)
        Ast(identifier).withRefEdge(identifier, entry.declaration)
      case None =>
        capturedGlobalIdentifierAst(name, code, line)
          .orElse(methodRefAst(name, code, line))
          .getOrElse {
            val identifier = identifierNode(OxOrigin(code, Option(line)), name, code, registerType(Defines.Any))
            Ast(identifier)
          }
    }
  }

  private def methodRefAst(name: String, code: String, line: Int): Option[Ast] = {
    functionsByName.get(name).map { function =>
      Ast(
        methodRefNode(
          OxOrigin(code, Option(line)),
          code,
          function.name,
          registerType(normalizeType(function.returnType))
        )
      )
    }
  }

  private def identifierAstForScopeEntry(name: String, code: String, line: Int, scopeEntry: ScopeEntry): Ast = {
    val typeName   = registerType(scopeEntry.typeFullName)
    val identifier = identifierNode(OxOrigin(code, Option(line)), name, code, typeName)
    Ast(identifier).withRefEdge(identifier, scopeEntry.declaration)
  }

  private def capturedGlobalIdentifierAst(name: String, code: String, line: Int): Option[Ast] = {
    for {
      context     <- functionCaptureContext
      globalEntry <- globalScopeByName.get(name)
    } yield {
      val capture = context.capturedGlobals.getOrElseUpdate(
        name, {
          val closureBindingId = s"$filename:${context.function.name}:$name"
          val localCode        = s"${Defines.GlobalTag} $name"
          val capturedLocal =
            localNode(OxOrigin(localCode, Option(line)), name, localCode, registerType(globalEntry.typeFullName))
              .closureBindingId(closureBindingId)
          val binding = NewClosureBinding()
            .closureBindingId(closureBindingId)
            .evaluationStrategy(EvaluationStrategies.BY_REFERENCE)
          CapturedGlobal(ScopeEntry(globalEntry.typeFullName, capturedLocal), binding, globalEntry)
        }
      )
      identifierAstForScopeEntry(name, code, line, capture.scopeEntry)
    }
  }

  private def captureAstForFunction(context: FunctionCaptureContext): Option[Ast] = {
    Option.when(context.capturedGlobals.nonEmpty) {
      context.capturedGlobals.values.foldLeft(Ast(context.methodRef)) { case (ast, capture) =>
        ast
          .withCaptureEdge(context.methodRef, capture.binding)
          .merge(Ast(capture.binding).withRefEdge(capture.binding, capture.globalEntry.declaration))
      }
    }
  }

  private def objectLikeMacroAst(identifier: OxIdentifier): Option[Ast] = {
    macroForUse(identifier.name, identifier.line)
      .filter(_.parameters.isEmpty)
      .map { macroDecl =>
        val typeFullName = macroReturnTypeFullName(macroDecl)
        val callNode_ =
          callNode(
            OxOrigin(identifier),
            identifier.code,
            identifier.name,
            macroFullName(macroDecl),
            DispatchTypes.INLINED,
            Option(macroSignature(macroDecl)),
            Option(registerType(typeFullName))
          )
        callAst(callNode_, Seq.empty)
      }
  }

  private def callTargetInfo(name: String, line: Int): (String, Option[String], String, String) = {
    macroForUse(name, line) match {
      case Some(macroDecl) =>
        (
          macroFullName(macroDecl),
          Option(macroSignature(macroDecl)),
          macroReturnTypeFullName(macroDecl),
          DispatchTypes.INLINED
        )
      case None =>
        functionsByName.get(name) match {
          case Some(function) =>
            (
              function.name,
              Option(function.signature),
              normalizeType(function.returnType),
              DispatchTypes.STATIC_DISPATCH
            )
          case None =>
            (name, None, Defines.Any, DispatchTypes.STATIC_DISPATCH)
        }
    }
  }

  private def macroForUse(name: String, line: Int): Option[OxMacroDecl] = {
    macroDeclarations.filter(macroDecl => macroDecl.name == name && macroDecl.line <= line).lastOption
  }

  private def macroFullName(macroDecl: OxMacroDecl): String = {
    s"$filename:${macroDecl.name}:${macroSignature(macroDecl)}"
  }

  private def macroSignature(macroDecl: OxMacroDecl): String = {
    s"${macroReturnTypeFullName(macroDecl)}(${macroDecl.parameters.size})"
  }

  private def macroReturnTypeFullName(macroDecl: OxMacroDecl): String = {
    if (macroDecl.parameters.isEmpty && isIntegerLiteral(macroDecl.body)) "int" else Defines.Any
  }

  private def literalType(value: String): String = {
    if (isIntegerLiteral(value)) registerType("int") else registerType(Defines.Any)
  }

  private def isIntegerLiteral(value: String): Boolean = {
    IntegerLiteralPattern.pattern.matcher(value.trim).matches()
  }

  private def operatorFor(operator: String): String = {
    operator match {
      case "+"   => Operators.addition
      case "-"   => Operators.subtraction
      case "*"   => Operators.multiplication
      case "/"   => Operators.division
      case "%"   => Operators.modulo
      case "<"   => Operators.lessThan
      case ">"   => Operators.greaterThan
      case "<="  => Operators.lessEqualsThan
      case ">="  => Operators.greaterEqualsThan
      case "=="  => Operators.equals
      case "!="  => Operators.notEquals
      case "&&"  => Operators.logicalAnd
      case "||"  => Operators.logicalOr
      case "&"   => Operators.and
      case "|"   => Operators.or
      case "^"   => Operators.xor
      case "<<"  => Operators.shiftLeft
      case ">>"  => Operators.arithmeticShiftRight
      case "="   => Operators.assignment
      case "+="  => Operators.assignmentPlus
      case "-="  => Operators.assignmentMinus
      case "*="  => Operators.assignmentMultiplication
      case "/="  => Operators.assignmentDivision
      case "%="  => Operators.assignmentModulo
      case "<<=" => Operators.assignmentShiftLeft
      case ">>=" => Operators.assignmentArithmeticShiftRight
      case "&="  => Operators.assignmentAnd
      case "^="  => Operators.assignmentXor
      case "|="  => Operators.assignmentOr
      case _     => Defines.OperatorUnknown
    }
  }

  private def unaryOperatorFor(operator: String, prefix: Boolean): String = {
    operator match {
      case "++" if prefix => Operators.preIncrement
      case "++"           => Operators.postIncrement
      case "--" if prefix => Operators.preDecrement
      case "--"           => Operators.postDecrement
      case "+"            => Operators.plus
      case "-"            => Operators.minus
      case "*"            => Operators.indirection
      case "&"            => Operators.addressOf
      case "~"            => Operators.not
      case "!"            => Operators.logicalNot
      case _              => Defines.OperatorUnknown
    }
  }

  private def normalizeType(typeName: String): String = {
    typeName
      .stripPrefix("struct ")
      .stripPrefix("union ")
      .stripPrefix("enum ")
      .trim
  }

  private def resolveAliasType(typeName: String, aliases: Map[String, String] = typeAliases): String = {
    val normalized = normalizeType(typeName)
    if (normalized.endsWith("*") && normalized.length > 1) {
      s"${resolveAliasType(normalized.dropRight(1), aliases)}*"
    } else if (normalized.endsWith("[]") && normalized.length > 2) {
      s"${resolveAliasType(normalized.dropRight(2), aliases)}[]"
    } else {
      aliases.getOrElse(normalized, normalized)
    }
  }

  private def aggregateAlias(typeName: String): Option[String] = {
    document.declarations.collectFirst {
      case typedef: OxTypedefDecl if resolveAliasType(typedef.typeName) == typeName => registerType(typedef.name)
    }
  }

  private def registerType(typeName: String): String = {
    val normalized = if (typeName.isBlank) Defines.Any else typeName
    usedTypes.add(normalized)
    normalized
  }

  override protected def code(node: OxOrigin): String = shortenCode(node.code)

  override protected def line(node: OxOrigin): Option[Int] = node.line

  override protected def column(node: OxOrigin): Option[Int] = None

  override protected def lineEnd(node: OxOrigin): Option[Int] = node.line

  override protected def columnEnd(element: OxOrigin): Option[Int] = None

}
