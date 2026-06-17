package io.joern.c2cpg.oxidized

import io.joern.c2cpg.Config
import io.joern.c2cpg.astcreation.Defines
import io.joern.c2cpg.parser.FileDefaults
import io.joern.x2cpg.{Ast, AstCreatorBase, SourceFiles, ValidationMode}
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
import java.util.IdentityHashMap
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
  private val LambdaMutableModifier                     = "MUTABLE"

  private final case class LambdaInfo(name: String, fullName: String, signature: String, returnType: String)
  private final case class ScopeEntry(typeFullName: String, declaration: NewNode, lambdaInfo: Option[LambdaInfo] = None)
  private final case class CapturedGlobal(scopeEntry: ScopeEntry, binding: NewClosureBinding, globalEntry: ScopeEntry)
  private final case class FunctionEntry(
    function: OxFunctionDecl,
    lexicalOwnerFullName: Option[String],
    ownerFullName: Option[String],
    simpleName: String,
    qualifiedName: String,
    fullName: String
  )
  private final case class ResolvedOperatorCall(
    entry: FunctionEntry,
    name: String,
    base: Option[OxExpression],
    arguments: Seq[OxExpression]
  )
  private final case class LocalDestructor(receiverCode: String, line: Int, entry: FunctionEntry)
  private final case class TemporaryDestructor(code: String, line: Int, entry: FunctionEntry)
  private final case class HeapConstructor(code: String, line: Int, entry: FunctionEntry, arguments: Seq[OxExpression])
  private final case class HeapDestructor(code: String, line: Int, entry: FunctionEntry, receiver: OxExpression)
  private final case class LambdaCaptureRequest(
    name: String,
    evaluationStrategy: String,
    initializer: Option[OxExpression]
  )
  private final case class LambdaCapture(
    name: String,
    scopeEntry: ScopeEntry,
    binding: NewClosureBinding,
    outer: Option[ScopeEntry],
    evaluationStrategy: String
  )
  private final case class JumpCleanupTarget(
    breakPreservedScopeDepth: Option[Int],
    continuePreservedScopeDepth: Option[Int],
    throwPreservedScopeDepth: Option[Int] = None
  )
  private final case class ArgumentInfo(typeFullName: Option[String], isRvalue: Boolean)
  private final case class FunctionCaptureContext(
    function: OxFunctionDecl,
    methodRef: NewMethodRef,
    capturedGlobals: mutable.LinkedHashMap[String, CapturedGlobal] = mutable.LinkedHashMap.empty
  )

  private val usedTypes: mutable.Set[String]             = mutable.Set(Defines.Any, Defines.Void)
  private val temporaryIndices: mutable.Map[String, Int] = mutable.HashMap.empty
  private val expressionTypeFullNameCache                = new IdentityHashMap[OxExpression, Option[String]]
  private lazy val functionEntries: Seq[FunctionEntry]   = collectFunctionEntries(document.declarations, None)
  private lazy val functionsByName: Map[String, Seq[FunctionEntry]] =
    functionEntries.groupBy(_.simpleName).view.mapValues(_.toSeq).toMap
  private lazy val functionsByQualifiedName: Map[String, Seq[FunctionEntry]] =
    functionEntries.groupBy(_.qualifiedName).view.mapValues(_.toSeq).toMap
  private val macroDeclarations: Seq[OxMacroDecl] =
    document.declarations.collect { case macroDecl: OxMacroDecl => macroDecl }
  private val macroUndefs: Seq[OxMacroUndefDecl] =
    document.declarations.collect { case macroUndef: OxMacroUndefDecl => macroUndef }
  private lazy val aggregateDeclarations: Seq[(OxStructDecl, Option[String])] =
    collectAggregateDeclarations(document.declarations, None)
  private lazy val aggregateTypeFullNames: Set[String] =
    aggregateDeclarations.map { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
    }.toSet
  private lazy val aggregateDeclarationsByType: Map[String, OxStructDecl] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      Seq(localName, fullName).distinct.map(typeName => typeName -> structDecl)
    }.toMap
  private lazy val requiredImplicitDefaultConstructorTypes: Set[String] =
    collectRequiredImplicitDefaultConstructorTypes(document.declarations, None)
  private lazy val outOfClassFunctionsByOwner: Map[String, Seq[FunctionEntry]] =
    functionEntries
      .filter(entry =>
        entry.function.name.contains("::") && entry.ownerFullName.exists(aggregateTypeFullNames.contains)
      )
      .groupBy(_.ownerFullName.get)
  private lazy val aggregateFieldEntriesByType: Map[String, Map[String, OxFieldDecl]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      Seq(localName, fullName).distinct.map { typeName =>
        typeName -> structDecl.fields.map(field => field.name -> field).toMap
      }
    }.toMap
  private lazy val aggregateFieldsByType: Map[String, Seq[OxFieldDecl]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      Seq(localName, fullName).distinct.map(typeName => typeName -> structDecl.fields)
    }.toMap
  private lazy val aggregateBaseTypesByType: Map[String, Seq[String]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      val baseTypes = structDecl.baseClasses.map(baseClass => resolveBaseTypeFullName(baseClass, parentFullName))
      Seq(localName, fullName).distinct.map(typeName => typeName -> baseTypes)
    }.toMap
  private val IntegerLiteralPattern = """[+-]?(?:0[xX][0-9a-fA-F]+|\d+)[uUlL]*""".r
  private val FloatingLiteralPattern =
    """[+-]?(?:(?:\d+\.\d*|\.\d+)(?:[eE][+-]?\d+)?|\d+[eE][+-]?\d+)[fFlL]?""".r
  private val CxxOverloadableBinaryOperators = Set(
    "+",
    "-",
    "*",
    "/",
    "%",
    "<",
    ">",
    "<=",
    ">=",
    "==",
    "!=",
    "&&",
    "||",
    "&",
    "|",
    "^",
    "<<",
    ">>",
    "=",
    "+=",
    "-=",
    "*=",
    "/=",
    "%=",
    "<<=",
    ">>=",
    "&=",
    "^=",
    "|="
  )

  private var scope: Map[String, ScopeEntry]                            = Map.empty
  private var globalLocalEntries: Map[OxGlobalVariableDecl, ScopeEntry] = Map.empty
  private var globalScopeByName: Map[String, ScopeEntry]                = Map.empty
  private var functionCaptureContext: Option[FunctionCaptureContext]    = None
  private var currentMethodOwnerTypeFullName: Option[String]            = None
  private var currentMethodFullName: Option[String]                     = None
  private var currentMethodReturnTypeFullName: Option[String]           = None
  private var typeAliases: Map[String, String]                          = Map.empty
  private var localDestructorScopes: List[Vector[LocalDestructor]]      = Nil
  private var jumpCleanupTargets: List[JumpCleanupTarget]               = Nil
  private val lambdaInfos: mutable.LinkedHashMap[String, LambdaInfo]    = mutable.LinkedHashMap.empty
  private val emittedLambdaFullNames: mutable.Set[String]               = mutable.Set.empty
  private val lambdaReturnTypesByFullName: mutable.Map[String, String]  = mutable.Map.empty
  private val lambdaSignaturesByFullName: mutable.Map[String, String]   = mutable.Map.empty

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
    val globalBlock   = blockNode(origin, NamespaceTraversal.globalNamespaceName, Defines.Any)
    val namespaceAsts = document.declarations.collect { case namespace: OxNamespaceDecl => astForNamespace(namespace) }
    val declarationAsts = document.declarations.flatMap(astForDeclaration)
    val globalMethodAst =
      methodAst(
        globalMethod,
        Seq.empty,
        blockAst(globalBlock, declarationAsts.toList),
        methodReturnNode(origin, Defines.Any)
      )

    val includeAsts = document.declarations.collect { case includeDecl: OxIncludeDecl => astForInclude(includeDecl) }
    Ast(namespaceBlock).withChildren(includeAsts ++ namespaceAsts :+ Ast(globalTypeDecl).withChild(globalMethodAst))
  }

  private def fileContent: Option[String] = {
    Try(Files.readString(Paths.get(document.path), StandardCharsets.UTF_8)).toOption
  }

  private def astForDeclaration(declaration: OxDeclaration): Seq[Ast] = {
    astForDeclaration(declaration, ownerFullName = None, parentAstFullName = globalNamespaceBlock().fullName)
  }

  private def astForDeclaration(
    declaration: OxDeclaration,
    ownerFullName: Option[String],
    parentAstFullName: String
  ): Seq[Ast] = {
    declaration match {
      case macroDecl: OxMacroDecl   => Seq(astForMacro(macroDecl))
      case _: OxMacroUndefDecl      => Seq.empty
      case _: OxIncludeDecl         => Seq.empty
      case _: OxNamespaceDecl       => Seq.empty
      case structDecl: OxStructDecl => Seq(astForStruct(structDecl, ownerFullName, parentAstFullName))
      case enumDecl: OxEnumDecl     => Seq(astForEnum(enumDecl, ownerFullName, parentAstFullName))
      case global: OxGlobalVariableDecl =>
        astsForGlobalVariable(global)
      case typedef: OxTypedefDecl => Seq(astForTypedef(typedef, ownerFullName, parentAstFullName))
      case function: OxFunctionDecl if isOutOfClassAggregateFunction(function, ownerFullName) =>
        Seq.empty
      case function: OxFunctionDecl =>
        astsForFunction(function, ownerFullName, NodeTypes.NAMESPACE_BLOCK, parentAstFullName)
    }
  }

  private def astForNamespace(namespaceDecl: OxNamespaceDecl): Ast = {
    astForNamespace(namespaceDecl, parentOwnerFullName = None)
  }

  private def astForNamespace(namespaceDecl: OxNamespaceDecl, parentOwnerFullName: Option[String]): Ast = {
    val localPath = namespacePath(namespaceDecl.name)
    val localName = localPath.lastOption.getOrElse(namespaceDecl.name)
    val ownerFullName = parentOwnerFullName
      .map(parent => (parent +: localPath).mkString("."))
      .getOrElse(localPath.mkString("."))
    val filename = declarationFilename(namespaceDecl)
    val namespaceBlock =
      namespaceBlockNode(OxOrigin(namespaceDecl), localName, s"$filename:$ownerFullName", filename)
        .code(namespaceDecl.code)
    val childAsts = namespaceDecl.declarations.flatMap {
      case nestedNamespace: OxNamespaceDecl => Seq(astForNamespace(nestedNamespace, Option(ownerFullName)))
      case declaration => astForDeclaration(declaration, Option(ownerFullName), namespaceBlock.fullName)
    }
    Ast(namespaceBlock).withChild(
      blockAst(blockNode(OxOrigin(namespaceDecl), namespaceDecl.code, Defines.Any), childAsts.toList)
    )
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
      methodNode(
        origin,
        macroDecl.name,
        macroDecl.name,
        macroFullName(macroDecl),
        Option(signature),
        macroFilename(macroDecl)
      )
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
      case namespaceDecl: OxNamespaceDecl =>
        val namespaceFullName = parentFullName
          .map(parent => s"$parent.${namespacePath(namespaceDecl.name).mkString(".")}")
          .orElse(Option(namespacePath(namespaceDecl.name).mkString(".")))
        collectAggregateDeclarations(namespaceDecl.declarations, namespaceFullName)
      case _ =>
        Seq.empty
    }
  }

  private def collectFunctionEntries(
    declarations: Seq[OxDeclaration],
    ownerFullName: Option[String]
  ): Seq[FunctionEntry] = {
    declarations.flatMap {
      case functionDecl: OxFunctionDecl =>
        val owner      = functionOwnerFullName(functionDecl, ownerFullName)
        val simpleName = functionSimpleName(functionDecl)
        val qualified  = owner.map(parent => s"$parent.$simpleName").getOrElse(simpleName)
        val fullName   = functionFullName(functionDecl, ownerFullName)
        Seq(FunctionEntry(functionDecl, ownerFullName, owner, simpleName, qualified, fullName))
      case structDecl: OxStructDecl =>
        val localName = normalizeType(structDecl.name)
        val owner     = ownerFullName.map(parent => s"$parent.$localName").getOrElse(localName)
        collectFunctionEntries(structDecl.nestedDeclarations, Option(owner))
      case namespaceDecl: OxNamespaceDecl =>
        val namespaceOwner = ownerFullName
          .map(parent => (parent +: namespacePath(namespaceDecl.name)).mkString("."))
          .orElse(Option(namespacePath(namespaceDecl.name).mkString(".")))
        collectFunctionEntries(namespaceDecl.declarations, namespaceOwner)
      case _ => Seq.empty
    }
  }

  private def collectRequiredImplicitDefaultConstructorTypes(
    declarations: Seq[OxDeclaration],
    ownerFullName: Option[String]
  ): Set[String] = {
    declarations.flatMap {
      case functionDecl: OxFunctionDecl =>
        collectRequiredImplicitDefaultConstructorTypesFromStatements(
          functionDecl.body,
          functionOwnerFullName(functionDecl, ownerFullName)
        )
      case structDecl: OxStructDecl =>
        val localName       = normalizeType(structDecl.name)
        val structFullName  = ownerFullName.map(parent => s"$parent.$localName").getOrElse(localName)
        val nestedOwnerName = Option(structFullName)
        collectRequiredImplicitDefaultConstructorTypes(structDecl.nestedDeclarations, nestedOwnerName)
      case namespaceDecl: OxNamespaceDecl =>
        val namespaceOwner = ownerFullName
          .map(parent => (parent +: namespacePath(namespaceDecl.name)).mkString("."))
          .orElse(Option(namespacePath(namespaceDecl.name).mkString(".")))
        collectRequiredImplicitDefaultConstructorTypes(namespaceDecl.declarations, namespaceOwner)
      case _ =>
        Set.empty[String]
    }.toSet
  }

  private def collectRequiredImplicitDefaultConstructorTypesFromStatements(
    statements: Seq[OxStatement],
    ownerFullName: Option[String]
  ): Set[String] = {
    statements
      .flatMap(statement => collectRequiredImplicitDefaultConstructorTypesFromStatement(statement, ownerFullName))
      .toSet
  }

  private def collectRequiredImplicitDefaultConstructorTypesFromStatement(
    statement: OxStatement,
    ownerFullName: Option[String]
  ): Set[String] = {
    statement match {
      case local: OxLocalDecl =>
        requiredImplicitDefaultConstructorType(local, ownerFullName).toSet ++
          local.initializer.toSet.flatMap(
            collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)
          )
      case OxStructuredBinding(_, _, _, _, _, initializer) =>
        initializer.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
      case OxAssignment(_, _, _, left, right) =>
        Seq(left, right).flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)).toSet
      case OxReturn(_, _, expression) =>
        expression.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
      case OxThrow(_, _, expression) =>
        expression.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
      case OxTry(_, _, body, catches) =>
        collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName) ++
          catches.flatMap(catchClause =>
            collectRequiredImplicitDefaultConstructorTypesFromStatements(catchClause.body, ownerFullName)
          )
      case OxIf(_, _, initializer, conditionInitializer, condition, thenBody, elseBody) =>
        collectRequiredImplicitDefaultConstructorTypesFromStatements(initializer, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(conditionInitializer, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromExpression(condition, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(thenBody, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(elseBody, ownerFullName)
      case OxWhile(_, _, initializer, conditionInitializer, condition, body) =>
        collectRequiredImplicitDefaultConstructorTypesFromStatements(initializer, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(conditionInitializer, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromExpression(condition, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName)
      case OxDoWhile(_, _, condition, body) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(condition, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName)
      case OxFor(_, _, initializer, condition, update, body) =>
        collectRequiredImplicitDefaultConstructorTypesFromStatements(initializer, ownerFullName) ++
          condition.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)) ++
          update.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName)
      case OxLabel(_, _, _, body) =>
        collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName)
      case OxSwitch(_, _, initializer, conditionInitializer, condition, body) =>
        collectRequiredImplicitDefaultConstructorTypesFromStatements(initializer, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(conditionInitializer, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromExpression(condition, ownerFullName) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName)
      case OxCase(_, _, value, body) =>
        value.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)) ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName)
      case OxExpressionStatement(_, _, expression) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(expression, ownerFullName)
      case _: OxUnknownStatement | _: OxUsingEnumStatement | _: OxBreak | _: OxContinue | _: OxGoto =>
        Set.empty
    }
  }

  private def collectRequiredImplicitDefaultConstructorTypesFromExpression(
    expression: OxExpression,
    ownerFullName: Option[String]
  ): Set[String] = {
    expression match {
      case OxBinary(_, _, _, left, right) =>
        Seq(left, right).flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)).toSet
      case OxUnary(_, _, _, _, argument) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(argument, ownerFullName)
      case OxConditional(_, _, condition, consequence, alternative) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(condition, ownerFullName) ++
          consequence.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)) ++
          collectRequiredImplicitDefaultConstructorTypesFromExpression(alternative, ownerFullName)
      case OxCast(_, _, _, value) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(value, ownerFullName)
      case OxFold(_, _, _, left, right) =>
        left.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)) ++
          right.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
      case OxPackExpansion(_, _, pattern) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(pattern, ownerFullName)
      case OxTypeOf(_, _, argument) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(argument, ownerFullName)
      case OxSizeOf(_, _, value, _) =>
        value.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
      case OxNew(_, _, _, arguments, initializerArguments) =>
        (arguments ++ initializerArguments)
          .flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
          .toSet
      case OxDelete(_, _, argument) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(argument, ownerFullName)
      case OxLambda(_, _, captures, _, _, _, _, body) =>
        captures
          .flatMap(_.initializer)
          .flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
          .toSet ++
          collectRequiredImplicitDefaultConstructorTypesFromStatements(body, ownerFullName)
      case OxCall(_, _, _, callee, arguments) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(callee, ownerFullName) ++
          arguments.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
      case OxFieldAccess(_, _, _, base) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(base, ownerFullName)
      case OxIndexAccess(_, _, base, index) =>
        Seq(base, index).flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)).toSet
      case OxInitializerList(_, _, elements) =>
        elements.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)).toSet
      case OxDesignatedInitializer(_, _, designator, value) =>
        Seq(designator, value)
          .flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
          .toSet
      case _: OxIdentifier | _: OxLiteral | _: OxDesignator =>
        Set.empty
    }
  }

  private def requiredImplicitDefaultConstructorType(
    local: OxLocalDecl,
    ownerFullName: Option[String]
  ): Option[String] = {
    val isDefaultConstruction = local.initializer match {
      case None =>
        true
      case Some(initializerList: OxInitializerList) if initializerList.elements.isEmpty =>
        isDirectListInitializer(local, initializerList)
      case _ =>
        false
    }
    Option
      .when(isDefaultConstruction)(localObjectAggregateTypeFullName(local.typeName, ownerFullName))
      .flatten
      .filter(hasImplicitDefaultConstructor)
  }

  private def localObjectAggregateTypeFullName(typeName: String, ownerFullName: Option[String]): Option[String] = {
    val normalized = stripCxxTypeQualifiers(normalizeType(resolveAliasType(typeName))).trim
    val isObjectType = normalized.nonEmpty &&
      normalized != Defines.Auto &&
      !normalized.endsWith("*") &&
      !normalized.endsWith("[]") &&
      !normalized.endsWith("&") &&
      !normalized.endsWith("&&")
    if (!isObjectType) {
      None
    } else {
      val ownerCandidates = ownerFullName.toSeq.flatMap { owner =>
        owner.split('.').toSeq.inits.filter(_.nonEmpty).map(parts => s"${parts.mkString(".")}.$normalized")
      }
      (ownerCandidates :+ normalized).find(aggregateTypeFullNames.contains)
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
        declarationFilename(structDecl),
        structDecl.code,
        NodeTypes.NAMESPACE_BLOCK,
        parentAstFullName,
        inherits =
          structDecl.baseClasses.map(baseClass => registerType(resolveBaseTypeFullName(baseClass, parentTypeFullName))),
        alias = aggregateAlias(typeName)
      )
    val fieldAsts = structDecl.fields.map { field =>
      val member =
        memberNode(origin.copy(code = field.code), field.name, field.code, registerType(normalizeType(field.typeName)))
      Ast(member).withChildren(Option.when(field.isStatic)(Ast(NewModifier().modifierType(ModifierTypes.STATIC))).toSeq)
    }
    val nestedAsts = structDecl.nestedDeclarations.flatMap {
      case nestedStruct: OxStructDecl => Seq(astForStruct(nestedStruct, Option(typeName), typeName))
      case nestedEnum: OxEnumDecl     => Seq(astForEnum(nestedEnum, Option(typeName), typeName))
      case nestedFunction: OxFunctionDecl if isShadowedByOutOfClassDefinition(typeName, nestedFunction) =>
        Seq.empty
      case nestedFunction: OxFunctionDecl =>
        astsForFunction(nestedFunction, Option(typeName), NodeTypes.TYPE_DECL, typeName)
      case _ => Seq.empty
    }
    val outOfClassMethodAsts = outOfClassFunctionsByOwner
      .getOrElse(typeName, Seq.empty)
      .flatMap(entry => astsForFunction(entry.function, entry.lexicalOwnerFullName, NodeTypes.TYPE_DECL, typeName))
    val implicitConstructorAst = Option.when(shouldEmitImplicitDefaultConstructor(typeName)) {
      implicitDefaultConstructorAst(structDecl, typeName)
    }
    Ast(typeDecl).withChildren(fieldAsts ++ implicitConstructorAst.toSeq ++ nestedAsts ++ outOfClassMethodAsts)
  }

  private def implicitDefaultConstructorAst(structDecl: OxStructDecl, typeName: String): Ast = {
    val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
    val origin          = OxOrigin(constructorName, None)
    val signature       = "void()"
    val fullName        = s"$typeName.$constructorName:$signature"
    val method =
      methodNode(
        origin,
        constructorName,
        constructorName,
        fullName,
        Option(signature),
        declarationFilename(structDecl),
        Option(NodeTypes.TYPE_DECL),
        Option(typeName)
      )
    val thisType = registerType(s"$typeName*")
    val thisParameter =
      parameterInNode(
        origin,
        Defines.This,
        Defines.This,
        0,
        isVariadic = false,
        EvaluationStrategies.BY_SHARING,
        thisType
      )
    val body         = blockAst(blockNode(origin, constructorName, Defines.Any))
    val methodReturn = methodReturnNode(origin, Defines.Void)
    methodAst(
      method,
      Seq(Ast(thisParameter)),
      body,
      methodReturn,
      Seq(NewModifier().modifierType(ModifierTypes.CONSTRUCTOR))
    )
  }

  private def resolveBaseTypeFullName(baseClass: String, parentTypeFullName: Option[String]): String = {
    val normalized = normalizeType(baseClass)
    val ownerCandidates = parentTypeFullName.toSeq.flatMap { parent =>
      parent.split('.').toSeq.inits.filter(_.nonEmpty).map(parts => s"${parts.mkString(".")}.$normalized")
    }
    val candidates = ownerCandidates :+ normalized
    candidates.find(aggregateTypeFullNames.contains).getOrElse(normalized)
  }

  private def typeAndBaseTypeFullNames(typeFullName: String): Seq[String] = {
    def loop(current: String, seen: Set[String]): Seq[String] = {
      val normalized = receiverAggregateTypeName(resolveAliasType(current))
      if (seen.contains(normalized)) {
        Seq.empty
      } else {
        normalized +: aggregateBaseTypesByType.getOrElse(normalized, Seq.empty).flatMap(loop(_, seen + normalized))
      }
    }

    loop(typeFullName, Set.empty).distinct
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
        declarationFilename(enumDecl),
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
          declarationFilename(enumDecl),
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

  private def astForTypedef(
    typedef: OxTypedefDecl,
    ownerFullName: Option[String] = None,
    parentAstFullName: String = globalNamespaceBlock().fullName
  ): Ast = {
    val origin = OxOrigin(typedef)
    val name = registerType(
      ownerFullName.map(owner => s"$owner.${normalizeType(typedef.name)}").getOrElse(normalizeType(typedef.name))
    )
    val aliasType = registerType(resolveAliasType(typedef.typeName))
    Ast(
      typeDeclNode(
        origin,
        typedef.name,
        name,
        declarationFilename(typedef),
        typedef.code,
        NodeTypes.NAMESPACE_BLOCK,
        parentAstFullName,
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
      val typeName  = registerType(globalTypeFullName(global))
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
        val typeName = registerType(globalTypeFullName(global))
        ScopeEntry(typeName, this.localNode(origin.copy(code = localCode), global.name, localCode, typeName))
      }
    )
    val localAst = Ast(scopeEntry.declaration)
    global.initializer match {
      case Some(initializer) =>
        val leftCode       = globalAssignmentTargetCode(global)
        val assignmentCode = s"$leftCode = ${initializer.code}"
        val left           = identifierAstForScopeEntry(global.name, leftCode, global.line, scopeEntry)
        val assignment =
          assignmentAst(origin.copy(code = assignmentCode), left, expressionAst(initializer), assignmentCode)
        Seq(localAst, assignment)
      case None =>
        Seq(localAst)
    }
  }

  private def localCodeForGlobal(global: OxGlobalVariableDecl): String = {
    stripConstinitSpecifier(global.initializer.fold(global.code)(_ => global.code.takeWhile(_ != '=').trim))
  }

  private def globalTypeFullName(global: OxGlobalVariableDecl): String = {
    typeFullNameWithStringLiteralLength(global.typeName, global.initializer)
  }

  private def globalAssignmentTargetCode(global: OxGlobalVariableDecl): String = {
    if (normalizeType(global.typeName).endsWith("[]")) s"${global.name}[]" else global.name
  }

  private def astsForFunction(
    function: OxFunctionDecl,
    ownerFullName: Option[String] = None,
    astParentType: String = NodeTypes.NAMESPACE_BLOCK,
    astParentFullName: String = globalNamespaceBlock().fullName
  ): Seq[Ast] = {
    val origin            = OxOrigin(function)
    val returnType        = registerType(normalizeType(function.returnType))
    val fullName          = functionFullName(function, ownerFullName)
    val simpleName        = functionSimpleName(function)
    val functionOwner     = functionOwnerFullName(function, ownerFullName)
    val parentTypeOwner   = functionOwner.filter(aggregateTypeFullNames.contains)
    val isStaticMethod    = isStaticFunction(function, parentTypeOwner)
    val isVirtualMethod   = isVirtualFunction(function, parentTypeOwner)
    val effectiveParentTy = parentTypeOwner.map(_ => NodeTypes.TYPE_DECL).getOrElse(astParentType)
    val effectiveParentFullName = parentTypeOwner.getOrElse {
      if (function.name.contains("::")) {
        functionOwner
          .map(owner => s"${declarationFilename(function)}:$owner")
          .getOrElse(astParentFullName)
      } else {
        astParentFullName
      }
    }
    val method =
      methodNode(
        origin,
        simpleName,
        simpleName,
        fullName,
        Option(function.signature),
        declarationFilename(function),
        Option(effectiveParentTy),
        Option(effectiveParentFullName)
      )
        .isExternal(!function.isDefinition)
    val implicitThisParameter = parentTypeOwner
      .filterNot(_ => isStaticMethod)
      .map { ownerTypeFullName =>
        val thisType = registerType(s"$ownerTypeFullName*")
        val thisNode =
          parameterInNode(
            origin,
            Defines.This,
            Defines.This,
            0,
            isVariadic = false,
            EvaluationStrategies.BY_SHARING,
            thisType
          )
        Defines.This -> (thisType, Ast(thisNode), thisNode)
      }
      .toSeq
    val explicitParameters = function.parameters.zipWithIndex.map { case (parameter, index) =>
      val parameterType = registerType(normalizeType(parameter.typeName))
      val parameterNode =
        parameterInNode(
          OxOrigin(parameter.code, Option(parameter.line)),
          parameter.name,
          parameter.code,
          index + 1,
          isVariadic = parameter.isVariadic,
          EvaluationStrategies.BY_VALUE,
          parameterType
        )
      parameter.name -> (parameterType, Ast(parameterNode), parameterNode)
    }
    val parameters = implicitThisParameter ++ explicitParameters

    val previousScope            = scope
    val previousCaptureContext   = functionCaptureContext
    val previousMethodOwner      = currentMethodOwnerTypeFullName
    val previousMethodFullName   = currentMethodFullName
    val previousMethodReturnType = currentMethodReturnTypeFullName
    val previousDestructorScopes = localDestructorScopes
    val previousJumpTargets      = jumpCleanupTargets
    val captureContext =
      FunctionCaptureContext(function, methodRefNode(origin, simpleName, fullName, simpleName))
    scope = parameters.map { case (name, (typeName, _, node)) => name -> ScopeEntry(typeName, node) }.toMap
    functionCaptureContext = Option(captureContext)
    currentMethodOwnerTypeFullName = parentTypeOwner
    currentMethodFullName = Option(fullName)
    currentMethodReturnTypeFullName = Option(returnType)
    localDestructorScopes = Vector.empty[LocalDestructor] :: Nil
    jumpCleanupTargets = Nil
    val bodyAsts =
      try {
        val statementAsts = function.body.flatMap(astsForStatement)
        val destructorAsts =
          Option
            .when(statementsMayCompleteNormally(function.body))(currentLocalDestructors.reverse.map(localDestructorAst))
            .getOrElse(Vector.empty)
        function.constructorInitializers.map(constructorInitializerAst) ++ statementAsts ++ destructorAsts
      } finally {
        localDestructorScopes = previousDestructorScopes
        jumpCleanupTargets = previousJumpTargets
        currentMethodReturnTypeFullName = previousMethodReturnType
        currentMethodFullName = previousMethodFullName
        currentMethodOwnerTypeFullName = previousMethodOwner
        functionCaptureContext = previousCaptureContext
        scope = previousScope
      }
    val captureLocalAsts =
      captureContext.capturedGlobals.values.map(capture => Ast(capture.scopeEntry.declaration)).toSeq
    val body         = blockAst(blockNode(origin, function.code, Defines.Any), (captureLocalAsts ++ bodyAsts).toList)
    val methodReturn = methodReturnNode(origin, returnType)
    val ast =
      methodAst(
        method,
        parameters.map(_._2._2),
        body,
        methodReturn,
        methodModifiers(simpleName, parentTypeOwner, isStaticMethod, isVirtualMethod)
      )

    captureAstForFunction(captureContext).fold(Seq(ast))(captureAst => Seq(ast, captureAst))
  }

  private def constructorInitializerAst(initializer: OxConstructorInitializer): Ast = {
    val fieldName      = qualifiedNameParts(initializer.field).lastOption.getOrElse(initializer.field)
    val assignmentCode = s"${Defines.This}->$fieldName = ${constructorInitializerValueCode(initializer)}"
    val left = implicitFieldAccessAst(fieldName, initializer.line).getOrElse(
      identifierAst(fieldName, fieldName, initializer.line)
    )
    val right = initializer.arguments match {
      case Seq(argument) => expressionAst(argument)
      case arguments =>
        operatorCallAst(
          OxOrigin(initializer.code, Option(initializer.line)),
          initializer.code,
          Operators.arrayInitializer,
          arguments.map(expressionAst)
        )
    }
    assignmentAst(OxOrigin(initializer.code, Option(initializer.line)), left, right, assignmentCode)
  }

  private def constructorInitializerValueCode(initializer: OxConstructorInitializer): String = {
    initializer.arguments match {
      case Seq(argument) => argument.code
      case arguments     => arguments.map(_.code).mkString("{", ", ", "}")
    }
  }

  private def registerLocalDestructor(name: String, typeName: String, line: Int): Unit = {
    destructorEntryForType(typeName).foreach { destructor =>
      registerLocalDestructor(LocalDestructor(name, line, destructor))
    }
  }

  private def registerLocalDestructor(destructor: LocalDestructor): Unit = {
    localDestructorScopes match {
      case current :: rest => localDestructorScopes = (current :+ destructor) :: rest
      case Nil             =>
    }
  }

  private def currentLocalDestructors: Vector[LocalDestructor] = {
    localDestructorScopes.headOption.getOrElse(Vector.empty)
  }

  private def activeLocalDestructors: Seq[LocalDestructor] = {
    localDestructorScopes.flatMap(_.reverse)
  }

  private def localDestructorsExitingTo(preservedScopeDepth: Int): Seq[LocalDestructor] = {
    val exitedScopeCount = (localDestructorScopes.length - preservedScopeDepth).max(0)
    localDestructorScopes.take(exitedScopeCount).flatMap(_.reverse)
  }

  private def breakLocalDestructors: Seq[LocalDestructor] = {
    jumpCleanupTargets
      .collectFirst { case JumpCleanupTarget(Some(preservedScopeDepth), _, _) =>
        localDestructorsExitingTo(preservedScopeDepth)
      }
      .getOrElse(Vector.empty)
  }

  private def continueLocalDestructors: Seq[LocalDestructor] = {
    jumpCleanupTargets
      .collectFirst { case JumpCleanupTarget(_, Some(preservedScopeDepth), _) =>
        localDestructorsExitingTo(preservedScopeDepth)
      }
      .getOrElse(Vector.empty)
  }

  private def throwLocalDestructors: Seq[LocalDestructor] = {
    jumpCleanupTargets
      .collectFirst { case JumpCleanupTarget(_, _, Some(preservedScopeDepth)) =>
        localDestructorsExitingTo(preservedScopeDepth)
      }
      .getOrElse(activeLocalDestructors)
  }

  private def localDestructorAst(destructor: LocalDestructor): Ast = {
    val code = s"${destructor.receiverCode}.${destructor.entry.simpleName}()"
    val callNode_ =
      callNode(
        OxOrigin(code, Option(destructor.line)),
        code,
        destructor.entry.simpleName,
        destructor.entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(destructor.entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(
      callNode_,
      base = Option(identifierAst(destructor.receiverCode, destructor.receiverCode, destructor.line))
    )
  }

  private def astsForStatement(statement: OxStatement): Seq[Ast] = {
    statement match {
      case unknown: OxUnknownStatement =>
        Seq(Ast(unknownNode(OxOrigin(unknown), unknown.code)))
      case _: OxUsingEnumStatement =>
        Seq.empty
      case local: OxLocalDecl =>
        astsForLocalDecl(local)
      case structuredBinding: OxStructuredBinding =>
        astsForStructuredBinding(structuredBinding)
      case assignment: OxAssignment =>
        val assignmentAst_ =
          overloadedAssignmentOperatorAst(assignment).getOrElse {
            val left  = expressionAst(assignment.left)
            val right = expressionAst(assignment.right)
            if (assignment.operator == "=") {
              assignmentAst(OxOrigin(assignment), left, right, assignment.code)
            } else {
              operatorCallAst(OxOrigin(assignment), assignment.code, operatorFor(assignment.operator), Seq(left, right))
            }
          }
        assignmentAst_ +:
          (heapConstructorAstsForExpressions(Seq(assignment.left, assignment.right)) ++
            temporaryDestructorAstsForExpressions(Seq(assignment.left, assignment.right)))
      case ret: OxReturn =>
        heapConstructorAstsForExpressions(ret.expression.toSeq) ++ temporaryDestructorAstsForReturnExpression(
          ret.expression
        ) ++ activeLocalDestructors.map(localDestructorAst) :+
          returnAst(returnNode(OxOrigin(ret), ret.code), ret.expression.toSeq.map(expressionAst))
      case throwStmt: OxThrow =>
        val throwAst = Ast(controlStructureNode(OxOrigin(throwStmt), ControlStructureTypes.THROW, throwStmt.code))
          .withChildren(throwStmt.expression.toSeq.map(expressionAst))
        heapConstructorAstsForExpressions(throwStmt.expression.toSeq) ++ temporaryDestructorAstsForExpressions(
          throwStmt.expression.toSeq
        ) ++ throwLocalDestructors.map(localDestructorAst) :+
          throwAst
      case tryStmt: OxTry =>
        val tryNode = controlStructureNode(OxOrigin("try", Option(tryStmt.line)), ControlStructureTypes.TRY, "try")
        val preservedScopeDepth = localDestructorScopes.length
        val bodyAst = withJumpCleanupTarget(
          JumpCleanupTarget(
            breakPreservedScopeDepth = None,
            continuePreservedScopeDepth = None,
            throwPreservedScopeDepth = Option(preservedScopeDepth)
          )
        ) {
          statementBlockAst(tryStmt.body, "try", tryStmt.line)
        }
        val catchAsts = tryStmt.catches.map(catchAst)
        Seq(tryCatchAst(tryNode, bodyAst, catchAsts, None))
      case ifStmt: OxIf =>
        def ifAsts: Seq[Ast] = {
          val initializerAsts = ifStmt.initializer.flatMap(astsForStatement)
          val ifNode          = controlStructureNode(OxOrigin(ifStmt), ControlStructureTypes.IF, ifStmt.code)
          val conditionAst    = conditionExpressionAstWithInitializers(ifStmt.conditionInitializer, ifStmt.condition)
          val conditionHeapConstructors =
            if (ifStmt.conditionInitializer.isEmpty) heapConstructorAstsForExpressions(Seq(ifStmt.condition))
            else Seq.empty
          val conditionTemporaryCleanup = temporaryDestructorAstsForExpressions(Seq(ifStmt.condition))
          val thenAst                   = statementBlockAst(ifStmt.thenBody, "then", ifStmt.line)
          val elseAst =
            Option.when(ifStmt.elseBody.nonEmpty) {
              Ast(controlStructureNode(OxOrigin("else", Option(ifStmt.line)), ControlStructureTypes.ELSE, "else"))
                .withChild(statementBlockAst(ifStmt.elseBody, "else", ifStmt.line))
            }
          initializerAsts :+
            ifThenElseAst(ifNode, Option(conditionAst), thenAst, elseAst)
              .withChildren(conditionHeapConstructors ++ conditionTemporaryCleanup)
        }

        if (ifStmt.initializer.isEmpty && ifStmt.conditionInitializer.isEmpty) {
          ifAsts
        } else {
          val (asts, destructors) = inNestedScopeCollectingDestructors(ifAsts)
          asts ++ Option
            .when(statementsMayCompleteNormally(Seq(ifStmt)))(destructors.reverse.map(localDestructorAst))
            .getOrElse(Seq.empty)
        }
      case whileStmt: OxWhile =>
        def whileAsts(preservedScopeDepth: Int): Seq[Ast] = {
          val initializerAsts          = whileStmt.initializer.flatMap(astsForStatement)
          val conditionDestructorStart = currentLocalDestructors.length
          val conditionAst = conditionExpressionAstWithInitializers(whileStmt.conditionInitializer, whileStmt.condition)
          val conditionDestructors = currentLocalDestructors.drop(conditionDestructorStart)
          val conditionHeapConstructors =
            if (whileStmt.conditionInitializer.isEmpty) heapConstructorAstsForExpressions(Seq(whileStmt.condition))
            else Seq.empty
          val conditionTemporaryCleanup = temporaryDestructorAstsForExpressions(Seq(whileStmt.condition))
          val hasScopedInitializer      = whileStmt.initializer.nonEmpty || whileStmt.conditionInitializer.nonEmpty
          val breakPreservedScopeDepth =
            if (hasScopedInitializer) localDestructorScopes.length else preservedScopeDepth
          val continuePreservedScopeDepth =
            if (whileStmt.conditionInitializer.nonEmpty) preservedScopeDepth
            else if (hasScopedInitializer) localDestructorScopes.length
            else preservedScopeDepth
          val bodyAst = withJumpCleanupTarget(
            JumpCleanupTarget(
              breakPreservedScopeDepth = Option(breakPreservedScopeDepth),
              continuePreservedScopeDepth = Option(continuePreservedScopeDepth)
            )
          ) {
            statementBlockAst(
              whileStmt.body,
              "while",
              whileStmt.line,
              extraDestructorsOnNormalCompletion = conditionDestructors
            )
          }
          initializerAsts :+
            whileAst(
              Option(conditionAst),
              Seq(bodyAst),
              code = Option(whileStmt.code),
              lineNumber = Option(whileStmt.line)
            ).withChildren(conditionHeapConstructors ++ conditionTemporaryCleanup)
        }
        val preservedScopeDepth = localDestructorScopes.length
        if (whileStmt.initializer.isEmpty && whileStmt.conditionInitializer.isEmpty) {
          whileAsts(preservedScopeDepth)
        } else {
          val (asts, destructors) = inNestedScopeCollectingDestructors(whileAsts(preservedScopeDepth))
          asts ++ destructors.reverse.map(localDestructorAst)
        }
      case doWhileStmt: OxDoWhile =>
        val preservedScopeDepth       = localDestructorScopes.length
        val conditionHeapConstructors = heapConstructorAstsForExpressions(Seq(doWhileStmt.condition))
        val conditionTemporaryCleanup = temporaryDestructorAstsForExpressions(Seq(doWhileStmt.condition))
        val bodyAst = withJumpCleanupTarget(
          JumpCleanupTarget(
            breakPreservedScopeDepth = Option(preservedScopeDepth),
            continuePreservedScopeDepth = Option(preservedScopeDepth)
          )
        ) {
          statementBlockAst(doWhileStmt.body, "do", doWhileStmt.line)
        }
        Seq(
          doWhileAst(
            Option(conditionExpressionAst(doWhileStmt.condition)),
            Seq(bodyAst),
            code = Option(doWhileStmt.code),
            lineNumber = Option(doWhileStmt.line)
          ).withChildren(conditionHeapConstructors ++ conditionTemporaryCleanup)
        )
      case forStmt: OxFor =>
        val (forAst_, initializerDestructors) = inNestedScopeCollectingDestructors {
          val forNode               = controlStructureNode(OxOrigin(forStmt), ControlStructureTypes.FOR, forStmt.code)
          val initializerAsts       = forStmt.initializer.flatMap(astsForStatement)
          val (localAsts, initAsts) = initializerAsts.partition(_.root.exists(_.isInstanceOf[NewLocal]))
          val conditionAsts         = forStmt.condition.toSeq.map(conditionExpressionAst)
          val conditionTemporaryCleanup = temporaryDestructorAstsForExpressions(forStmt.condition.toSeq)
          val conditionHeapConstructors = heapConstructorAstsForExpressions(forStmt.condition.toSeq)
          val updateAsts = forStmt.update.toSeq.flatMap { update =>
            expressionAst(update) +:
              (heapConstructorAstsForExpressions(Seq(update)) ++ temporaryDestructorAstsForExpressions(Seq(update)))
          }
          val preservedScopeDepth = localDestructorScopes.length
          val bodyAst = withJumpCleanupTarget(
            JumpCleanupTarget(
              breakPreservedScopeDepth = Option(preservedScopeDepth),
              continuePreservedScopeDepth = Option(preservedScopeDepth)
            )
          ) {
            statementBlockAst(forStmt.body, "for", forStmt.line)
          }
          forAst(forNode, localAsts, initAsts, conditionAsts, updateAsts, bodyAst).withChildren(
            conditionHeapConstructors ++ conditionTemporaryCleanup
          )
        }
        forAst_ +: initializerDestructors.reverse.map(localDestructorAst)
      case breakStmt: OxBreak =>
        breakLocalDestructors.map(localDestructorAst) :+
          Ast(controlStructureNode(OxOrigin(breakStmt), ControlStructureTypes.BREAK, breakStmt.code))
      case continueStmt: OxContinue =>
        continueLocalDestructors.map(localDestructorAst) :+
          Ast(controlStructureNode(OxOrigin(continueStmt), ControlStructureTypes.CONTINUE, continueStmt.code))
      case gotoStmt: OxGoto =>
        Seq(Ast(controlStructureNode(OxOrigin(gotoStmt), ControlStructureTypes.GOTO, gotoStmt.code)))
      case labelStmt: OxLabel =>
        Ast(jumpTargetNode(OxOrigin(labelStmt), labelStmt.label, labelStmt.code)) +:
          labelStmt.body.flatMap(astsForStatement)
      case switchStmt: OxSwitch =>
        val (switchAsts, switchDestructors) = inNestedScopeCollectingDestructors {
          val initializerAsts = switchStmt.initializer.flatMap(astsForStatement)
          val switchNode = controlStructureNode(OxOrigin(switchStmt), ControlStructureTypes.SWITCH, switchStmt.code)
          val conditionAst =
            conditionExpressionAstWithInitializers(
              switchStmt.conditionInitializer,
              switchStmt.condition,
              wrapTruthy = false
            )
          val conditionHeapConstructors =
            if (switchStmt.conditionInitializer.isEmpty) heapConstructorAstsForExpressions(Seq(switchStmt.condition))
            else Seq.empty
          val conditionTemporaryCleanup = temporaryDestructorAstsForExpressions(Seq(switchStmt.condition))
          val switchAst_ = {
            val preservedScopeDepth = localDestructorScopes.length
            val bodyAsts = withJumpCleanupTarget(
              JumpCleanupTarget(
                breakPreservedScopeDepth = Option(preservedScopeDepth),
                continuePreservedScopeDepth = None
              )
            ) {
              switchBodyWithUsingEnumCases(switchStmt.body).flatMap(astsForStatement)
            }
            switchAst(switchNode, conditionAst, bodyAsts)
              .withChildren(conditionHeapConstructors ++ conditionTemporaryCleanup)
          }
          initializerAsts :+ switchAst_
        }
        switchAsts ++ switchDestructors.reverse.map(localDestructorAst)
      case caseStmt: OxCase =>
        val name = if (caseStmt.value.isDefined) "case" else "default"
        Ast(jumpTargetNode(OxOrigin(caseStmt), name, caseStmt.code)) +:
          (caseStmt.value.toSeq.map(expressionAst) ++ caseStmt.body.flatMap(astsForStatement))
      case expressionStatement: OxExpressionStatement =>
        expressionStatement.expression match {
          case deleteExpression: OxDelete =>
            heapConstructorAstsForExpressions(Seq(deleteExpression.argument)) ++
              heapDestructorAstsForDelete(deleteExpression) ++
              Seq(expressionAst(deleteExpression)) ++
              temporaryDestructorAstsForExpressions(Seq(deleteExpression.argument))
          case expression =>
            expressionAst(expression) +:
              (heapConstructorAstsForExpressions(Seq(expression)) ++ temporaryDestructorAstsForExpressions(
                Seq(expression)
              ))
        }
    }
  }

  private def switchBodyWithUsingEnumCases(body: Seq[OxStatement]): Seq[OxStatement] = {
    var activeUsingEnumType: Option[String] = None
    body.map {
      case usingEnum: OxUsingEnumStatement =>
        activeUsingEnumType = Option(usingEnum.typeName)
        usingEnum
      case caseStmt: OxCase =>
        activeUsingEnumType.map(qualifyUsingEnumCase(caseStmt, _)).getOrElse(caseStmt)
      case statement =>
        statement
    }
  }

  private def qualifyUsingEnumCase(caseStmt: OxCase, typeName: String): OxCase = {
    caseStmt.value match {
      case Some(OxIdentifier(name, _, line)) =>
        val normalizedType = normalizeType(typeName)
        val code           = s"$normalizedType.$name"
        caseStmt.copy(value =
          Some(
            OxFieldAccess(
              field = name,
              code = code,
              line = line,
              base = OxIdentifier(normalizedType, normalizedType, line)
            )
          )
        )
      case _ =>
        caseStmt
    }
  }

  private def astsForLocalDecl(local: OxLocalDecl, useConstructorInitializers: Boolean = true): Seq[Ast] = {
    val origin          = OxOrigin(local)
    val localLambdaInfo = local.initializer.collect { case lambda: OxLambda => lambdaInfo(lambda) }
    val typeName        = registerType(localTypeFullName(local))
    val localCode       = localDeclarationCode(local)
    val localNode       = this.localNode(origin.copy(code = localCode), local.name, localCode, typeName)
    scope = scope.updated(local.name, ScopeEntry(typeName, localNode, localLambdaInfo))
    registerLocalDestructor(local.name, typeName, local.line)
    val extendedTemporaryDestructor = local.initializer.flatMap(referenceBoundTemporaryDestructor(typeName, _))
    extendedTemporaryDestructor.foreach(registerLocalDestructor)
    val localAst = Ast(localNode)
    val temporaryDestructorAsts =
      temporaryDestructorAstsForLocalInitializer(local.initializer, extendedTemporaryDestructor.isDefined)
    local.initializer match {
      case Some(initializer: OxInitializerList)
          if useConstructorInitializers && isConstructorInitializer(typeName, initializer) =>
        Seq(localAst, constructorAssignmentAst(local, initializer, typeName)) ++ temporaryDestructorAsts
      case Some(initializer) if useConstructorInitializers && isCopyConstructorInitializer(typeName, initializer) =>
        Seq(
          localAst,
          constructorAssignmentAst(local, Seq(initializer), initializer.code, OxOrigin(initializer), typeName)
        ) ++ temporaryDestructorAsts
      case Some(initializer) =>
        val (left, targetCode) = localAssignmentTargetAst(local, typeName)
        val assignmentCode     = s"$targetCode = ${initializer.code}"
        val assignment =
          assignmentAst(origin.copy(code = assignmentCode), left, expressionAst(initializer), assignmentCode)
        val fieldAssignments = designatedInitializerAssignmentAsts(local, initializer, typeName)
        Seq(localAst, assignment) ++ fieldAssignments ++ heapConstructorAstsForExpressions(Seq(initializer)) ++
          temporaryDestructorAsts
      case None if useConstructorInitializers && isDefaultConstructorInitializer(typeName) =>
        Seq(localAst, constructorAssignmentAst(local, Seq.empty, "", origin, typeName))
      case None =>
        Seq(localAst)
    }
  }

  private def localAssignmentTargetAst(local: OxLocalDecl, typeName: String): (Ast, String) = {
    val normalizedType = normalizeType(resolveAliasType(typeName))
    if (normalizedType.endsWith("*")) {
      val targetCode = s"*${local.name}"
      identifierAst(local.name, targetCode, local.line) -> targetCode
    } else if (normalizedType.endsWith("&") || normalizedType.endsWith("&&")) {
      val targetCode = s"&${local.name}"
      identifierAst(local.name, targetCode, local.line) -> targetCode
    } else {
      identifierAst(local.name, local.name, local.line) -> local.name
    }
  }

  private def designatedInitializerAssignmentAsts(
    local: OxLocalDecl,
    initializer: OxExpression,
    typeName: String
  ): Seq[Ast] = {
    initializer match {
      case OxInitializerList(_, _, elements) if aggregateFieldEntriesByType.contains(resolveAliasType(typeName)) =>
        elements.collect { case OxDesignatedInitializer(_, line, OxDesignator(fieldName, _, _), value) =>
          val fieldCode = s"${local.name}.$fieldName"
          val code      = s"$fieldCode = ${value.code}"
          val fieldType = fieldTypeFullName(typeName, fieldName).getOrElse(Defines.Any)
          val left = fieldAccessAstForOperator(
            OxOrigin(fieldCode, Option(line)),
            OxOrigin(fieldName, Option(line)),
            identifierAst(local.name, local.name, line),
            fieldCode,
            fieldName,
            registerType(fieldType)
          )
          assignmentAst(OxOrigin(code, Option(line)), left, expressionAst(value), code)
        }
      case _ => Seq.empty
    }
  }

  private def astsForStructuredBinding(binding: OxStructuredBinding): Seq[Ast] = {
    val tempTypeName = if (normalizeType(binding.typeName).startsWith(Defines.Auto)) Defines.Auto else binding.typeName
    val tempLocal = OxLocalDecl(
      name = binding.tempName,
      typeName = tempTypeName,
      code = s"$tempTypeName ${binding.tempName}",
      line = binding.line,
      initializer = binding.initializer
    )
    val tempAsts = astsForLocalDecl(tempLocal, useConstructorInitializers = false)
    val tempType = scope.get(binding.tempName).map(_.typeFullName).getOrElse(registerType(Defines.Any))
    tempAsts ++ binding.names.zipWithIndex.flatMap { case (name, index) =>
      val access = structuredBindingAccess(binding.tempName, tempType, name, index, binding.line)
      astsForLocalDecl(
        OxLocalDecl(name = name, typeName = Defines.Auto, code = name, line = binding.line, initializer = Some(access))
      )
    }
  }

  private def structuredBindingAccess(
    tempName: String,
    tempType: String,
    name: String,
    index: Int,
    line: Int
  ): OxExpression = {
    val base = OxIdentifier(tempName, tempName, line)
    if (isArrayLikeType(tempType)) {
      val indexCode = index.toString
      OxIndexAccess(
        code = s"$tempName[$indexCode]",
        line = line,
        base = base,
        index = OxLiteral(indexCode, indexCode, line)
      )
    } else {
      val fieldName = aggregateFieldByIndex(tempType, index).map(_.name).getOrElse(name)
      OxFieldAccess(field = fieldName, code = s"$tempName.$fieldName", line = line, base = base)
    }
  }

  private def aggregateFieldByIndex(typeName: String, index: Int): Option[OxFieldDecl] = {
    typeAndBaseTypeFullNames(typeName).collectFirst(Function.unlift { candidate =>
      aggregateFieldsByType.get(normalizeType(candidate)).flatMap(_.lift(index))
    })
  }

  private def isArrayLikeType(typeName: String): Boolean = {
    val normalized = normalizeType(resolveAliasType(typeName))
    normalized.endsWith("[]") || normalized.matches(""".*\[[^\]]*\]$""")
  }

  private def localDeclarationCode(local: OxLocalDecl): String = {
    val code = local.initializer match {
      case Some(initializer) =>
        val prefix = localInitializerPrefix(local, initializer)
        if (prefix.endsWith("=")) prefix.stripSuffix("=").trim else prefix
      case None => local.code
    }
    stripConstinitSpecifier(code)
  }

  private def localInitializerPrefix(local: OxLocalDecl, initializer: OxExpression): String = {
    val initializerIndex = local.code.lastIndexOf(initializer.code)
    if (initializerIndex >= 0) local.code.take(initializerIndex).trim else local.code
  }

  private def stripConstinitSpecifier(code: String): String = {
    code.trim.replaceFirst("""^constinit\s+""", "")
  }

  private def localTypeFullName(local: OxLocalDecl): String = {
    val explicitType = typeFullNameWithStringLiteralLength(local.typeName, local.initializer)
    local.initializer match {
      case Some(lambda: OxLambda) if explicitType == Defines.Auto => lambdaInfo(lambda).fullName
      case Some(initializerList: OxInitializerList)
          if explicitType.startsWith(Defines.Auto) && isDirectListInitializer(local, initializerList) =>
        initializerListElementTypeFullName(initializerList)
          .flatMap(typeName => inferredAutoTypeFullName(explicitType, typeName))
          .getOrElse(explicitType)
      case Some(initializer) if explicitType.startsWith(Defines.Auto) =>
        expressionTypeFullName(initializer)
          .flatMap(typeName => inferredAutoTypeFullName(explicitType, typeName))
          .getOrElse(explicitType)
      case _ => explicitType
    }
  }

  private def isDirectListInitializer(local: OxLocalDecl, initializerList: OxInitializerList): Boolean = {
    !localInitializerPrefix(local, initializerList).endsWith("=")
  }

  private def typeFullNameWithStringLiteralLength(typeName: String, initializer: Option[OxExpression]): String = {
    val explicitType = normalizeType(typeName)
    initializer match {
      case Some(OxLiteral(value, _, _)) if explicitType.endsWith("[]") =>
        stringLiteralElementCount(value)
          .map(count => s"${explicitType.stripSuffix("[]")}[$count]")
          .getOrElse(explicitType)
      case _ =>
        explicitType
    }
  }

  private def inferredAutoTypeFullName(explicitType: String, initializerType: String): Option[String] = {
    val resolvedInitializerType = normalizeType(resolveAliasType(initializerType))
    explicitType match {
      case Defines.Auto =>
        Some(resolvedInitializerType)
      case "auto*" if resolvedInitializerType.endsWith("*") =>
        Some(resolvedInitializerType)
      case "auto&" =>
        Some(s"${stripCxxReference(resolvedInitializerType)}&")
      case "auto&&" =>
        Some(s"${stripCxxReference(resolvedInitializerType)}&&")
      case _ =>
        None
    }
  }

  private def isConstructorInitializer(typeName: String, initializer: OxInitializerList): Boolean = {
    val initializerCode = initializer.code.trim
    aggregateTypeFullNames.contains(typeName) &&
    (initializerCode.startsWith("(") || (initializerCode
      .startsWith("{") && constructorEntry(typeName, initializer.elements).isDefined))
  }

  private def isCopyConstructorInitializer(typeName: String, initializer: OxExpression): Boolean = {
    aggregateTypeFullNames.contains(typeName) && !initializer.isInstanceOf[OxInitializerList]
  }

  private def isDefaultConstructorInitializer(typeName: String): Boolean = {
    aggregateTypeFullNames.contains(typeName) &&
    (constructorEntry(typeName, Seq.empty).isDefined || hasImplicitDefaultConstructor(typeName))
  }

  private def hasImplicitDefaultConstructor(typeName: String): Boolean = {
    val resolvedType = resolveAliasType(typeName)
    aggregateDeclarationsByType.get(resolvedType).exists { structDecl =>
      FileDefaults.hasCppFileExtension(declarationFilename(structDecl)) && constructorEntriesForType(
        resolvedType
      ).isEmpty
    }
  }

  private def shouldEmitImplicitDefaultConstructor(typeName: String): Boolean = {
    hasImplicitDefaultConstructor(typeName) && requiredImplicitDefaultConstructorTypes.contains(
      resolveAliasType(typeName)
    )
  }

  private def constructorAssignmentAst(local: OxLocalDecl, initializer: OxInitializerList, typeName: String): Ast = {
    constructorAssignmentAst(local, initializer.elements, initializer.code.trim, OxOrigin(initializer), typeName)
  }

  private def constructorAssignmentAst(
    local: OxLocalDecl,
    arguments: Seq[OxExpression],
    initializerCode: String,
    initializerOrigin: OxOrigin,
    typeName: String
  ): Ast = {
    val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
    val constructor     = constructorEntry(typeName, arguments)
    val implicitSignature =
      Option.when(arguments.isEmpty && hasImplicitDefaultConstructor(typeName))("void()")
    val signature = constructor.map(_.function.signature).orElse(implicitSignature)
    val methodFullName = constructor
      .map(_.fullName)
      .orElse(signature.map(sig => s"$typeName.$constructorName:$sig"))
      .getOrElse(s"$typeName.$constructorName")
    val initCode =
      if (initializerCode.startsWith("(") && initializerCode.endsWith(")"))
        initializerCode.stripPrefix("(").stripSuffix(")")
      else if (initializerCode.startsWith("{") && initializerCode.endsWith("}"))
        initializerCode.stripPrefix("{").stripSuffix("}")
      else initializerCode
    val constructorCode = s"$typeName.$constructorName($initCode)"
    val callNode_ = callNode(
      initializerOrigin.copy(code = constructorCode),
      constructorCode,
      constructorName,
      methodFullName,
      DispatchTypes.STATIC_DISPATCH,
      signature,
      Some(registerType(Defines.Void))
    )
    val assignmentCode = s"${local.name} = $constructorCode"
    val left           = identifierAst(local.name, local.name, local.line)
    val right =
      constructorInvocationBlockAst(initializerOrigin, typeName, callNode_, arguments.map(expressionAst))
    assignmentAst(OxOrigin(local).copy(code = assignmentCode), left, right, assignmentCode)
  }

  private def constructorInvocationBlockAst(
    origin: OxOrigin,
    typeName: String,
    constructorCallNode: NewCall,
    arguments: Seq[Ast]
  ): Ast = {
    val block         = blockNode(origin, constructorCallNode.code, Defines.Any)
    val tmpName       = nextTemporaryName()
    val tmpLocal      = localNode(origin.copy(code = tmpName), tmpName, tmpName, registerType(typeName))
    val tmpIdentifier = identifierNode(origin.copy(code = tmpName), tmpName, tmpName, registerType(typeName))
    val tmpAst        = Ast(tmpIdentifier).withRefEdge(tmpIdentifier, tmpLocal)
    val allocCallNode = callNode(
      origin.copy(code = Operators.alloc),
      Operators.alloc,
      Operators.alloc,
      Operators.alloc,
      DispatchTypes.STATIC_DISPATCH
    )
    val allocAssignCode = s"$tmpName = ${Operators.alloc}"
    val allocAssignAst =
      assignmentAst(origin.copy(code = allocAssignCode), tmpAst, Ast(allocCallNode), allocAssignCode)
    val baseIdentifier = identifierNode(origin.copy(code = tmpName), tmpName, tmpName, registerType(typeName))
    val baseAst        = Ast(baseIdentifier).withRefEdge(baseIdentifier, tmpLocal)
    val addressOfNode =
      callNode(
        origin.copy(code = s"&$tmpName"),
        s"&$tmpName",
        Operators.addressOf,
        Operators.addressOf,
        DispatchTypes.STATIC_DISPATCH
      )
    val addressOfAst       = callAst(addressOfNode, Seq(baseAst))
    val retIdentifier      = identifierNode(origin.copy(code = tmpName), tmpName, tmpName, registerType(typeName))
    val retAst             = Ast(retIdentifier).withRefEdge(retIdentifier, tmpLocal)
    val constructorCallAst = createCallAst(constructorCallNode, arguments, base = Option(addressOfAst))
    Ast(block).withChildren(Seq(Ast(tmpLocal), allocAssignAst, constructorCallAst, retAst))
  }

  private def nextTemporaryName(): String = {
    val scopeName = currentMethodFullName.getOrElse(globalNamespaceBlock().fullName)
    val index     = temporaryIndices.getOrElse(scopeName, 0)
    temporaryIndices.update(scopeName, index + 1)
    s"<tmp>$index"
  }

  private def constructorEntry(typeName: String, arguments: Seq[OxExpression]): Option[FunctionEntry] = {
    val candidates = constructorEntriesForType(typeName)
    selectFunctionEntry(candidates, Some(arguments))
  }

  private def constructorEntriesForType(typeName: String): Seq[FunctionEntry] = {
    val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
    functionEntries.filter(entry => entry.qualifiedName == s"$typeName.$constructorName")
  }

  private def heapConstructorAstsForExpressions(expressions: Seq[OxExpression]): Seq[Ast] = {
    expressions.flatMap(heapConstructorsForExpression).map(heapConstructorAst)
  }

  private def heapConstructorsForExpression(expression: OxExpression): Seq[HeapConstructor] = {
    val nested = expression match {
      case OxBinary(_, _, _, left, right) =>
        Seq(left, right).flatMap(heapConstructorsForExpression)
      case OxUnary(_, _, _, _, argument) =>
        heapConstructorsForExpression(argument)
      case OxConditional(_, _, condition, consequence, alternative) =>
        Seq(condition).flatMap(heapConstructorsForExpression) ++
          consequence.toSeq.flatMap(heapConstructorsForExpression) ++
          Seq(alternative).flatMap(heapConstructorsForExpression)
      case OxFold(_, _, _, left, right) =>
        left.toSeq.flatMap(heapConstructorsForExpression) ++ right.toSeq.flatMap(heapConstructorsForExpression)
      case OxPackExpansion(_, _, pattern) =>
        heapConstructorsForExpression(pattern)
      case OxTypeOf(_, _, argument) =>
        heapConstructorsForExpression(argument)
      case OxCast(_, _, _, value) =>
        heapConstructorsForExpression(value)
      case OxSizeOf(_, _, value, _) =>
        value.toSeq.flatMap(heapConstructorsForExpression)
      case OxNew(_, _, _, arguments, _) =>
        arguments.flatMap(heapConstructorsForExpression)
      case OxDelete(_, _, argument) =>
        heapConstructorsForExpression(argument)
      case _: OxLambda =>
        Seq.empty
      case OxCall(_, _, _, callee, arguments) =>
        heapConstructorsForExpression(callee) ++ arguments.flatMap(heapConstructorsForExpression)
      case OxFieldAccess(_, _, _, base) =>
        heapConstructorsForExpression(base)
      case OxIndexAccess(_, _, base, index) =>
        Seq(base, index).flatMap(heapConstructorsForExpression)
      case OxInitializerList(_, _, elements) =>
        elements.flatMap(heapConstructorsForExpression)
      case OxDesignatedInitializer(_, _, designator, value) =>
        Seq(designator, value).flatMap(heapConstructorsForExpression)
      case _: OxIdentifier | _: OxLiteral | _: OxDesignator =>
        Seq.empty
    }
    val current = expression match {
      case newExpression: OxNew =>
        heapConstructorForNew(newExpression).toSeq
      case _ =>
        Seq.empty
    }
    nested ++ current
  }

  private def heapConstructorForNew(newExpression: OxNew): Option[HeapConstructor] = {
    val aggregateType = receiverAggregateTypeName(newExpression.typeName)
    resolveAggregateTypeFullName(aggregateType).flatMap { typeName =>
      constructorEntry(typeName, newExpression.initializerArguments).map { entry =>
        val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
        val initCode        = newExpression.initializerArguments.map(_.code).mkString(", ")
        val constructorCode = s"$typeName.$constructorName($initCode)"
        HeapConstructor(constructorCode, newExpression.line, entry, newExpression.initializerArguments)
      }
    }
  }

  private def heapConstructorAst(constructor: HeapConstructor): Ast = {
    val callNode_ =
      callNode(
        OxOrigin(constructor.code, Option(constructor.line)),
        constructor.code,
        constructor.entry.simpleName,
        constructor.entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(constructor.entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(callNode_, constructor.arguments.map(expressionAst))
  }

  private def heapDestructorAstsForDelete(deleteExpression: OxDelete): Seq[Ast] = {
    heapDestructorForDelete(deleteExpression).toSeq.map(heapDestructorAst)
  }

  private def heapDestructorForDelete(deleteExpression: OxDelete): Option[HeapDestructor] = {
    expressionTypeFullName(deleteExpression.argument).flatMap { pointerType =>
      val receiverType  = receiverAggregateTypeName(pointerType)
      val aggregateType = resolveAggregateTypeFullName(receiverType).getOrElse(receiverType)
      destructorEntryForType(aggregateType).map { entry =>
        val receiverCode = deleteExpression.argument.code
        HeapDestructor(s"$receiverCode->${entry.simpleName}()", deleteExpression.line, entry, deleteExpression.argument)
      }
    }
  }

  private def heapDestructorAst(destructor: HeapDestructor): Ast = {
    val callNode_ =
      callNode(
        OxOrigin(destructor.code, Option(destructor.line)),
        destructor.code,
        destructor.entry.simpleName,
        destructor.entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(destructor.entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(callNode_, base = Option(expressionAst(destructor.receiver)))
  }

  private def constructorTemporaryTypeFullName(call: OxCall): Option[String] = {
    resolveAggregateTypeFullName(call.name).orElse(bracedTypeConstructionTypeFullName(call))
  }

  private def bracedTypeConstructionTypeFullName(call: OxCall): Option[String] = {
    val code     = call.code.trim
    val typeName = stripTemplateArguments(call.name)
    Option
      .when(code.startsWith(call.name) && code.drop(call.name.length).trim.startsWith("{") && typeName.nonEmpty) {
        normalizedQualifiedName(typeName)
      }
  }

  private def constructorTemporaryEntry(call: OxCall): Option[(String, FunctionEntry)] = {
    constructorTemporaryTypeFullName(call).flatMap(typeName =>
      constructorEntry(typeName, call.arguments).map(typeName -> _)
    )
  }

  private def temporaryDestructorAstsForExpressions(expressions: Seq[OxExpression]): Seq[Ast] = {
    expressions
      .flatMap(expression => temporaryDestructorsForExpression(expression))
      .reverse
      .map(temporaryDestructorAst)
  }

  private def temporaryDestructorAstsForReturnExpression(expression: Option[OxExpression]): Seq[Ast] = {
    expression.toSeq
      .flatMap(expression =>
        temporaryDestructorsForExpression(expression, includeCurrent = !isCurrentReturnedObjectTemporary(expression))
      )
      .reverse
      .map(temporaryDestructorAst)
  }

  private def temporaryDestructorAstsForLocalInitializer(
    expression: Option[OxExpression],
    extendCurrentTemporaryLifetime: Boolean
  ): Seq[Ast] = {
    expression.toSeq
      .flatMap(expression =>
        temporaryDestructorsForExpression(expression, includeCurrent = !extendCurrentTemporaryLifetime)
      )
      .reverse
      .map(temporaryDestructorAst)
  }

  private def temporaryDestructorsForExpression(
    expression: OxExpression,
    includeCurrent: Boolean = true
  ): Seq[TemporaryDestructor] = {
    val nested = expression match {
      case OxBinary(_, _, _, left, right) =>
        Seq(left, right).flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxUnary(_, _, _, _, argument) =>
        temporaryDestructorsForExpression(argument)
      case OxConditional(_, _, condition, consequence, alternative) =>
        val includeBranchCurrent = temporaryTypeFullNameForExpression(expression).isEmpty
        Seq(condition).flatMap(expression => temporaryDestructorsForExpression(expression)) ++
          consequence.toSeq.flatMap(expression =>
            temporaryDestructorsForExpression(expression, includeCurrent = includeBranchCurrent)
          ) ++
          Seq(alternative).flatMap(expression =>
            temporaryDestructorsForExpression(expression, includeCurrent = includeBranchCurrent)
          )
      case OxFold(_, _, _, left, right) =>
        left.toSeq.flatMap(expression => temporaryDestructorsForExpression(expression)) ++
          right.toSeq.flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxPackExpansion(_, _, pattern) =>
        temporaryDestructorsForExpression(pattern)
      case OxTypeOf(_, _, argument) =>
        temporaryDestructorsForExpression(argument)
      case cast @ OxCast(_, _, _, value) =>
        val castType            = temporaryTypeFullNameForExpression(cast)
        val valueType           = temporaryTypeFullNameForExpression(value)
        val includeValueCurrent = castType.isEmpty || castType != valueType
        temporaryDestructorsForExpression(value, includeCurrent = includeValueCurrent)
      case OxSizeOf(_, _, value, _) =>
        value.toSeq.flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxNew(_, _, _, arguments, _) =>
        arguments.flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxDelete(_, _, argument) =>
        temporaryDestructorsForExpression(argument)
      case _: OxLambda =>
        Seq.empty
      case OxCall(_, _, _, callee, arguments) =>
        temporaryDestructorsForExpression(callee) ++
          arguments.flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxFieldAccess(_, _, _, base) =>
        temporaryDestructorsForExpression(base)
      case OxIndexAccess(_, _, base, index) =>
        Seq(base, index).flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxInitializerList(_, _, elements) =>
        elements.flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxDesignatedInitializer(_, _, designator, value) =>
        Seq(designator, value).flatMap(expression => temporaryDestructorsForExpression(expression))
      case _: OxIdentifier | _: OxLiteral | _: OxDesignator =>
        Seq.empty
    }
    val current = expression match {
      case expression if includeCurrent =>
        temporaryTypeFullNameForExpression(expression)
          .flatMap(destructorEntryForType)
          .map(entry => TemporaryDestructor(temporaryDestructorCode(expression, entry), expression.line, entry))
          .toSeq
      case _ =>
        Seq.empty
    }
    nested ++ current
  }

  private def isCurrentReturnedObjectTemporary(expression: OxExpression): Boolean = {
    val currentReturnType = currentMethodReturnedObjectTypeFullName
    val expressionType    = temporaryTypeFullNameForExpression(expression)
    currentReturnType.isDefined && currentReturnType == expressionType
  }

  private def referenceBoundTemporaryDestructor(
    localTypeName: String,
    expression: OxExpression
  ): Option[LocalDestructor] = {
    Option
      .when(isCxxReferenceType(localTypeName)) {
        temporaryTypeFullNameForExpression(expression)
          .flatMap(destructorEntryForType)
          .map(entry => LocalDestructor(temporaryDestructorReceiverCode(expression), expression.line, entry))
      }
      .flatten
  }

  private def isCxxReferenceType(typeName: String): Boolean = {
    val normalizedType = normalizeType(resolveAliasType(typeName))
    normalizedType.endsWith("&") || normalizedType.endsWith("&&")
  }

  private def currentMethodReturnedObjectTypeFullName: Option[String] = {
    currentMethodReturnTypeFullName
      .map(typeName => normalizeType(resolveAliasType(typeName)))
      .flatMap(returnedObjectTypeFullName)
  }

  private def temporaryTypeFullNameForExpression(expression: OxExpression): Option[String] = {
    expression match {
      case call: OxCall               => temporaryTypeFullNameForCall(call)
      case conditional: OxConditional => conditionalTemporaryTypeFullName(conditional)
      case cast: OxCast               => castTemporaryTypeFullName(cast)
      case _                          => None
    }
  }

  private def temporaryTypeFullNameForCall(call: OxCall): Option[String] = {
    constructorTemporaryTypeFullName(call).orElse(returnedObjectTemporaryTypeFullName(call))
  }

  private def conditionalTemporaryTypeFullName(conditional: OxConditional): Option[String] = {
    conditional.consequence.flatMap { consequence =>
      val branchTypes =
        Seq(consequence, conditional.alternative).map(temporaryTypeFullNameForExpression)
      Option
        .when(branchTypes.forall(_.isDefined)) {
          branchTypes.flatten.distinct
        }
        .collect { case Seq(typeName) => typeName }
    }
  }

  private def castTemporaryTypeFullName(cast: OxCast): Option[String] = {
    Option(normalizeType(resolveAliasType(cast.typeName))).flatMap(returnedObjectTypeFullName)
  }

  private def temporaryDestructorCode(expression: OxExpression, entry: FunctionEntry): String = {
    s"${temporaryDestructorReceiverCode(expression)}.${entry.simpleName}()"
  }

  private def temporaryDestructorReceiverCode(expression: OxExpression): String = {
    expression match {
      case _: OxConditional | _: OxCast => s"(${expression.code})"
      case _                            => expression.code
    }
  }

  private def returnedObjectTemporaryTypeFullName(call: OxCall): Option[String] = {
    callReturnTypeFullName(call)
      .map(typeName => normalizeType(resolveAliasType(typeName)))
      .flatMap(returnedObjectTypeFullName)
  }

  private def returnedObjectTypeFullName(typeName: String): Option[String] = {
    Option(typeName)
      .filterNot(typeName =>
        typeName == Defines.Void ||
          typeName.endsWith("*") ||
          typeName.endsWith("[]") ||
          typeName.endsWith("&") ||
          typeName.endsWith("&&")
      )
      .flatMap(typeName => resolveAggregateTypeFullName(receiverAggregateTypeName(typeName)))
  }

  private def temporaryDestructorAst(destructor: TemporaryDestructor): Ast = {
    val callNode_ =
      callNode(
        OxOrigin(destructor.code, Option(destructor.line)),
        destructor.code,
        destructor.entry.simpleName,
        destructor.entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(destructor.entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(callNode_)
  }

  private def destructorEntryForType(typeName: String): Option[FunctionEntry] = {
    val normalizedType = normalizeType(resolveAliasType(typeName))
    val isObjectType = !(
      normalizedType.endsWith("*") ||
        normalizedType.endsWith("[]") ||
        normalizedType.endsWith("&") ||
        normalizedType.endsWith("&&")
    )
    Option
      .when(isObjectType) {
        val receiverType   = receiverAggregateTypeName(typeName)
        val aggregateType  = resolveAggregateTypeFullName(receiverType).getOrElse(receiverType)
        val destructorName = s"~${aggregateType.split('.').lastOption.getOrElse(aggregateType)}"
        functionCandidatesByQualifiedName(s"$aggregateType.$destructorName").lastOption
      }
      .flatten
  }

  private def statementBlockAst(
    statements: Seq[OxStatement],
    code: String,
    line: Int,
    extraDestructorsOnNormalCompletion: Seq[LocalDestructor] = Seq.empty
  ): Ast = {
    inNestedScopeWithDestructors {
      val statementAsts = statements.flatMap(astsForStatement)
      val destructorAsts =
        Option
          .when(statementsMayCompleteNormally(statements))(
            currentLocalDestructors.reverse.map(localDestructorAst) ++
              extraDestructorsOnNormalCompletion.reverse.map(localDestructorAst)
          )
          .getOrElse(Vector.empty)
      blockAst(blockNode(OxOrigin(code, Option(line)), code, Defines.Any), (statementAsts ++ destructorAsts).toList)
    }
  }

  private def catchAst(catchClause: OxCatchClause): Ast = {
    val catchNode =
      controlStructureNode(OxOrigin("catch", Option(catchClause.line)), ControlStructureTypes.CATCH, "catch")
    inNestedScopeWithDestructors {
      val parameterAsts = catchClause.parameter.toSeq.map(catchParameterAst)
      val bodyAst       = statementBlockAst(catchClause.body, "catch", catchClause.line)
      val destructorAsts =
        Option
          .when(statementsMayCompleteNormally(catchClause.body))(
            currentLocalDestructors.reverse.map(localDestructorAst)
          )
          .getOrElse(Vector.empty)
      Ast(catchNode).withChildren(parameterAsts).withChild(bodyAst).withChildren(destructorAsts)
    }
  }

  private def catchParameterAst(parameter: OxParameterDecl): Ast = {
    val typeName = registerType(normalizeType(parameter.typeName))
    val node     = localNode(OxOrigin(parameter.code, Option(parameter.line)), parameter.name, parameter.code, typeName)
    scope = scope.updated(parameter.name, ScopeEntry(typeName, node))
    registerLocalDestructor(parameter.name, typeName, parameter.line)
    Ast(node)
  }

  private def statementsMayCompleteNormally(statements: Seq[OxStatement]): Boolean = {
    statements.foldLeft(true) {
      case (true, statement) => statementMayCompleteNormally(statement)
      case (false, _)        => false
    }
  }

  private def statementMayCompleteNormally(statement: OxStatement): Boolean = {
    statement match {
      case _: OxReturn | _: OxThrow | _: OxBreak | _: OxContinue | _: OxGoto => false
      case tryStmt: OxTry =>
        statementsMayCompleteNormally(tryStmt.body) || tryStmt.catches.exists(catchClause =>
          statementsMayCompleteNormally(catchClause.body)
        )
      case ifStmt: OxIf =>
        ifStmt.elseBody.isEmpty ||
        statementsMayCompleteNormally(ifStmt.thenBody) ||
        statementsMayCompleteNormally(ifStmt.elseBody)
      case _ => true
    }
  }

  private def withJumpCleanupTarget[T](target: JumpCleanupTarget)(body: => T): T = {
    val outerJumpCleanupTargets = jumpCleanupTargets
    jumpCleanupTargets = target :: jumpCleanupTargets
    try body
    finally {
      jumpCleanupTargets = outerJumpCleanupTargets
    }
  }

  private def inNestedScope[T](body: => T): T = {
    val outerScope            = scope
    val outerDestructorScopes = localDestructorScopes
    localDestructorScopes = Vector.empty[LocalDestructor] :: localDestructorScopes
    try body
    finally {
      localDestructorScopes = outerDestructorScopes
      scope = outerScope
    }
  }

  private def inNestedScopeCollectingDestructors[T](body: => T): (T, Vector[LocalDestructor]) = {
    val outerScope            = scope
    val outerDestructorScopes = localDestructorScopes
    localDestructorScopes = Vector.empty[LocalDestructor] :: localDestructorScopes
    try {
      val result      = body
      val destructors = currentLocalDestructors
      (result, destructors)
    } finally {
      localDestructorScopes = outerDestructorScopes
      scope = outerScope
    }
  }

  private def inNestedScopeWithDestructors[T](body: => T): T = {
    val outerScope            = scope
    val outerDestructorScopes = localDestructorScopes
    localDestructorScopes = Vector.empty[LocalDestructor] :: localDestructorScopes
    try body
    finally {
      localDestructorScopes = outerDestructorScopes
      scope = outerScope
    }
  }

  private def conditionExpressionAst(expression: OxExpression): Ast = {
    wrapConditionInNullComparison(expression, expressionAst(expression))
  }

  private def conditionExpressionAstWithInitializers(
    initializers: Seq[OxStatement],
    expression: OxExpression,
    wrapTruthy: Boolean = true
  ): Ast = {
    def conditionAstForExpression: Ast = {
      if (wrapTruthy) conditionExpressionAst(expression) else expressionAst(expression)
    }

    if (initializers.isEmpty) {
      conditionAstForExpression
    } else {
      val initializerAsts     = initializers.flatMap(astsForStatement)
      val heapConstructorAsts = heapConstructorAstsForExpressions(Seq(expression))
      val conditionAst        = conditionAstForExpression
      val conditionCode = conditionAst.root
        .collect { case expressionNode: ExpressionNew =>
          expressionNode.code
        }
        .getOrElse(expression.code)
      blockAst(
        blockNode(OxOrigin(conditionCode, Option(expression.line)), conditionCode, Defines.Any),
        (initializerAsts ++ heapConstructorAsts :+ conditionAst).toList
      )
    }
  }

  private def wrapConditionInNullComparison(expression: OxExpression, conditionAst: Ast): Ast = {
    def isWrapCandidate(ast: Ast): Boolean = {
      ast.root match {
        case Some(_: NewCall)    => false
        case Some(_: NewBlock)   => false
        case Some(_: NewLiteral) => false
        case _                   => true
      }
    }

    if (conditionAst.root.isEmpty) {
      conditionAst
    } else {
      booleanConversionOperatorAst(expression).getOrElse {
        if (!isWrapCandidate(conditionAst)) {
          conditionAst
        } else {
          val (literalCode, literalType) = conditionAst.root match {
            case Some(identifier: NewIdentifier) if identifier.typeFullName.endsWith("*") => "NULL" -> Defines.Any
            case _                                                                        => "0"    -> "int"
          }
          val literalAst_ =
            Ast(literalNode(OxOrigin(literalCode, Option(expression.line)), literalCode, registerType(literalType)))
          val comparisonCode = s"${expression.code} != $literalCode"
          val call =
            callNode(
              OxOrigin(comparisonCode, Option(expression.line)),
              comparisonCode,
              Operators.notEquals,
              Operators.notEquals,
              DispatchTypes.STATIC_DISPATCH,
              None,
              Some(registerType("int"))
            )
          callAst(call, Seq(conditionAst, literalAst_))
        }
      }
    }
  }

  private def expressionAst(expression: OxExpression): Ast = {
    expression match {
      case identifier: OxIdentifier =>
        objectLikeMacroAst(identifier).getOrElse(identifierAst(identifier.name, identifier.code, identifier.line))
      case literal: OxLiteral =>
        Ast(literalNode(OxOrigin(literal), literal.code, literalType(literal.value)))
      case binary: OxBinary =>
        overloadedBinaryOperatorAst(binary).getOrElse(
          operatorCallAst(OxOrigin(binary), binary.code, operatorFor(binary.operator), binaryOperandAsts(binary))
        )
      case unary: OxUnary =>
        operatorCallAst(
          OxOrigin(unary),
          unary.code,
          unaryOperatorFor(unary.operator, unary.prefix),
          Seq(unaryOperandAst(unary))
        )
      case conditional: OxConditional =>
        operatorCallAst(
          OxOrigin(conditional),
          conditional.code,
          Operators.conditional,
          Seq(contextualBooleanAst(conditional.condition)) ++
            conditional.consequence.toSeq.map(expressionAst) ++
            Seq(expressionAst(conditional.alternative))
        )
      case fold: OxFold =>
        foldAst(fold)
      case packExpansion: OxPackExpansion =>
        expressionAst(packExpansion.pattern)
      case typeOf: OxTypeOf =>
        operatorCallAst(OxOrigin(typeOf), typeOf.code, Defines.OperatorTypeOf, Seq(expressionAst(typeOf.argument)))
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
      case newExpression: OxNew =>
        val typeArgument =
          Ast(
            literalNode(
              OxOrigin(newExpression.typeName, Option(newExpression.line)),
              newExpression.typeName,
              registerType(Defines.Any)
            )
          )
        operatorCallAst(
          OxOrigin(newExpression),
          newExpression.code,
          Operators.alloc,
          typeArgument +: newExpression.arguments.map(expressionAst)
        )
      case deleteExpression: OxDelete =>
        operatorCallAst(
          OxOrigin(deleteExpression),
          deleteExpression.code,
          Operators.delete,
          Seq(expressionAst(deleteExpression.argument)),
          typeFullName = registerType(Defines.Void)
        )
      case lambda: OxLambda =>
        lambdaExpressionAst(lambda)
      case call: OxCall =>
        astForCallExpression(call)
      case fieldAccess: OxFieldAccess =>
        fieldAccessAstForOperator(
          OxOrigin(fieldAccess),
          OxOrigin(fieldIdentifierCode(fieldAccess), Option(fieldAccess.line)),
          expressionAst(fieldAccess.base),
          fieldAccess.code,
          fieldAccess.field,
          registerType(expressionTypeFullName(fieldAccess).getOrElse(Defines.Any))
        )
      case indexAccess: OxIndexAccess =>
        overloadedIndexOperatorAst(indexAccess).getOrElse {
          val operatorName = Operators.indirectIndexAccess
          operatorCallAst(
            OxOrigin(indexAccess),
            indexAccess.code,
            operatorName,
            Seq(expressionAst(indexAccess.base), expressionAst(indexAccess.index))
          )
        }
      case initializerList: OxInitializerList =>
        operatorCallAst(
          OxOrigin(initializerList),
          initializerList.code,
          Operators.arrayInitializer,
          initializerList.elements.map(expressionAst),
          registerType(expressionTypeFullName(initializerList).getOrElse(Defines.Any))
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

  private def binaryOperandAsts(binary: OxBinary): Seq[Ast] = {
    val operandAst: OxExpression => Ast =
      if (Set("&&", "||", "and", "or").contains(binary.operator)) contextualBooleanAst
      else expressionAst
    Seq(operandAst(binary.left), operandAst(binary.right))
  }

  private def unaryOperandAst(unary: OxUnary): Ast = {
    if (unary.operator == "!") contextualBooleanAst(unary.argument) else expressionAst(unary.argument)
  }

  private def contextualBooleanAst(expression: OxExpression): Ast = {
    booleanConversionOperatorAst(expression).getOrElse(expressionAst(expression))
  }

  private def foldAst(fold: OxFold): Ast = {
    val foldOperator = "<operator>.fold"
    val callNode_ =
      callNode(
        OxOrigin(fold),
        fold.code,
        foldOperator,
        foldOperator,
        DispatchTypes.STATIC_DISPATCH,
        None,
        Option(registerType(foldExpressionTypeFullName(fold).getOrElse(Defines.Any)))
      )
    val operatorName = operatorFor(fold.operator)
    val operatorRef =
      methodRefNode(OxOrigin(operatorName, Option(fold.line)), operatorName, operatorName, registerType(operatorName))
    val operands = (fold.left, fold.right) match {
      case (Some(left), None)  => Seq(expressionAst(left), expressionAst(left))
      case (None, Some(right)) => Seq(expressionAst(right), expressionAst(right))
      case (left, right)       => left.toSeq.map(expressionAst) ++ right.toSeq.map(expressionAst)
    }
    callAst(callNode_, Ast(operatorRef) +: operands)
  }

  private def astForCallExpression(call: OxCall): Ast = {
    lambdaCallAst(call).getOrElse {
      overloadedCallOperatorAst(call).getOrElse {
        if (isPointerCall(call)) pointerCallAst(call) else directCallAst(call)
      }
    }
  }

  private def lambdaCallAst(call: OxCall): Option[Ast] = {
    lambdaCallableInfo(call.callee).map { info =>
      val callNode_ =
        callNode(
          OxOrigin(call),
          call.code,
          Defines.OperatorCall,
          s"${Defines.OperatorCall}:${info.signature}",
          DispatchTypes.DYNAMIC_DISPATCH,
          Option(info.signature),
          Option(registerType(info.returnType))
        )
      createCallAst(callNode_, call.arguments.map(expressionAst), receiver = Option(expressionAst(call.callee)))
    }
  }

  private def lambdaExpressionAst(lambda: OxLambda): Ast = {
    val info     = lambdaInfo(lambda)
    val captures = lambdaCaptures(lambda, info)
    emitLambda(lambda, info, captures)
    val methodRef = methodRefNode(OxOrigin(lambda), info.fullName, info.fullName, info.fullName)
    captures.foldLeft(Ast(methodRef)) { case (ast, capture) =>
      val bindingAst = capture.outer
        .map(outer => Ast(capture.binding).withRefEdge(capture.binding, outer.declaration))
        .getOrElse(Ast(capture.binding))
      ast
        .withCaptureEdge(methodRef, capture.binding)
        .merge(bindingAst)
    }
  }

  private def lambdaInfo(lambda: OxLambda): LambdaInfo = {
    lambdaInfos.getOrElseUpdate(
      lambdaKey(lambda), {
        val name       = nextClosureName()
        val owner      = currentMethodFullName.getOrElse(globalNamespaceBlock().fullName)
        val returnType = registerType(normalizeType(lambda.returnType))
        val signature  = lambda.signature
        val fullName   = s"$owner.$name:$signature"
        lambdaReturnTypesByFullName.update(fullName, returnType)
        lambdaSignaturesByFullName.update(fullName, signature)
        LambdaInfo(name, fullName, signature, returnType)
      }
    )
  }

  private def lambdaKey(lambda: OxLambda): String = {
    s"${currentMethodFullName.getOrElse(globalNamespaceBlock().fullName)}:${lambda.line}:${lambda.code}"
  }

  private def lambdaCaptures(lambda: OxLambda, info: LambdaInfo): Seq[LambdaCapture] = {
    val requestedCaptures = mutable.LinkedHashMap.empty[String, LambdaCaptureRequest]
    lambda.captures.foreach { capture =>
      capture.name.foreach { name =>
        requestedCaptures.update(
          name,
          LambdaCaptureRequest(name, lambdaCaptureEvaluationStrategy(capture), capture.initializer)
        )
      }
    }
    lambdaDefaultCaptureEvaluationStrategy(lambda).foreach { strategy =>
      inferredLambdaCaptureNames(lambda).foreach { name =>
        if (!requestedCaptures.contains(name)) {
          val captureStrategy = if (name == Defines.This) EvaluationStrategies.BY_SHARING else strategy
          requestedCaptures.update(name, LambdaCaptureRequest(name, captureStrategy, None))
        }
      }
    }
    requestedCaptures.values.toSeq.flatMap { request =>
      lambdaCapture(request, info, lambda)
    }
  }

  private def lambdaCapture(
    request: LambdaCaptureRequest,
    info: LambdaInfo,
    lambda: OxLambda
  ): Option[LambdaCapture] = {
    val outerEntry = scope
      .get(request.name)
      .orElse(request.initializer.collect { case OxIdentifier(name, _, _) => name }.flatMap(scope.get))
    val typeName = outerEntry
      .map(_.typeFullName)
      .orElse(request.initializer.flatMap(expressionTypeFullName))
      .map(typeName => registerType(normalizeType(typeName)))
      .getOrElse(Defines.Any)
    if (outerEntry.isEmpty && request.initializer.isEmpty) {
      return None
    }
    val captureName = request.name
    val bindingId   = s"${info.fullName}:$captureName"
    val local =
      localNode(OxOrigin(captureName, Option(lambda.line)), captureName, captureName, typeName)
        .closureBindingId(bindingId)
    val binding = NewClosureBinding()
      .closureBindingId(bindingId)
      .evaluationStrategy(request.evaluationStrategy)
    Some(
      LambdaCapture(
        captureName,
        ScopeEntry(typeName, local, outerEntry.flatMap(_.lambdaInfo)),
        binding,
        outerEntry,
        request.evaluationStrategy
      )
    )
  }

  private def lambdaCaptureEvaluationStrategy(capture: OxLambdaCapture): String = {
    capture.captureKind match {
      case "defaultByReference" | "explicitByReference" | "initByReference" => EvaluationStrategies.BY_REFERENCE
      case "this"                                                           => EvaluationStrategies.BY_SHARING
      case _                                                                => EvaluationStrategies.BY_VALUE
    }
  }

  private def lambdaDefaultCaptureEvaluationStrategy(lambda: OxLambda): Option[String] = {
    lambda.captures.collectFirst {
      case capture if capture.captureKind == "defaultByReference" => EvaluationStrategies.BY_REFERENCE
      case capture if capture.captureKind == "defaultByValue"     => EvaluationStrategies.BY_VALUE
    }
  }

  private def inferredLambdaCaptureNames(lambda: OxLambda): Seq[String] = {
    val declared = mutable.Set.from(lambda.parameters.map(_.name).filter(_.nonEmpty))
    val names    = mutable.LinkedHashSet.empty[String]

    def reference(name: String): Unit = {
      if (!declared.contains(name) && scope.contains(name)) {
        names.add(name)
      }
    }

    def visitStatement(statement: OxStatement): Unit = {
      statement match {
        case OxLocalDecl(name, _, _, _, initializer) =>
          initializer.foreach(visitExpression)
          declared.add(name)
        case OxStructuredBinding(_, _, _, _, names, initializer) =>
          initializer.foreach(visitExpression)
          declared.addAll(names)
        case OxAssignment(_, _, _, left, right) =>
          visitExpression(left)
          visitExpression(right)
        case OxReturn(_, _, expression) =>
          expression.foreach(visitExpression)
        case OxThrow(_, _, expression) =>
          expression.foreach(visitExpression)
        case OxTry(_, _, body, catches) =>
          body.foreach(visitStatement)
          catches.foreach { catchClause =>
            val previousDeclarations = declared.toSet
            catchClause.parameter.foreach(parameter => declared.add(parameter.name))
            catchClause.body.foreach(visitStatement)
            declared.filterInPlace(previousDeclarations.contains)
          }
        case OxIf(_, _, initializer, conditionInitializer, condition, thenBody, elseBody) =>
          initializer.foreach(visitStatement)
          conditionInitializer.foreach(visitStatement)
          visitExpression(condition)
          thenBody.foreach(visitStatement)
          elseBody.foreach(visitStatement)
        case OxWhile(_, _, initializer, conditionInitializer, condition, body) =>
          initializer.foreach(visitStatement)
          conditionInitializer.foreach(visitStatement)
          visitExpression(condition)
          body.foreach(visitStatement)
        case OxDoWhile(_, _, condition, body) =>
          visitExpression(condition)
          body.foreach(visitStatement)
        case OxFor(_, _, initializer, condition, update, body) =>
          initializer.foreach(visitStatement)
          condition.foreach(visitExpression)
          update.foreach(visitExpression)
          body.foreach(visitStatement)
        case OxLabel(_, _, _, body) =>
          body.foreach(visitStatement)
        case OxSwitch(_, _, initializer, conditionInitializer, condition, body) =>
          initializer.foreach(visitStatement)
          conditionInitializer.foreach(visitStatement)
          visitExpression(condition)
          body.foreach(visitStatement)
        case OxCase(_, _, value, body) =>
          value.foreach(visitExpression)
          body.foreach(visitStatement)
        case OxExpressionStatement(_, _, expression) =>
          visitExpression(expression)
        case _: OxUsingEnumStatement                =>
        case _: OxUnknownStatement                  =>
        case _: OxBreak | _: OxContinue | _: OxGoto =>
      }
    }

    def visitExpression(expression: OxExpression): Unit = {
      expression match {
        case OxIdentifier(name, _, _) =>
          reference(name)
        case OxLiteral(_, _, _) =>
        case OxBinary(_, _, _, left, right) =>
          visitExpression(left)
          visitExpression(right)
        case OxUnary(_, _, _, _, argument) =>
          visitExpression(argument)
        case OxConditional(_, _, condition, consequence, alternative) =>
          visitExpression(condition)
          consequence.foreach(visitExpression)
          visitExpression(alternative)
        case OxFold(_, _, _, left, right) =>
          left.foreach(visitExpression)
          right.foreach(visitExpression)
        case OxPackExpansion(_, _, pattern) =>
          visitExpression(pattern)
        case OxTypeOf(_, _, argument) =>
          visitExpression(argument)
        case OxCast(_, _, _, value) =>
          visitExpression(value)
        case OxSizeOf(_, _, value, _) =>
          value.foreach(visitExpression)
        case OxNew(_, _, _, arguments, initializerArguments) =>
          arguments.foreach(visitExpression)
          initializerArguments.foreach(visitExpression)
        case OxDelete(_, _, argument) =>
          visitExpression(argument)
        case _: OxLambda =>
        case OxCall(_, _, _, callee, arguments) =>
          visitExpression(callee)
          arguments.foreach(visitExpression)
        case OxFieldAccess(_, _, _, base) =>
          visitExpression(base)
        case OxIndexAccess(_, _, base, index) =>
          visitExpression(base)
          visitExpression(index)
        case OxInitializerList(_, _, elements) =>
          elements.foreach(visitExpression)
        case OxDesignatedInitializer(_, _, designator, value) =>
          visitExpression(designator)
          visitExpression(value)
        case OxDesignator(_, _, _) =>
      }
    }

    lambda.body.foreach(visitStatement)
    names.toSeq
  }

  private def emitLambda(lambda: OxLambda, info: LambdaInfo, captures: Seq[LambdaCapture]): Unit = {
    if (!emittedLambdaFullNames.add(info.fullName)) {
      return
    }

    val origin = OxOrigin(lambda)
    val method =
      methodNode(
        origin,
        info.name,
        lambda.code,
        info.fullName,
        Option(info.signature),
        filename,
        Option(NodeTypes.TYPE_DECL),
        Option(info.fullName)
      )
    val parameterEntries = lambda.parameters.zipWithIndex.map { case (parameter, index) =>
      val parameterType = registerType(normalizeType(parameter.typeName))
      val node =
        parameterInNode(
          OxOrigin(parameter.code, Option(parameter.line)),
          parameter.name,
          parameter.code,
          index + 1,
          isVariadic = false,
          EvaluationStrategies.BY_VALUE,
          parameterType
        )
      parameter.name -> (parameterType, Ast(node), node)
    }

    val previousScope            = scope
    val previousCaptureContext   = functionCaptureContext
    val previousMethodOwner      = currentMethodOwnerTypeFullName
    val previousMethodFullName   = currentMethodFullName
    val previousMethodReturnType = currentMethodReturnTypeFullName
    val previousDestructorScopes = localDestructorScopes
    val previousJumpTargets      = jumpCleanupTargets
    scope = (captures.map(capture => capture.name -> capture.scopeEntry) ++
      parameterEntries.map { case (name, (typeName, _, node)) => name -> ScopeEntry(typeName, node) }).toMap
    functionCaptureContext = None
    currentMethodOwnerTypeFullName = None
    currentMethodFullName = Option(info.fullName)
    currentMethodReturnTypeFullName = Option(info.returnType)
    localDestructorScopes = Vector.empty[LocalDestructor] :: Nil
    jumpCleanupTargets = Nil
    val bodyAsts =
      try {
        lambda.body.flatMap(astsForStatement)
      } finally {
        localDestructorScopes = previousDestructorScopes
        jumpCleanupTargets = previousJumpTargets
        currentMethodReturnTypeFullName = previousMethodReturnType
        currentMethodFullName = previousMethodFullName
        currentMethodOwnerTypeFullName = previousMethodOwner
        functionCaptureContext = previousCaptureContext
        scope = previousScope
      }

    val captureLocalAsts = captures.map(capture => Ast(capture.scopeEntry.declaration))
    val body             = blockAst(blockNode(origin, lambda.code, Defines.Any), (captureLocalAsts ++ bodyAsts).toList)
    val methodReturn     = methodReturnNode(origin, info.returnType)
    val modifierTypes =
      Seq(ModifierTypes.VIRTUAL, ModifierTypes.STATIC, ModifierTypes.PRIVATE, ModifierTypes.LAMBDA) ++
        Option.when(lambda.isMutable)(LambdaMutableModifier)
    val modifiers = modifierTypes.map(modifier => modifierNode(origin, modifier))
    val methodAst_ =
      methodAst(method, parameterEntries.map(_._2._2), body, methodReturn, modifiers)

    val typeDecl =
      typeDeclNode(
        origin,
        info.name,
        info.fullName,
        filename,
        lambda.code,
        NodeTypes.NAMESPACE_BLOCK,
        globalNamespaceBlock().fullName,
        Seq(registerType(Defines.Function))
      )
    val binding = NewBinding()
      .name(Defines.OperatorCall)
      .methodFullName(info.fullName)
      .signature(info.signature)
    val bindingAst = Ast(binding)
      .withBindsEdge(typeDecl, binding)
      .withRefEdge(binding, method)

    Ast.storeInDiffGraph(Ast(typeDecl).withChild(methodAst_), diffGraph)
    Ast.storeInDiffGraph(bindingAst, diffGraph)
  }

  private def lambdaCallableInfo(expression: OxExpression): Option[LambdaInfo] = {
    expression match {
      case OxIdentifier(name, _, _) =>
        scope.get(name).flatMap(_.lambdaInfo).orElse(lambdaCallableInfoByType(expression))
      case _ => lambdaCallableInfoByType(expression)
    }
  }

  private def lambdaCallableInfoByType(expression: OxExpression): Option[LambdaInfo] = {
    expressionTypeFullName(expression).flatMap(lambdaCallableInfoByTypeFullName)
  }

  private def lambdaCallableInfoByTypeFullName(typeFullName: String): Option[LambdaInfo] = {
    for {
      signature  <- lambdaSignaturesByFullName.get(typeFullName)
      returnType <- lambdaReturnTypesByFullName.get(typeFullName)
    } yield LambdaInfo(
      typeFullName.split('.').lastOption.getOrElse(typeFullName).takeWhile(_ != ':'),
      typeFullName,
      signature,
      returnType
    )
  }

  private def booleanConversionOperatorAst(expression: OxExpression): Option[Ast] = {
    booleanConversionOperatorTarget(expression).map { target =>
      astForResolvedOperatorCall(OxOrigin(expression), s"${expression.code}.${target.name}()", target)
    }
  }

  private def booleanConversionOperatorTarget(expression: OxExpression): Option[ResolvedOperatorCall] = {
    val operatorName = "operator bool"
    selectFunctionEntry(memberFunctionCandidates(expression, operatorName), Some(Seq.empty))
      .filter(entry => normalizeType(entry.function.returnType) == "bool")
      .map(entry => ResolvedOperatorCall(entry, operatorName, Option(expression), Seq.empty))
  }

  private def overloadedBinaryOperatorAst(binary: OxBinary): Option[Ast] = {
    overloadedBinaryOperatorTarget(binary).map(target =>
      astForResolvedOperatorCall(OxOrigin(binary), binary.code, target)
    )
  }

  private def overloadedBinaryOperatorTarget(binary: OxBinary): Option[ResolvedOperatorCall] = {
    cxxOperatorFunctionName(binary.operator).flatMap { operatorName =>
      val memberTarget =
        selectFunctionEntry(memberFunctionCandidates(binary.left, operatorName), Some(Seq(binary.right)))
          .map(entry => ResolvedOperatorCall(entry, operatorName, Option(binary.left), Seq(binary.right)))
      memberTarget.orElse {
        selectFunctionEntry(freeFunctionCandidatesByName(operatorName), Some(Seq(binary.left, binary.right)))
          .map(entry => ResolvedOperatorCall(entry, operatorName, None, Seq(binary.left, binary.right)))
      }
    }
  }

  private def overloadedAssignmentOperatorAst(assignment: OxAssignment): Option[Ast] = {
    overloadedAssignmentOperatorTarget(assignment).map(target =>
      astForResolvedOperatorCall(OxOrigin(assignment), assignment.code, target)
    )
  }

  private def overloadedAssignmentOperatorTarget(assignment: OxAssignment): Option[ResolvedOperatorCall] = {
    cxxOperatorFunctionName(assignment.operator).flatMap { operatorName =>
      selectFunctionEntry(memberFunctionCandidates(assignment.left, operatorName), Some(Seq(assignment.right)))
        .map(entry => ResolvedOperatorCall(entry, operatorName, Option(assignment.left), Seq(assignment.right)))
    }
  }

  private def overloadedIndexOperatorAst(indexAccess: OxIndexAccess): Option[Ast] = {
    overloadedIndexOperatorTarget(indexAccess).map(target =>
      astForResolvedOperatorCall(OxOrigin(indexAccess), indexAccess.code, target)
    )
  }

  private def overloadedIndexOperatorTarget(indexAccess: OxIndexAccess): Option[ResolvedOperatorCall] = {
    val operatorName = "operator[]"
    selectFunctionEntry(memberFunctionCandidates(indexAccess.base, operatorName), Some(Seq(indexAccess.index)))
      .map(entry => ResolvedOperatorCall(entry, operatorName, Option(indexAccess.base), Seq(indexAccess.index)))
  }

  private def overloadedCallOperatorAst(call: OxCall): Option[Ast] = {
    overloadedCallOperatorTarget(call).map(target => astForResolvedOperatorCall(OxOrigin(call), call.code, target))
  }

  private def overloadedCallOperatorTarget(call: OxCall): Option[ResolvedOperatorCall] = {
    val operatorName = "operator()"
    selectFunctionEntry(memberFunctionCandidates(call.callee, operatorName), Some(call.arguments))
      .map(entry => ResolvedOperatorCall(entry, operatorName, Option(call.callee), call.arguments))
  }

  private def astForResolvedOperatorCall(origin: OxOrigin, code: String, target: ResolvedOperatorCall): Ast = {
    val dispatchType =
      if (isVirtualFunctionEntry(target.entry)) DispatchTypes.DYNAMIC_DISPATCH else DispatchTypes.STATIC_DISPATCH
    val callNode_ =
      callNode(
        origin.copy(code = code),
        code,
        target.name,
        target.entry.fullName,
        dispatchType,
        Option(target.entry.function.signature),
        Option(registerType(normalizeType(target.entry.function.returnType)))
      )
    val base = target.base.map(expressionAst)
    createCallAst(
      callNode_,
      target.arguments.map(expressionAst),
      base = base,
      receiver = if (dispatchType == DispatchTypes.DYNAMIC_DISPATCH) base else None
    )
  }

  private def memberFunctionCandidates(receiver: OxExpression, name: String): Seq[FunctionEntry] = {
    expressionTypeFullName(receiver)
      .map(receiverAggregateTypeName)
      .toSeq
      .flatMap(receiverType =>
        typeAndBaseTypeFullNames(receiverType)
          .flatMap(typeName => resolveAggregateTypeFullName(typeName).toSeq :+ typeName)
          .distinct
          .reverse
          .flatMap(typeName => functionCandidatesByQualifiedName(s"$typeName.$name"))
      )
  }

  private def receiverAggregateTypeName(typeName: String): String = {
    val normalized = normalizeType(resolveAliasType(typeName))
    stripTemplateArguments(stripCxxTypeQualifiers(stripCxxReference(normalized).stripSuffix("*").stripSuffix("[]")))
  }

  private def stripTemplateArguments(typeName: String): String = {
    val builder = new StringBuilder
    var depth   = 0
    typeName.foreach {
      case '<' =>
        depth += 1
      case '>' if depth > 0 =>
        depth -= 1
      case ch if depth == 0 =>
        builder.append(ch)
      case _ =>
    }
    if (depth == 0) builder.toString else typeName
  }

  private def stripCxxReference(typeName: String): String = {
    if (typeName.endsWith("&&")) typeName.dropRight(2)
    else if (typeName.endsWith("&")) typeName.dropRight(1)
    else typeName
  }

  private def stripCxxTypeQualifiers(typeName: String): String = {
    typeName
      .split("\\s+")
      .filterNot(part => Set("const", "volatile", "mutable").contains(part))
      .mkString(" ")
  }

  private def cxxOperatorFunctionName(operator: String): Option[String] = {
    Option.when(CxxOverloadableBinaryOperators.contains(operator))(s"operator$operator")
  }

  private def directCallAst(call: OxCall): Ast = {
    val (name, methodFullName, signature, typeFullName, dispatchType) = callTargetInfo(call)
    val callNode_ =
      callNode(
        OxOrigin(call),
        call.code,
        name,
        methodFullName,
        dispatchType,
        signature,
        Option(registerType(typeFullName))
      )
    val base = memberCallBaseAst(call)
    createCallAst(
      callNode_,
      call.arguments.map(expressionAst),
      base = base,
      receiver = if (dispatchType == DispatchTypes.DYNAMIC_DISPATCH) base else None
    )
  }

  private def memberCallBaseAst(call: OxCall): Option[Ast] = {
    call.callee match {
      case OxFieldAccess(_, _, _, base) => Option(expressionAst(base))
      case _                            => None
    }
  }

  private def createCallAst(
    callNode: NewCall,
    arguments: Seq[Ast] = Seq.empty,
    base: Option[Ast] = None,
    receiver: Option[Ast] = None
  ): Ast = {
    setArgumentIndices(arguments)

    val baseRoot = base.flatMap(_.root).toList
    baseRoot match {
      case List(expression: ExpressionNew) => expression.argumentIndex = 0
      case _                               =>
    }

    val baseAst = base.getOrElse(Ast())
    var ast     = Ast(callNode).withChild(baseAst)

    if (receiver.isDefined && receiver != base) {
      receiver.get.root.get.asInstanceOf[ExpressionNew].argumentIndex = -1
      ast = ast.withChild(receiver.get)
    }

    ast = ast
      .withChildren(arguments)
      .withArgEdges(callNode, baseRoot)
      .withArgEdges(callNode, arguments.flatMap(_.root))

    if (receiver.isDefined) {
      ast = ast.withReceiverEdge(callNode, receiver.get.root.get)
    }
    ast
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
      case _: OxFieldAccess               => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case OxUnary("*", _, _, _, _)       => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxUnary                     => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxIdentifier | _: OxCast    => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxCall | _: OxIndexAccess   => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxLambda                    => false
      case _: OxInitializerList           => false
      case _: OxDesignatedInitializer     => false
      case _: OxDesignator                => false
      case _: OxLiteral                   => false
      case OxPackExpansion(_, _, pattern) => expressionTypeFullName(pattern).exists(isFunctionPointerType)
      case _: OxBinary | _: OxConditional | _: OxFold | _: OxTypeOf | _: OxSizeOf | _: OxNew | _: OxDelete => false
    }
  }

  private def callReturnTypeFullName(call: OxCall): Option[String] = {
    lambdaCallableInfo(call.callee)
      .map(_.returnType)
      .orElse(constructorTemporaryTypeFullName(call))
      .orElse(
        overloadedCallOperatorTarget(call)
          .map(target => normalizeType(target.entry.function.returnType))
          .orElse(
            expressionTypeFullName(call.callee)
              .flatMap(returnTypeFromFunctionPointer)
              .orElse(functionEntryForCall(call).map(entry => normalizeType(entry.function.returnType)))
          )
      )
  }

  private def expressionTypeFullName(expression: OxExpression): Option[String] = {
    if (expressionTypeFullNameCache.containsKey(expression)) {
      expressionTypeFullNameCache.get(expression)
    } else {
      val typeFullName = expressionTypeFullNameUncached(expression)
      expressionTypeFullNameCache.put(expression, typeFullName)
      typeFullName
    }
  }

  private def expressionTypeFullNameUncached(expression: OxExpression): Option[String] = {
    expression match {
      case OxIdentifier(name, _, _) =>
        scope
          .get(name)
          .map(entry => resolveAliasType(entry.typeFullName))
          .orElse(staticFieldTypeFullName(name))
          .orElse(implicitFieldTypeFullName(name))
          .orElse(globalScopeByName.get(name).map(entry => resolveAliasType(entry.typeFullName)))
      case OxLiteral(value, _, _) =>
        Option(literalType(value))
      case fold: OxFold =>
        foldExpressionTypeFullName(fold)
      case OxPackExpansion(_, _, pattern) =>
        expressionTypeFullName(pattern)
      case OxTypeOf(_, _, _) =>
        None
      case OxFieldAccess(field, _, _, base) =>
        expressionTypeFullName(base).flatMap(typeName => fieldTypeFullName(typeName, field))
      case OxUnary("*", _, _, _, argument) =>
        expressionTypeFullName(argument).map(dereferencedTypeFullName)
      case OxUnary("&", _, _, _, argument) =>
        expressionTypeFullName(argument).map(typeName =>
          s"${stripCxxReference(normalizeType(resolveAliasType(typeName)))}*"
        )
      case OxCast(typeName, _, _, _) =>
        Option(resolveAliasType(typeName))
      case OxNew(typeName, _, _, _, _) =>
        Option(s"${normalizeType(resolveAliasType(typeName))}*")
      case lambda: OxLambda =>
        Option(lambdaInfo(lambda).fullName)
      case indexAccess: OxIndexAccess =>
        overloadedIndexOperatorTarget(indexAccess)
          .map(target => normalizeType(target.entry.function.returnType))
          .orElse(expressionTypeFullName(indexAccess.base).map(_.stripSuffix("[]")))
      case initializerList: OxInitializerList =>
        initializerListTypeFullName(initializerList)
      case call: OxCall =>
        callReturnTypeFullName(call)
      case binary: OxBinary =>
        overloadedBinaryOperatorTarget(binary)
          .map(target => normalizeType(target.entry.function.returnType))
          .orElse(binaryExpressionTypeFullName(binary))
      case _ =>
        None
    }
  }

  private def binaryExpressionTypeFullName(binary: OxBinary): Option[String] = {
    val leftType  = expressionTypeFullName(binary.left)
    val rightType = expressionTypeFullName(binary.right)
    (leftType, rightType) match {
      case (Some(left), Some(right)) if left == right => Some(left)
      case (Some("int"), _)                           => Some("int")
      case (_, Some("int"))                           => Some("int")
      case _                                          => None
    }
  }

  private def foldExpressionTypeFullName(fold: OxFold): Option[String] = {
    if (Set("&&", "||", "and", "or").contains(fold.operator)) Option(registerType("bool"))
    else fold.left.orElse(fold.right).flatMap(expressionTypeFullName)
  }

  private def initializerListTypeFullName(initializerList: OxInitializerList): Option[String] = {
    initializerListElementTypeFullName(initializerList).map(typeName => s"std.initializer_list<$typeName>")
  }

  private def initializerListElementTypeFullName(initializerList: OxInitializerList): Option[String] = {
    val elementTypes = initializerList.elements.map(expressionTypeFullName)
    Option
      .when(elementTypes.nonEmpty && elementTypes.forall(_.isDefined)) {
        elementTypes.flatten.map(normalizeType).distinct
      }
      .collect { case Seq(typeName) => typeName }
  }

  private def dereferencedTypeFullName(typeFullName: String): String = {
    val normalized = normalizeType(resolveAliasType(typeFullName))
    if (isFunctionPointerType(normalized)) normalized
    else if (normalized.endsWith("*")) normalized.stripSuffix("*")
    else normalized
  }

  private def fieldTypeFullName(baseTypeFullName: String, field: String): Option[String] = {
    fieldEntryForTypeHierarchy(baseTypeFullName, field).map { case (_, fieldDecl) =>
      resolveAliasType(fieldDecl.typeName)
    }
  }

  private def fieldEntryForTypeHierarchy(baseTypeFullName: String, field: String): Option[(String, OxFieldDecl)] = {
    val normalized = resolveAliasType(baseTypeFullName)
    val candidates = Seq(normalized, receiverAggregateTypeName(normalized)).distinct
    candidates
      .flatMap(typeAndBaseTypeFullNames)
      .collectFirst(Function.unlift { typeName =>
        aggregateFieldEntriesByType
          .get(normalizeType(typeName))
          .flatMap(_.get(field))
          .map(fieldDecl => normalizeType(typeName) -> fieldDecl)
      })
  }

  private def implicitFieldTypeFullName(name: String): Option[String] = {
    currentMethodOwnerTypeFullName.flatMap(ownerTypeFullName => fieldTypeFullName(s"$ownerTypeFullName*", name))
  }

  private def staticFieldTypeFullName(name: String): Option[String] = {
    staticFieldTarget(name).map { case (_, field) => resolveAliasType(field.typeName) }
  }

  private def staticFieldTarget(name: String): Option[(String, OxFieldDecl)] = {
    val parts = qualifiedNameParts(name)
    if (parts.size > 1) {
      val ownerName = parts.dropRight(1).mkString(".")
      val fieldName = parts.last
      resolveAggregateTypeFullName(ownerName).flatMap(ownerTypeFullName =>
        fieldEntryForTypeHierarchy(ownerTypeFullName, fieldName)
          .filter(_._2.isStatic)
      )
    } else {
      currentMethodOwnerTypeFullName.flatMap(ownerTypeFullName =>
        fieldEntryForTypeHierarchy(ownerTypeFullName, name)
          .filter(_._2.isStatic)
      )
    }
  }

  private def staticFieldOwnerCode(name: String, ownerTypeFullName: String): String = {
    qualifiedNameParts(name).dropRight(1) match {
      case parts if parts.nonEmpty => parts.mkString("::")
      case _                       => ownerTypeFullName
    }
  }

  private def resolveAggregateTypeFullName(typeName: String): Option[String] = {
    val normalized     = normalizeType(typeName)
    val templateErased = stripTemplateArguments(normalized)
    val candidates = currentMethodOwnerTypeFullName
      .filter(owner => Seq(normalized, templateErased).contains(owner.split('.').lastOption.getOrElse(owner)))
      .toSeq ++ Seq(normalized, templateErased) ++ aggregateTypeFullNames
      .filter(typeName => Seq(normalized, templateErased).exists(candidate => typeName.endsWith(s".$candidate")))
      .toSeq
      .sorted
    candidates.find(aggregateTypeFullNames.contains)
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
    else if (fieldAccess.code.contains("::")) fieldAccess.code.split("::").lastOption.getOrElse(fieldAccess.field)
    else fieldAccess.code.split('.').lastOption.getOrElse(fieldAccess.field)
  }

  private def fieldAccessAstForOperator(
    origin: OxOrigin,
    fieldIdentifierOrigin: OxOrigin,
    base: Ast,
    code: String,
    fieldName: String,
    fieldTypeFullName: String
  ): Ast = {
    val operatorName = if (code.contains("->")) Operators.indirectFieldAccess else Operators.fieldAccess
    val call =
      callNode(origin, code, operatorName, operatorName, DispatchTypes.STATIC_DISPATCH, None, Option(fieldTypeFullName))
    callAst(call, Seq(base, Ast(fieldIdentifierNode(fieldIdentifierOrigin, fieldName, fieldName))))
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
        staticFieldAccessAst(name, code, line)
          .orElse(implicitFieldAccessAst(name, line))
          .orElse(capturedGlobalIdentifierAst(name, code, line))
          .orElse(methodRefAst(name, code, line))
          .getOrElse {
            val identifier = identifierNode(OxOrigin(code, Option(line)), name, code, registerType(Defines.Any))
            Ast(identifier)
          }
    }
  }

  private def implicitFieldAccessAst(name: String, line: Int): Option[Ast] = {
    for {
      ownerTypeFullName <- currentMethodOwnerTypeFullName
      (_, field)        <- fieldEntryForTypeHierarchy(ownerTypeFullName, name)
      if !field.isStatic
      thisEntry <- scope.get(Defines.This)
    } yield {
      val thisIdentifier =
        identifierNode(
          OxOrigin(Defines.This, Option(line)),
          Defines.This,
          Defines.This,
          registerType(thisEntry.typeFullName)
        )
      val thisAst       = Ast(thisIdentifier).withRefEdge(thisIdentifier, thisEntry.declaration)
      val fieldTypeName = registerType(implicitFieldTypeFullName(name).getOrElse(Defines.Any))
      val code          = s"${Defines.This}->$name"
      val call =
        callNode(
          OxOrigin(code, Option(line)),
          code,
          Operators.indirectFieldAccess,
          Operators.indirectFieldAccess,
          DispatchTypes.STATIC_DISPATCH,
          None,
          Option(fieldTypeName)
        )
      callAst(call, Seq(thisAst, Ast(fieldIdentifierNode(OxOrigin(name, Option(line)), name, name))))
    }
  }

  private def staticFieldAccessAst(name: String, code: String, line: Int): Option[Ast] = {
    staticFieldTarget(name).map { case (ownerTypeFullName, field) =>
      val ownerCode = staticFieldOwnerCode(name, ownerTypeFullName)
      val accessCode =
        if (qualifiedNameParts(name).size > 1) code else s"$ownerTypeFullName.${field.name}"
      val ownerIdentifier =
        identifierNode(OxOrigin(ownerCode, Option(line)), ownerCode, ownerCode, registerType(ownerTypeFullName))
      fieldAccessAst(
        OxOrigin(accessCode, Option(line)),
        OxOrigin(field.name, Option(line)),
        Ast(ownerIdentifier),
        accessCode,
        field.name,
        registerType(resolveAliasType(field.typeName))
      )
    }
  }

  private def methodRefAst(name: String, code: String, line: Int): Option[Ast] = {
    selectFunctionEntry(currentOwnerFunctionCandidates(name), None)
      .orElse(selectFunctionEntry(functionCandidatesByName(name), None))
      .map { functionEntry =>
        Ast(
          methodRefNode(
            OxOrigin(code, Option(line)),
            code,
            functionEntry.fullName,
            registerType(normalizeType(functionEntry.function.returnType))
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
          val closureBindingId = s"${declarationFilename(context.function)}:${context.function.name}:$name"
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

  private def callTargetInfo(call: OxCall): (String, String, Option[String], String, String) = {
    constructorTemporaryEntry(call) match {
      case Some((typeName, functionEntry)) =>
        (
          functionEntry.simpleName,
          functionEntry.fullName,
          Option(functionEntry.function.signature),
          typeName,
          DispatchTypes.STATIC_DISPATCH
        )
      case None =>
        macroForUse(call.name, call.line) match {
          case Some(macroDecl) =>
            (
              callName(call),
              macroFullName(macroDecl),
              Option(macroSignature(macroDecl)),
              macroReturnTypeFullName(macroDecl),
              DispatchTypes.INLINED
            )
          case None =>
            functionEntryForCall(call) match {
              case Some(functionEntry) =>
                val dispatchType =
                  if (isVirtualFunctionEntry(functionEntry)) DispatchTypes.DYNAMIC_DISPATCH
                  else DispatchTypes.STATIC_DISPATCH
                (
                  callName(call),
                  functionEntry.fullName,
                  Option(functionEntry.function.signature),
                  normalizeType(functionEntry.function.returnType),
                  dispatchType
                )
              case None =>
                (
                  callName(call),
                  normalizedQualifiedName(call.name),
                  None,
                  callReturnTypeFullName(call).getOrElse(Defines.Any),
                  DispatchTypes.STATIC_DISPATCH
                )
            }
        }
    }
  }

  private def functionEntryForCall(call: OxCall): Option[FunctionEntry] = {
    call.callee match {
      case OxFieldAccess(field, _, _, base) =>
        val candidates = expressionTypeFullName(base)
          .map(receiverAggregateTypeName)
          .toSeq
          .flatMap(receiverType =>
            typeAndBaseTypeFullNames(receiverType).reverse.flatMap(typeName =>
              functionCandidatesByQualifiedName(s"$typeName.$field")
            )
          )
        selectFunctionEntry(candidates, Some(call.arguments))
      case _ =>
        val lookupName    = stripTemplateArguments(call.name)
        val qualifiedName = normalizedQualifiedName(lookupName)
        if (qualifiedNameParts(call.name).size > 1) {
          val candidates = functionCandidatesByQualifiedName(qualifiedName)
          selectFunctionEntry(
            if (candidates.nonEmpty) candidates else functionCandidatesByName(lookupName),
            Some(call.arguments)
          )
        } else {
          val ownerCandidates     = currentOwnerFunctionCandidates(lookupName)
          val qualifiedCandidates = functionCandidatesByQualifiedName(qualifiedName)
          val candidates =
            if (ownerCandidates.nonEmpty) ownerCandidates
            else if (qualifiedCandidates.nonEmpty) qualifiedCandidates
            else functionCandidatesByName(lookupName)
          selectFunctionEntry(candidates, Some(call.arguments))
        }
    }
  }

  private def currentOwnerFunctionCandidates(name: String): Seq[FunctionEntry] = {
    currentMethodOwnerTypeFullName.toSeq.flatMap { ownerTypeFullName =>
      typeAndBaseTypeFullNames(ownerTypeFullName).reverse.flatMap(typeName =>
        functionCandidatesByQualifiedName(s"$typeName.$name")
      )
    }
  }

  private def functionCandidatesByName(name: String): Seq[FunctionEntry] = {
    functionsByName.getOrElse(name, Seq.empty)
  }

  private def freeFunctionCandidatesByName(name: String): Seq[FunctionEntry] = {
    functionCandidatesByName(name).filterNot(_.ownerFullName.exists(aggregateTypeFullNames.contains))
  }

  private def functionCandidatesByQualifiedName(name: String): Seq[FunctionEntry] = {
    functionsByQualifiedName.getOrElse(name, Seq.empty)
  }

  private def selectFunctionEntry(
    candidates: Seq[FunctionEntry],
    arguments: Option[Seq[OxExpression]]
  ): Option[FunctionEntry] = {
    arguments match {
      case Some(arguments) =>
        val arityMatches = candidates.filter(_.function.parameters.size == arguments.size)
        val pool         = if (arityMatches.nonEmpty) arityMatches else candidates
        val argumentInfos =
          arguments.map(argument => ArgumentInfo(expressionTypeFullName(argument), expressionIsRvalue(argument)))
        pool.zipWithIndex
          .maxByOption { case (candidate, index) => (overloadScore(candidate, argumentInfos), index) }
          .map(_._1)
      case None =>
        candidates.lastOption
    }
  }

  private def overloadScore(candidate: FunctionEntry, argumentInfos: Seq[ArgumentInfo]): Int = {
    val arityPenalty = math.abs(candidate.function.parameters.size - argumentInfos.size) * -100
    arityPenalty + candidate.function.parameters
      .zip(argumentInfos)
      .map { case (parameter, argumentInfo) =>
        argumentInfo.typeFullName.map(typeCompatibilityScore(parameter.typeName, _, argumentInfo.isRvalue)).getOrElse(1)
      }
      .sum
  }

  private def typeCompatibilityScore(
    parameterTypeName: String,
    argumentTypeName: String,
    argumentIsRvalue: Boolean
  ): Int = {
    val parameterType = overloadComparableType(parameterTypeName)
    val argumentType  = overloadComparableType(argumentTypeName)
    val baseScore =
      if (isTemplateParameterComparableType(parameterType)) 2
      else if (parameterType == Defines.Any || argumentType == Defines.Any) 1
      else if (parameterType == argumentType) 4
      else if (parameterType.endsWith(s".$argumentType") || argumentType.endsWith(s".$parameterType")) 3
      else 0
    if (baseScore == 0) 0 else baseScore + referenceValueCategoryScore(parameterTypeName, argumentIsRvalue)
  }

  private def isTemplateParameterComparableType(typeName: String): Boolean = {
    typeName.matches("[A-Z][0-9]?")
  }

  private def referenceValueCategoryScore(parameterTypeName: String, argumentIsRvalue: Boolean): Int = {
    val parameterType = normalizeType(resolveAliasType(parameterTypeName))
    if (parameterType.endsWith("&&")) {
      if (argumentIsRvalue) 3 else -3
    } else if (parameterType.endsWith("&")) {
      if (argumentIsRvalue) 0 else 2
    } else {
      0
    }
  }

  private def expressionIsRvalue(expression: OxExpression): Boolean = {
    expression match {
      case _: OxIdentifier | _: OxFieldAccess | _: OxIndexAccess => false
      case OxUnary("*", _, _, _, _)                              => false
      case OxPackExpansion(_, _, pattern)                        => expressionIsRvalue(pattern)
      case _: OxTypeOf                                           => true
      case call: OxCall =>
        callReturnTypeFullName(call).map(typeNameIsRvalue).getOrElse(true)
      case OxCast(typeName, _, _, _) =>
        typeNameIsRvalue(typeName)
      case _ =>
        true
    }
  }

  private def typeNameIsRvalue(typeName: String): Boolean = {
    val normalized = normalizeType(typeName)
    !normalized.endsWith("&") || normalized.endsWith("&&")
  }

  private def overloadComparableType(typeName: String): String = {
    val dereferenced =
      if (typeName.endsWith("&&")) typeName.dropRight(2)
      else if (typeName.endsWith("&")) typeName.dropRight(1)
      else typeName
    stripTemplateArguments(resolveAliasType(dereferenced))
      .split("\\s+")
      .filterNot(part => Set("const", "volatile", "mutable").contains(part))
      .mkString(" ")
  }

  private def callName(call: OxCall): String = {
    call.callee match {
      case OxFieldAccess(field, _, _, _) => field
      case _ => stripTemplateArguments(qualifiedNameParts(call.name).lastOption.getOrElse(call.name))
    }
  }

  private def macroForUse(name: String, line: Int): Option[OxMacroDecl] = {
    macroDeclarations
      .filter(macroDecl => macroDecl.name == name && macroDecl.visibleLine <= line)
      .filterNot(macroDecl => macroIsUndefinedAtUse(macroDecl, line))
      .lastOption
  }

  private def macroIsUndefinedAtUse(macroDecl: OxMacroDecl, line: Int): Boolean = {
    macroUndefs.exists { macroUndef =>
      macroUndef.name == macroDecl.name &&
      macroUndefIsAfterMacro(macroUndef, macroDecl) &&
      macroUndef.visibleLine <= line
    }
  }

  private def macroUndefIsAfterMacro(macroUndef: OxMacroUndefDecl, macroDecl: OxMacroDecl): Boolean = {
    macroUndef.visibleLine > macroDecl.visibleLine ||
    (macroUndef.visibleLine == macroDecl.visibleLine &&
      macroUndef.sourcePath == macroDecl.sourcePath &&
      macroUndef.line > macroDecl.line)
  }

  private def macroFullName(macroDecl: OxMacroDecl): String = {
    s"${macroFilename(macroDecl)}:${macroDecl.name}:${macroSignature(macroDecl)}"
  }

  private def macroFilename(macroDecl: OxMacroDecl): String = {
    declarationFilename(macroDecl)
  }

  private def functionFullName(function: OxFunctionDecl, ownerFullName: Option[String]): String = {
    val simpleName = functionSimpleName(function)
    if (isSyntheticRequiresFunction(function) && ownerFullName.isEmpty) {
      s"${function.name}:${function.signature}"
    } else
      functionOwnerFullName(function, ownerFullName)
        .map(owner => s"$owner.$simpleName:${function.signature}")
        .getOrElse(topLevelFunctionFullName(function))
  }

  private def topLevelFunctionFullName(function: OxFunctionDecl): String = {
    if (FileDefaults.hasCppFileExtension(declarationFilename(function))) s"${function.name}:${function.signature}"
    else function.name
  }

  private def isSyntheticRequiresFunction(function: OxFunctionDecl): Boolean = {
    function.name == "requires" && function.returnType == "requires" && !function.isDefinition
  }

  private def functionSimpleName(function: OxFunctionDecl): String = {
    qualifiedNameParts(function.name).lastOption.getOrElse(function.name)
  }

  private def functionOwnerFullName(function: OxFunctionDecl, ownerFullName: Option[String]): Option[String] = {
    val parts = qualifiedNameParts(function.name)
    if (parts.size > 1) {
      val localOwner = parts.dropRight(1).mkString(".")
      ownerFullName.map(owner => s"$owner.$localOwner").orElse(Option(localOwner))
    } else {
      ownerFullName
    }
  }

  private def isOutOfClassAggregateFunction(function: OxFunctionDecl, ownerFullName: Option[String]): Boolean = {
    function.name.contains("::") &&
    functionOwnerFullName(function, ownerFullName).exists(aggregateTypeFullNames.contains)
  }

  private def isShadowedByOutOfClassDefinition(typeFullName: String, function: OxFunctionDecl): Boolean = {
    !function.isDefinition &&
    outOfClassFunctionsByOwner
      .getOrElse(typeFullName, Seq.empty)
      .exists(entry =>
        entry.function.isDefinition &&
          entry.simpleName == functionSimpleName(function) &&
          entry.function.signature == function.signature
      )
  }

  private def isStaticFunction(function: OxFunctionDecl, parentTypeOwner: Option[String]): Boolean = {
    function.isStatic || parentTypeOwner.exists { ownerTypeFullName =>
      functionEntries.exists(entry =>
        entry.ownerFullName.contains(ownerTypeFullName) &&
          entry.simpleName == functionSimpleName(function) &&
          entry.function.signature == function.signature &&
          entry.function.isStatic
      )
    }
  }

  private def isVirtualFunctionEntry(entry: FunctionEntry): Boolean = {
    isVirtualFunction(entry.function, entry.ownerFullName.filter(aggregateTypeFullNames.contains))
  }

  private def isVirtualFunction(function: OxFunctionDecl, parentTypeOwner: Option[String]): Boolean = {
    function.isVirtual || parentTypeOwner.exists { ownerTypeFullName =>
      functionEntries.exists(entry =>
        entry.ownerFullName.contains(ownerTypeFullName) &&
          entry.simpleName == functionSimpleName(function) &&
          entry.function.signature == function.signature &&
          entry.function.isVirtual
      )
    }
  }

  private def methodModifiers(
    simpleName: String,
    parentTypeOwner: Option[String],
    isStaticMethod: Boolean,
    isVirtualMethod: Boolean
  ): Seq[NewModifier] = {
    val isConstructor = parentTypeOwner
      .flatMap(_.split('.').lastOption)
      .contains(simpleName)
    Option.when(isConstructor)(NewModifier().modifierType(ModifierTypes.CONSTRUCTOR)).toSeq ++
      Option.when(isStaticMethod)(NewModifier().modifierType(ModifierTypes.STATIC)).toSeq ++
      Option.when(isVirtualMethod)(NewModifier().modifierType(ModifierTypes.VIRTUAL)).toSeq
  }

  private def declarationFilename(declaration: OxDeclaration): String = {
    declaration.sourcePath.map(SourceFiles.toRelativePath(_, config.inputPath)).getOrElse(filename)
  }

  private def namespacePath(name: String): Seq[String] = {
    qualifiedNameParts(name) match {
      case Seq() => Seq(name)
      case path  => path
    }
  }

  private def qualifiedNameParts(name: String): Seq[String] = {
    name
      .split("::")
      .map(_.trim)
      .filter(_.nonEmpty)
      .toSeq
  }

  private def normalizedQualifiedName(name: String): String = {
    qualifiedNameParts(name).mkString(".") match {
      case ""         => name
      case normalized => normalized
    }
  }

  private def macroSignature(macroDecl: OxMacroDecl): String = {
    s"${macroReturnTypeFullName(macroDecl)}(${macroDecl.parameters.size})"
  }

  private def macroReturnTypeFullName(macroDecl: OxMacroDecl): String = {
    if (macroDecl.parameters.isEmpty && isIntegerLiteral(macroDecl.body)) "int" else Defines.Any
  }

  private def literalType(value: String): String = {
    value.trim match {
      case "true" | "false" | "TRUE" | "FALSE"   => registerType("bool")
      case "nullptr"                             => registerType("std.nullptr_t")
      case literal if isIntegerLiteral(literal)  => registerType("int")
      case literal if isFloatingLiteral(literal) => registerType(floatingLiteralTypeFullName(literal))
      case literal if isCharLiteral(literal)     => registerType("char")
      case literal =>
        stringLiteralElementCount(literal)
          .map(count => registerType(s"char[$count]"))
          .getOrElse(registerType(Defines.Any))
    }
  }

  private def isCharLiteral(value: String): Boolean = {
    val literal  = value.trim
    val prefixes = Seq("u8", "u", "U", "L", "")
    prefixes.exists { prefix =>
      literal.startsWith(s"$prefix'") && literal.endsWith("'") && literal.length > prefix.length + 2
    }
  }

  private def stringLiteralElementCount(value: String): Option[Int] = {
    val literal      = value.trim
    val tokenLengths = mutable.ArrayBuffer.empty[Int]
    var index        = 0
    while (index < literal.length) {
      while (index < literal.length && literal.charAt(index).isWhitespace) index += 1
      if (index < literal.length) {
        parseStringLiteralToken(literal, index) match {
          case Some((length, nextIndex)) =>
            tokenLengths.addOne(length)
            index = nextIndex
          case None =>
            return None
        }
      }
    }
    Option.when(tokenLengths.nonEmpty)(tokenLengths.sum + 1)
  }

  private def parseStringLiteralToken(literal: String, start: Int): Option[(Int, Int)] = {
    val prefixes = Seq("u8R", "uR", "UR", "LR", "R", "u8", "u", "U", "L", "")
    prefixes.collectFirst(Function.unlift { prefix =>
      val tokenStart = start + prefix.length
      Option
        .when(literal.startsWith(prefix, start) && tokenStart < literal.length) {
          if (prefix.endsWith("R")) parseRawStringLiteralToken(literal, tokenStart)
          else parseRegularStringLiteralToken(literal, tokenStart)
        }
        .flatten
    })
  }

  private def parseRawStringLiteralToken(literal: String, quoteIndex: Int): Option[(Int, Int)] = {
    if (literal.charAt(quoteIndex) != '"') return None
    val delimiterStart = quoteIndex + 1
    val openParen      = literal.indexOf('(', delimiterStart)
    if (openParen < 0) return None
    val delimiter  = literal.substring(delimiterStart, openParen)
    val close      = s")$delimiter\""
    val closeIndex = literal.indexOf(close, openParen + 1)
    Option.when(closeIndex >= 0) {
      val contentLength = literal.substring(openParen + 1, closeIndex).codePointCount(0, closeIndex - openParen - 1)
      (contentLength, closeIndex + close.length)
    }
  }

  private def parseRegularStringLiteralToken(literal: String, quoteIndex: Int): Option[(Int, Int)] = {
    if (literal.charAt(quoteIndex) != '"') return None
    var index  = quoteIndex + 1
    var length = 0
    while (index < literal.length) {
      literal.charAt(index) match {
        case '"' =>
          return Some((length, index + 1))
        case '\\' if index + 1 < literal.length =>
          index = escapedLiteralEnd(literal, index + 1)
          length += 1
        case _ =>
          val codePoint = literal.codePointAt(index)
          index += Character.charCount(codePoint)
          length += 1
      }
    }
    None
  }

  private def escapedLiteralEnd(literal: String, start: Int): Int = {
    literal.charAt(start) match {
      case 'x' =>
        val end = firstIndexWhere(literal, start + 1)(ch => !ch.isDigit && !"abcdefABCDEF".contains(ch))
        math.max(start + 1, end)
      case 'u' =>
        math.min(literal.length, start + 5)
      case 'U' =>
        math.min(literal.length, start + 9)
      case ch if ch >= '0' && ch <= '7' =>
        var index = start + 1
        var seen  = 1
        while (index < literal.length && seen < 3 && literal.charAt(index) >= '0' && literal.charAt(index) <= '7') {
          index += 1
          seen += 1
        }
        index
      case _ =>
        start + 1
    }
  }

  private def firstIndexWhere(value: String, start: Int)(predicate: Char => Boolean): Int = {
    var index = start
    while (index < value.length && !predicate(value.charAt(index))) index += 1
    index
  }

  private def isIntegerLiteral(value: String): Boolean = {
    IntegerLiteralPattern.pattern.matcher(value.trim).matches()
  }

  private def isFloatingLiteral(value: String): Boolean = {
    FloatingLiteralPattern.pattern.matcher(value.trim).matches()
  }

  private def floatingLiteralTypeFullName(value: String): String = {
    Character.toLowerCase(value.trim.last) match {
      case 'f' => "float"
      case 'l' => "long double"
      case _   => "double"
    }
  }

  private def operatorFor(operator: String): String = {
    operator match {
      case "+"      => Operators.addition
      case "-"      => Operators.subtraction
      case "*"      => Operators.multiplication
      case "/"      => Operators.division
      case "%"      => Operators.modulo
      case "<"      => Operators.lessThan
      case ">"      => Operators.greaterThan
      case "<="     => Operators.lessEqualsThan
      case ">="     => Operators.greaterEqualsThan
      case "=="     => Operators.equals
      case "!="     => Operators.notEquals
      case "not_eq" => Operators.notEquals
      case "<=>"    => Operators.compare
      case "&&"     => Operators.logicalAnd
      case "and"    => Operators.logicalAnd
      case "||"     => Operators.logicalOr
      case "or"     => Operators.logicalOr
      case "&"      => Operators.and
      case "bitand" => Operators.and
      case "|"      => Operators.or
      case "bitor"  => Operators.or
      case "^"      => Operators.xor
      case "xor"    => Operators.xor
      case "<<"     => Operators.shiftLeft
      case ">>"     => Operators.arithmeticShiftRight
      case "="      => Operators.assignment
      case "+="     => Operators.assignmentPlus
      case "-="     => Operators.assignmentMinus
      case "*="     => Operators.assignmentMultiplication
      case "/="     => Operators.assignmentDivision
      case "%="     => Operators.assignmentModulo
      case "<<="    => Operators.assignmentShiftLeft
      case ">>="    => Operators.assignmentArithmeticShiftRight
      case "&="     => Operators.assignmentAnd
      case "^="     => Operators.assignmentXor
      case "|="     => Operators.assignmentOr
      case _        => Defines.OperatorUnknown
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
      case "co_await"     => "<operator>.await"
      case "co_yield"     => "<operator>.yield"
      case _              => Defines.OperatorUnknown
    }
  }

  private def normalizeType(typeName: String): String = {
    typeName
      .stripPrefix("struct ")
      .stripPrefix("union ")
      .stripPrefix("enum ")
      .trim
      .replace("::", ".")
  }

  private def resolveAliasType(typeName: String, aliases: Map[String, String] = typeAliases): String = {
    val normalized = normalizeType(typeName)
    if (normalized.endsWith("*") && normalized.length > 1) {
      s"${resolveAliasType(normalized.dropRight(1), aliases)}*"
    } else if (normalized.endsWith("&&") && normalized.length > 2) {
      s"${resolveAliasType(normalized.dropRight(2), aliases)}&&"
    } else if (normalized.endsWith("&") && normalized.length > 1) {
      s"${resolveAliasType(normalized.dropRight(1), aliases)}&"
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
