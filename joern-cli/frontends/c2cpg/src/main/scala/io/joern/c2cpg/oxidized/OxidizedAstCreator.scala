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
import java.util.regex.{Matcher, Pattern}
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
  private val CxxTypeQualifiers                         = Set("const", "volatile", "mutable", "restrict")
  private val CxxArithmeticTypes = Set(
    "bool",
    "char",
    "signed char",
    "unsigned char",
    "short",
    "unsigned short",
    "int",
    "unsigned int",
    "long",
    "unsigned long",
    "long long",
    "unsigned long long",
    "float",
    "double",
    "long double"
  )
  private val CxxIntegralPromotionSources =
    Set("bool", "char", "signed char", "unsigned char", "short", "unsigned short")

  private final case class LambdaInfo(
    name: String,
    fullName: String,
    signature: String,
    returnType: String,
    semanticReturnType: String
  )
  private final case class ScopeEntry(
    typeFullName: String,
    declaration: NewNode,
    lambdaInfo: Option[LambdaInfo] = None,
    semanticTypeFullName: Option[String] = None
  ) {
    def expressionTypeFullName: String = semanticTypeFullName.getOrElse(typeFullName)
  }
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
  private final case class StaticLocalStorage(
    local: OxLocalDecl,
    typeName: String,
    receiverPrefix: String,
    guardName: String,
    guardReceiverCode: String
  )
  private final case class HeapConstructor(line: Int, info: ConstructorInvocationInfo, arguments: Seq[OxExpression])
  private final case class HeapDestructor(
    code: String,
    line: Int,
    entry: FunctionEntry,
    receiver: OxExpression,
    isArrayDelete: Boolean
  )
  private final case class ConstructorInitializerResolution(
    arguments: Seq[OxExpression],
    entry: Option[FunctionEntry],
    preserveInitializerListCode: Boolean = false
  )
  private final case class ResolvedBaseClass(typeFullName: String, isVirtual: Boolean)
  private sealed trait ConstructorSubobject
  private final case class ConstructorBaseSubobject(typeName: String)    extends ConstructorSubobject
  private final case class ConstructorFieldSubobject(field: OxFieldDecl) extends ConstructorSubobject
  private final case class ConstructorFieldArrayElementSubobject(field: OxFieldDecl, index: Int)
      extends ConstructorSubobject
  private final case class ConstructorPrefixStep(asts: Seq[Ast], constructed: Seq[ConstructorSubobject], line: Int)
  private final case class ConstructorInvocationInfo(
    typeName: String,
    constructorName: String,
    constructor: Option[FunctionEntry],
    signature: Option[String],
    methodFullName: String,
    code: String
  )
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
  private final case class ArgumentInfo(expression: OxExpression, typeFullName: Option[String], isRvalue: Boolean)
  private final case class OverloadScore(score: Int, argumentScores: Seq[Int], isViable: Boolean)
  private final case class ScoredOverload(candidate: FunctionEntry, score: OverloadScore, index: Int)
  private final case class FunctionReturnExpression(
    expression: OxExpression,
    localTypes: Map[String, String],
    returnCode: String
  )
  private final case class FunctionCaptureContext(
    function: OxFunctionDecl,
    methodRef: NewMethodRef,
    capturedGlobals: mutable.LinkedHashMap[String, CapturedGlobal] = mutable.LinkedHashMap.empty
  )
  private sealed trait AggregateInitializerSlot
  private final case class AggregateBaseInitializerSlot(typeName: String) extends AggregateInitializerSlot
  private final case class AggregateFieldInitializerSlot(ownerTypeName: String, field: OxFieldDecl)
      extends AggregateInitializerSlot
  private sealed trait AggregateMemberItem
  private final case class AggregateFieldMemberItem(field: OxFieldDecl) extends AggregateMemberItem
  private final case class AggregateAnonymousMemberItem(typeName: String, declaration: OxStructDecl)
      extends AggregateMemberItem
  private sealed trait AggregatePathSegment
  private final case class AggregateFieldPathSegment(name: String, isIndirect: Boolean = false)
      extends AggregatePathSegment
  private final case class AggregateIndexPathSegment(indexCode: String, indexExpression: Option[OxExpression] = None)
      extends AggregatePathSegment
  private final case class AggregateAssignmentRoot(name: String, line: Int, scopeEntry: Option[ScopeEntry])
  private final case class AggregateAssignmentTarget(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    targetTypeName: String,
    fieldPathPrefix: Seq[AggregatePathSegment]
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
  private lazy val aggregateDeclarationEntriesByType: Map[String, (OxStructDecl, String)] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      Seq(localName, fullName).distinct.map(typeName => typeName -> (structDecl -> fullName))
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
        typeName -> aggregateFieldsForLookup(structDecl, fullName).map(field => field.name -> field).toMap
      }
    }.toMap
  private lazy val aggregateFieldsByType: Map[String, Seq[OxFieldDecl]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      Seq(localName, fullName).distinct.map(typeName => typeName -> aggregateFieldsForLookup(structDecl, fullName))
    }.toMap
  private lazy val aggregateBaseClassesByType: Map[String, Seq[ResolvedBaseClass]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      val baseTypes = structDecl.baseClassDeclarations.map(baseClass =>
        ResolvedBaseClass(resolveBaseTypeFullName(baseClass.name, parentFullName), baseClass.isVirtual)
      )
      Seq(localName, fullName).distinct.map(typeName => typeName -> baseTypes)
    }.toMap
  private lazy val aggregateBaseTypesByType: Map[String, Seq[String]] =
    aggregateBaseClassesByType.view.mapValues(_.map(_.typeFullName)).toMap
  private lazy val aggregateUsingDeclarationsByType: Map[String, Seq[OxUsingDeclaration]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      Seq(localName, fullName).distinct.map(typeName => typeName -> structDecl.usingDeclarations)
    }.toMap
  private val TemplateParameterListPattern = raw"template\s*<([^>]*)>".r
  private val TemplateTypeParameterPattern = raw"(?:typename|class)\s*(?:\.\.\.)?\s+([A-Za-z_]\w*)".r
  private val IdentifierTokenPattern       = raw"[A-Za-z_]\w*".r
  private val IntegerLiteralPattern        = """[+-]?(?:0[xX][0-9a-fA-F]+|\d+)[uUlL]*""".r
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
  private val CxxOverloadableUnaryOperators              = Set("+", "-", "*", "&", "~", "!", "++", "--")
  private val CxxPostfixUnaryOperatorsWithDummyParameter = Set("++", "--")

  private var scope: Map[String, ScopeEntry]                                   = Map.empty
  private var globalLocalEntries: Map[OxGlobalVariableDecl, ScopeEntry]        = Map.empty
  private var globalScopeByName: Map[String, ScopeEntry]                       = Map.empty
  private var functionCaptureContext: Option[FunctionCaptureContext]           = None
  private var currentMethodOwnerTypeFullName: Option[String]                   = None
  private var currentMethodFullName: Option[String]                            = None
  private var currentMethodSimpleName: Option[String]                          = None
  private var currentMethodIsConst: Option[Boolean]                            = None
  private var currentMethodReturnTypeFullName: Option[String]                  = None
  private var typeAliases: Map[String, String]                                 = Map.empty
  private var localDestructorScopes: List[Vector[LocalDestructor]]             = Nil
  private var jumpCleanupTargets: List[JumpCleanupTarget]                      = Nil
  private var gotoLabelCleanupDestructors: Map[String, Seq[LocalDestructor]]   = Map.empty
  private var staticLocalStorages: Vector[StaticLocalStorage]                  = Vector.empty
  private val lambdaInfos: mutable.LinkedHashMap[String, LambdaInfo]           = mutable.LinkedHashMap.empty
  private val emittedLambdaFullNames: mutable.Set[String]                      = mutable.Set.empty
  private val lambdaReturnTypesByFullName: mutable.Map[String, String]         = mutable.Map.empty
  private val lambdaSemanticReturnTypesByFullName: mutable.Map[String, String] = mutable.Map.empty
  private val lambdaSignaturesByFullName: mutable.Map[String, String]          = mutable.Map.empty
  private val autoReturnInferenceStack: mutable.Set[String]                    = mutable.Set.empty

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
    val declarationAsts            = globalMethodDeclarationAsts(document.declarations)
    val staticLocalDestructionAsts = staticLocalDestructorAsts()
    val globalDestructorAsts       = globalDestructorAstsForDeclarations(document.declarations)
    val globalMethodAst =
      methodAst(
        globalMethod,
        Seq.empty,
        blockAst(globalBlock, (declarationAsts ++ staticLocalDestructionAsts ++ globalDestructorAsts).toList),
        methodReturnNode(origin, Defines.Any)
      )

    val includeAsts = document.declarations.collect { case includeDecl: OxIncludeDecl => astForInclude(includeDecl) }
    Ast(namespaceBlock).withChildren(includeAsts ++ namespaceAsts :+ Ast(globalTypeDecl).withChild(globalMethodAst))
  }

  private def globalMethodDeclarationAsts(
    declarations: Seq[OxDeclaration],
    ownerFullName: Option[String] = None,
    includeNonGlobals: Boolean = true
  ): Seq[Ast] = {
    declarations.flatMap {
      case namespace: OxNamespaceDecl =>
        globalMethodDeclarationAsts(
          namespace.declarations,
          Option(namespaceOwnerFullName(namespace, ownerFullName)),
          includeNonGlobals = false
        )
      case global: OxGlobalVariableDecl =>
        astsForGlobalVariable(global, ownerFullName)
      case declaration if includeNonGlobals =>
        astForDeclaration(declaration, ownerFullName, globalNamespaceBlock().fullName)
      case _ =>
        Seq.empty
    }
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
        astsForGlobalVariable(global, ownerFullName)
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
    val localPath     = namespacePath(namespaceDecl.name)
    val localName     = localPath.lastOption.getOrElse(namespaceDecl.name)
    val ownerFullName = namespaceOwnerFullName(namespaceDecl, parentOwnerFullName)
    val filename      = declarationFilename(namespaceDecl)
    val namespaceBlock =
      namespaceBlockNode(OxOrigin(namespaceDecl), localName, s"$filename:$ownerFullName", filename)
        .code(namespaceDecl.code)
    val childAsts = namespaceDecl.declarations.flatMap {
      case nestedNamespace: OxNamespaceDecl => Seq(astForNamespace(nestedNamespace, Option(ownerFullName)))
      case _: OxGlobalVariableDecl          => Seq.empty
      case declaration => astForDeclaration(declaration, Option(ownerFullName), namespaceBlock.fullName)
    }
    Ast(namespaceBlock).withChild(
      blockAst(blockNode(OxOrigin(namespaceDecl), namespaceDecl.code, Defines.Any), childAsts.toList)
    )
  }

  private def namespaceOwnerFullName(namespaceDecl: OxNamespaceDecl, parentOwnerFullName: Option[String]): String = {
    val localPath = namespacePath(namespaceDecl.name)
    parentOwnerFullName
      .map(parent => (parent +: localPath).mkString("."))
      .getOrElse(localPath.mkString("."))
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

  private def aggregateFieldsForLookup(structDecl: OxStructDecl, typeName: String): Seq[OxFieldDecl] = {
    aggregateMemberItems(structDecl, typeName).flatMap {
      case AggregateFieldMemberItem(field) => Seq(field)
      case AggregateAnonymousMemberItem(nestedTypeName, nestedDecl) =>
        aggregateFieldsForLookup(nestedDecl, nestedTypeName)
    }
  }

  private def aggregateMemberItems(structDecl: OxStructDecl, typeName: String): Seq[AggregateMemberItem] = {
    val fieldItems = structDecl.fields.map { field =>
      aggregateMemberCodeIndex(structDecl, field.code) -> AggregateFieldMemberItem(field)
    }
    val anonymousItems = structDecl.nestedDeclarations.collect {
      case nestedStruct: OxStructDecl if isAnonymousAggregateDecl(nestedStruct) =>
        val nestedTypeName = s"$typeName.${normalizeType(nestedStruct.name)}"
        aggregateMemberCodeIndex(structDecl, nestedStruct.code) -> AggregateAnonymousMemberItem(
          nestedTypeName,
          nestedStruct
        )
    }
    (fieldItems ++ anonymousItems).sortBy(_._1).map(_._2)
  }

  private def aggregateMemberCodeIndex(structDecl: OxStructDecl, memberCode: String): Int = {
    val index = structDecl.code.indexOf(memberCode)
    if (index >= 0) index else Int.MaxValue
  }

  private def isAnonymousAggregateDecl(structDecl: OxStructDecl): Boolean = {
    normalizeType(structDecl.name).startsWith("<type>")
  }

  private def isUnionAggregateDecl(structDecl: OxStructDecl): Boolean = {
    structDecl.code.trim.startsWith("union")
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
      case global: OxGlobalVariableDecl =>
        requiredImplicitDefaultConstructorType(global, ownerFullName).toSet ++
          global.initializer.toSet.flatMap(
            collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)
          )
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
      case OxAssignment(_, _, _, left, right) =>
        Seq(left, right).flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)).toSet
      case OxUnary(_, _, _, _, argument) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(argument, ownerFullName)
      case OxConditional(_, _, condition, consequence, alternative) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(condition, ownerFullName) ++
          consequence.toSet.flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName)) ++
          collectRequiredImplicitDefaultConstructorTypesFromExpression(alternative, ownerFullName)
      case OxCast(_, _, _, _, value) =>
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
      case newExpression @ OxNew(_, _, _, arguments, initializerArguments) =>
        implicitDefaultConstructorTypeForNew(newExpression, ownerFullName).toSet ++
          (arguments ++ initializerArguments)
            .flatMap(collectRequiredImplicitDefaultConstructorTypesFromExpression(_, ownerFullName))
            .toSet
      case OxDelete(_, _, argument) =>
        collectRequiredImplicitDefaultConstructorTypesFromExpression(argument, ownerFullName)
      case OxLambda(_, _, captures, _, _, _, _, _, body) =>
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
      .when(isDefaultConstruction)(localDefaultConstructedAggregateTypeFullName(local, ownerFullName))
      .flatten
      .filter(hasImplicitDefaultConstructor)
  }

  private def requiredImplicitDefaultConstructorType(
    global: OxGlobalVariableDecl,
    ownerFullName: Option[String]
  ): Option[String] = {
    Option
      .when(global.initializer.isEmpty)(globalDefaultConstructedAggregateTypeFullName(global, ownerFullName))
      .flatten
      .filter(hasImplicitDefaultConstructor)
  }

  private def globalDefaultConstructedAggregateTypeFullName(
    global: OxGlobalVariableDecl,
    ownerFullName: Option[String]
  ): Option[String] = {
    localObjectAggregateTypeFullName(global.typeName, ownerFullName)
      .orElse(globalArrayElementAggregateTypeFullName(global, ownerFullName))
  }

  private def globalArrayElementAggregateTypeFullName(
    global: OxGlobalVariableDecl,
    ownerFullName: Option[String]
  ): Option[String] = {
    Option
      .when(globalArrayElementCount(global).exists(_ > 0))(
        arrayElementTypeFullName(global.typeName).flatMap(localObjectAggregateTypeFullName(_, ownerFullName))
      )
      .flatten
  }

  private def localDefaultConstructedAggregateTypeFullName(
    local: OxLocalDecl,
    ownerFullName: Option[String]
  ): Option[String] = {
    localObjectAggregateTypeFullName(local.typeName, ownerFullName)
      .orElse(localArrayElementAggregateTypeFullName(local, ownerFullName))
  }

  private def localArrayElementAggregateTypeFullName(
    local: OxLocalDecl,
    ownerFullName: Option[String]
  ): Option[String] = {
    Option
      .when(localArrayElementCount(local).exists(_ > 0))(
        arrayElementTypeFullName(local.typeName).flatMap(localObjectAggregateTypeFullName(_, ownerFullName))
      )
      .flatten
  }

  private def localObjectAggregateTypeFullName(
    typeName: String,
    ownerFullName: Option[String],
    aliases: Map[String, String] = typeAliases
  ): Option[String] = {
    val normalized = stripCxxTypeQualifiers(normalizeType(resolveAliasType(typeName, aliases))).trim
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

  private def implicitDefaultConstructorTypeForNew(
    newExpression: OxNew,
    ownerFullName: Option[String]
  ): Option[String] = {
    Option
      .when(newExpression.initializerArguments.isEmpty)(
        localObjectAggregateTypeFullName(newExpression.typeName, ownerFullName)
      )
      .flatten
      .filter(hasImplicitDefaultConstructor)
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
        inherits = structDecl.baseClassDeclarations.map(baseClass =>
          registerType(resolveBaseTypeFullName(baseClass.name, parentTypeFullName))
        ),
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
    val previousScope            = scope
    val previousMethodOwner      = currentMethodOwnerTypeFullName
    val previousMethodFullName   = currentMethodFullName
    val previousMethodIsConst    = currentMethodIsConst
    val previousMethodReturnType = currentMethodReturnTypeFullName
    scope = Map(Defines.This -> ScopeEntry(thisType, thisParameter))
    currentMethodOwnerTypeFullName = Option(typeName)
    currentMethodFullName = Option(fullName)
    currentMethodIsConst = Option(false)
    currentMethodReturnTypeFullName = Option(Defines.Void)
    val defaultInitializerAsts =
      try {
        constructorPrefixInitializerAsts(typeName, Seq.empty, structDecl.line)
      } finally {
        currentMethodReturnTypeFullName = previousMethodReturnType
        currentMethodIsConst = previousMethodIsConst
        currentMethodFullName = previousMethodFullName
        currentMethodOwnerTypeFullName = previousMethodOwner
        scope = previousScope
      }
    val body         = blockAst(blockNode(origin, constructorName, Defines.Any), defaultInitializerAsts.toList)
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

  private def virtualBaseTypesForMostDerived(typeFullName: String): Seq[String] = {
    def loop(current: String, seen: Set[String]): Seq[String] = {
      val normalized = receiverAggregateTypeName(resolveAliasType(current))
      if (seen.contains(normalized)) {
        Seq.empty
      } else {
        aggregateBaseClassesByType.getOrElse(normalized, Seq.empty).flatMap { baseClass =>
          val baseType    = receiverAggregateTypeName(resolveAliasType(baseClass.typeFullName))
          val nestedBases = loop(baseClass.typeFullName, seen + normalized)
          val currentBase = Option.when(baseClass.isVirtual && !seen.contains(baseType))(baseClass.typeFullName)
          nestedBases ++ currentBase
        }
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
    val aliasType = registerType(ownerResolvedTypeFullNamePreservingCv(typedef.typeName, ownerFullName))
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
    typeAliases = collectTypeAliases(document.declarations, None, Map.empty)
  }

  private def collectTypeAliases(
    declarations: Seq[OxDeclaration],
    ownerFullName: Option[String],
    aliases: Map[String, String]
  ): Map[String, String] = {
    var currentAliases = aliases
    declarations.foreach {
      case namespace: OxNamespaceDecl =>
        currentAliases = collectTypeAliases(
          namespace.declarations,
          Option(namespaceOwnerFullName(namespace, ownerFullName)),
          currentAliases
        )
      case typedef: OxTypedefDecl =>
        val localName = normalizeType(typedef.name)
        val target    = ownerResolvedTypeFullNamePreservingCv(typedef.typeName, ownerFullName, currentAliases)
        currentAliases = currentAliases.updated(localName, target)
        ownerFullName.foreach { owner =>
          currentAliases = currentAliases.updated(s"$owner.$localName", target)
        }
      case _ =>
    }
    currentAliases
  }

  private def initializeGlobalScope(): Unit = {
    val globalEntries = globalVariableDeclarations(document.declarations).map { case (global, ownerFullName) =>
      val localCode        = localCodeForGlobal(global)
      val typeName         = registerType(globalTypeFullName(global, ownerFullName))
      val semanticTypeName = registerType(globalSemanticTypeFullName(global, ownerFullName))
      val node             = localNode(OxOrigin(global).copy(code = localCode), global.name, localCode, typeName)
      global -> (ownerFullName, ScopeEntry(typeName, node, semanticTypeFullName = Option(semanticTypeName)))
    }
    globalLocalEntries = globalEntries.map { case (global, (_, scopeEntry)) =>
      global -> scopeEntry
    }.toMap
    globalScopeByName = globalEntries.flatMap { case (global, (ownerFullName, scopeEntry)) =>
      val qualifiedNames = ownerFullName.toSeq.flatMap { owner =>
        Seq(s"$owner.${global.name}", s"${owner.split('.').mkString("::")}::${global.name}")
      }
      (global.name +: qualifiedNames).map(_ -> scopeEntry)
    }.toMap
  }

  private def globalVariableDeclarations(
    declarations: Seq[OxDeclaration],
    ownerFullName: Option[String] = None
  ): Seq[(OxGlobalVariableDecl, Option[String])] = {
    declarations.flatMap {
      case namespace: OxNamespaceDecl =>
        globalVariableDeclarations(namespace.declarations, Option(namespaceOwnerFullName(namespace, ownerFullName)))
      case global: OxGlobalVariableDecl =>
        Seq(global -> ownerFullName)
      case _ =>
        Seq.empty
    }
  }

  private def astsForGlobalVariable(global: OxGlobalVariableDecl, ownerFullName: Option[String] = None): Seq[Ast] = {
    val origin    = OxOrigin(global)
    val localCode = localCodeForGlobal(global)
    val scopeEntry = globalLocalEntries.getOrElse(
      global, {
        val typeName         = registerType(globalTypeFullName(global, ownerFullName))
        val semanticTypeName = registerType(globalSemanticTypeFullName(global, ownerFullName))
        ScopeEntry(
          typeName,
          this.localNode(origin.copy(code = localCode), global.name, localCode, typeName),
          semanticTypeFullName = Option(semanticTypeName)
        )
      }
    )
    val localAst        = Ast(scopeEntry.declaration)
    val constructorAsts = globalConstructorAsts(global, scopeEntry)
    if (constructorAsts.nonEmpty) {
      Seq(localAst) ++ constructorAsts
    } else {
      global.initializer match {
        case Some(initializer) =>
          val leftCode       = globalAssignmentTargetCode(global)
          val assignmentCode = s"$leftCode = ${initializer.code}"
          val left           = identifierAstForScopeEntry(global.name, leftCode, global.line, scopeEntry)
          val assignment =
            assignmentAst(origin.copy(code = assignmentCode), left, expressionAst(initializer), assignmentCode)
          val initializerAggregateAssignments = aggregateAssignmentExpressionAsts(initializer)
          val fieldAssignments =
            aggregateInitializerAssignmentAsts(
              AggregateAssignmentRoot(global.name, global.line, Option(scopeEntry)),
              initializer,
              scopeEntry.typeFullName
            )
          Seq(localAst) ++ initializerAggregateAssignments ++ Seq(assignment) ++ fieldAssignments
        case None =>
          Seq(localAst)
      }
    }
  }

  private def globalConstructorAsts(global: OxGlobalVariableDecl, scopeEntry: ScopeEntry): Seq[Ast] = {
    val typeName = scopeEntry.typeFullName
    global.initializer match {
      case Some(initializer: OxInitializerList) if isConstructorInitializer(typeName, initializer) =>
        val resolution = constructorInitializerResolution(typeName, initializer)
        resolution.arguments.flatMap(aggregateAssignmentExpressionAsts) ++
          Seq(globalConstructorAssignmentAst(global, scopeEntry, initializer, typeName, resolution)) ++
          temporaryDestructorAstsForConstructorArguments(resolution.arguments, resolution.entry)
      case Some(initializer) if isCopyConstructorInitializer(typeName, initializer) =>
        val arguments   = Seq(initializer)
        val constructor = constructorEntry(typeName, arguments)
        aggregateAssignmentExpressionAsts(initializer) ++ Seq(
          globalConstructorAssignmentAst(
            global,
            scopeEntry,
            typeName,
            arguments,
            initializer.code,
            OxOrigin(initializer),
            constructor
          )
        ) ++ temporaryDestructorAstsForConstructorArguments(arguments, constructor)
      case Some(initializer: OxInitializerList) =>
        globalArrayInitializerConstructorAsts(global, scopeEntry, typeName, initializer)
      case None =>
        val arrayConstructorAsts = globalArrayDefaultConstructorAsts(global, scopeEntry, typeName)
        if (arrayConstructorAsts.nonEmpty) {
          arrayConstructorAsts
        } else if (isDefaultConstructorInitializer(typeName)) {
          Seq(globalConstructorAssignmentAst(global, scopeEntry, typeName, Seq.empty, "", OxOrigin(global), None))
        } else {
          Seq.empty
        }
      case _ =>
        Seq.empty
    }
  }

  private def globalConstructorAssignmentAst(
    global: OxGlobalVariableDecl,
    scopeEntry: ScopeEntry,
    initializer: OxInitializerList,
    typeName: String,
    resolution: ConstructorInitializerResolution
  ): Ast = {
    globalConstructorAssignmentAst(
      global,
      scopeEntry,
      typeName,
      resolution.arguments,
      initializer.code.trim,
      OxOrigin(initializer),
      resolution.entry,
      preserveInitializerListCode = resolution.preserveInitializerListCode
    )
  }

  private def globalConstructorAssignmentAst(
    global: OxGlobalVariableDecl,
    scopeEntry: ScopeEntry,
    typeName: String,
    arguments: Seq[OxExpression],
    initializerCode: String,
    initializerOrigin: OxOrigin,
    resolvedConstructor: Option[FunctionEntry],
    preserveInitializerListCode: Boolean = false
  ): Ast = {
    val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
    val constructor     = resolvedConstructor.orElse(constructorEntry(typeName, arguments))
    val implicitSignature =
      Option.when(arguments.isEmpty && hasImplicitDefaultConstructor(typeName))("void()")
    val signature = constructor.map(_.function.signature).orElse(implicitSignature)
    val methodFullName = constructor
      .map(_.fullName)
      .orElse(signature.map(sig => s"$typeName.$constructorName:$sig"))
      .getOrElse(s"$typeName.$constructorName")
    val initCode        = normalizedConstructorInitCode(initializerCode, preserveInitializerListCode)
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
    val assignmentCode = s"${global.name} = $constructorCode"
    val left           = identifierAstForScopeEntry(global.name, global.name, global.line, scopeEntry)
    val argumentAsts = constructor
      .map(entry => argumentAstsForFunctionEntry(entry, arguments))
      .getOrElse(arguments.map(expressionAst))
    val right = constructorInvocationBlockAst(initializerOrigin, typeName, callNode_, argumentAsts)
    assignmentAst(OxOrigin(global).copy(code = assignmentCode), left, right, assignmentCode)
  }

  private def globalArrayDefaultConstructorAsts(
    global: OxGlobalVariableDecl,
    scopeEntry: ScopeEntry,
    typeName: String
  ): Seq[Ast] = {
    for {
      count       <- globalArrayElementCount(global).toSeq
      elementType <- arrayElementTypeFullName(typeName).toSeq
      info        <- constructorInvocationInfo(elementType, Seq.empty, "").toSeq
      index       <- 0 until count
    } yield globalArrayElementConstructorAssignmentAst(global, scopeEntry, typeName, index, global.line, info)
  }

  private def globalArrayInitializerConstructorAsts(
    global: OxGlobalVariableDecl,
    scopeEntry: ScopeEntry,
    typeName: String,
    initializer: OxInitializerList
  ): Seq[Ast] = {
    if (initializer.elements.exists(_.isInstanceOf[OxDesignatedInitializer])) {
      Seq.empty
    } else {
      val count       = globalArrayElementCount(global)
      val elementType = arrayElementTypeFullName(typeName)
      (count, elementType) match {
        case (Some(elementCount), Some(elementTypeName)) =>
          val explicitInitializers = initializer.elements.take(elementCount)
          val explicitConstructorAsts = explicitInitializers.zipWithIndex.map { case (elementInitializer, index) =>
            globalArrayElementInitializerConstructorAsts(
              global,
              scopeEntry,
              typeName,
              elementTypeName,
              index,
              elementInitializer
            )
          }
          if (explicitConstructorAsts.exists(_.isEmpty)) {
            Seq.empty
          } else {
            val defaultConstructorAsts = for {
              info  <- constructorInvocationInfo(elementTypeName, Seq.empty, "").toSeq
              index <- explicitInitializers.size until elementCount
            } yield globalArrayElementConstructorAssignmentAst(global, scopeEntry, typeName, index, global.line, info)
            explicitConstructorAsts.flatten ++ defaultConstructorAsts
          }
        case _ =>
          Seq.empty
      }
    }
  }

  private def globalArrayElementInitializerConstructorAsts(
    global: OxGlobalVariableDecl,
    scopeEntry: ScopeEntry,
    arrayTypeName: String,
    elementTypeName: String,
    index: Int,
    initializer: OxExpression
  ): Seq[Ast] = {
    arrayElementConstructorInvocationInfo(elementTypeName, initializer).toSeq.flatMap {
      case (info, arguments, constructorEntry) =>
        arguments.flatMap(aggregateAssignmentExpressionAsts) ++
          Seq(
            globalArrayElementConstructorAssignmentAst(global, scopeEntry, arrayTypeName, index, global.line, info)
          ) ++
          temporaryDestructorAstsForConstructorArguments(arguments, constructorEntry)
    }
  }

  private def globalArrayElementConstructorAssignmentAst(
    global: OxGlobalVariableDecl,
    scopeEntry: ScopeEntry,
    arrayTypeName: String,
    index: Int,
    line: Int,
    info: ConstructorInvocationInfo
  ): Ast = {
    val elementCode    = s"${global.name}[$index]"
    val assignmentCode = s"$elementCode = ${info.code}"
    val left           = arrayElementAccessAst(global.name, arrayTypeName, index, line, Option(scopeEntry))
    val callNode_      = constructorCallNode(OxOrigin(info.code, Option(line)), info)
    val right = constructorInvocationBlockAst(OxOrigin(info.code, Option(line)), info.typeName, callNode_, Seq.empty)
    assignmentAst(OxOrigin(assignmentCode, Option(line)), left, right, assignmentCode)
  }

  private def globalDestructorAstsForDeclarations(declarations: Seq[OxDeclaration]): Seq[Ast] = {
    globalVariableDeclarations(declarations).reverse.flatMap { case (global, _) => globalDestructorAsts(global) }
  }

  private def globalDestructorAsts(global: OxGlobalVariableDecl): Seq[Ast] = {
    globalLocalEntries.get(global).toSeq.flatMap { scopeEntry =>
      val typeName = scopeEntry.typeFullName
      val arrayDestructors = for {
        count       <- globalArrayElementCount(global).toSeq
        elementType <- arrayElementTypeFullName(typeName).toSeq
        entry       <- destructorEntryForType(elementType).toSeq
        index       <- (0 until count).reverse
      } yield globalArrayElementDestructorAst(global, scopeEntry, typeName, index, global.line, entry)
      if (arrayDestructors.nonEmpty) {
        arrayDestructors
      } else {
        destructorEntryForType(typeName).toSeq.map { entry =>
          globalDestructorAst(
            global.name,
            global.line,
            entry,
            identifierAstForScopeEntry(global.name, global.name, global.line, scopeEntry)
          )
        }
      }
    }
  }

  private def globalArrayElementDestructorAst(
    global: OxGlobalVariableDecl,
    scopeEntry: ScopeEntry,
    arrayTypeName: String,
    index: Int,
    line: Int,
    entry: FunctionEntry
  ): Ast = {
    val receiverCode = s"${global.name}[$index]"
    globalDestructorAst(
      receiverCode,
      line,
      entry,
      arrayElementAccessAst(global.name, arrayTypeName, index, line, Option(scopeEntry))
    )
  }

  private def globalDestructorAst(receiverCode: String, line: Int, entry: FunctionEntry, receiverAst: Ast): Ast = {
    val code = s"$receiverCode.${entry.simpleName}()"
    val callNode_ =
      callNode(
        OxOrigin(code, Option(line)),
        code,
        entry.simpleName,
        entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(callNode_, base = Option(receiverAst))
  }

  private def localCodeForGlobal(global: OxGlobalVariableDecl): String = {
    stripConstinitSpecifier(global.initializer.fold(global.code)(_ => global.code.takeWhile(_ != '=').trim))
  }

  private def globalTypeFullName(global: OxGlobalVariableDecl, ownerFullName: Option[String] = None): String = {
    ownerResolvedGlobalTypeFullName(
      typeFullNameWithStringLiteralLength(global.typeName, global.initializer),
      ownerFullName
    )
  }

  private def globalSemanticTypeFullName(global: OxGlobalVariableDecl, ownerFullName: Option[String] = None): String = {
    ownerResolvedTypeFullNamePreservingCv(
      typeFullNameWithStringLiteralLength(global.semanticTypeName, global.initializer),
      ownerFullName
    )
  }

  private def ownerResolvedTypeFullNamePreservingCv(
    typeName: String,
    ownerFullName: Option[String],
    aliases: Map[String, String] = typeAliases
  ): String = {
    val normalized           = normalizeType(resolveAliasType(typeName, aliases))
    val objectTypeName       = unaliasedAggregateObjectTypeName(normalized)
    val lookupObjectTypeName = normalizeType(resolveAliasType(objectTypeName, aliases))
    val resolvedObjectTypeName =
      localObjectAggregateTypeFullNamePreservingTemplate(lookupObjectTypeName, ownerFullName, aliases)
    val resolvedTypeName = resolvedObjectTypeName
      .map(resolvedObjectTypeName => replaceObjectTypeName(normalized, objectTypeName, resolvedObjectTypeName))
      .getOrElse(normalized)
    ownerResolvedTemplateArgumentTypeFullNames(resolvedTypeName, ownerFullName, aliases)
  }

  private def localObjectAggregateTypeFullNamePreservingTemplate(
    typeName: String,
    ownerFullName: Option[String],
    aliases: Map[String, String]
  ): Option[String] = {
    localObjectAggregateTypeFullName(typeName, ownerFullName, aliases).orElse {
      val erasedTypeName = stripTemplateArguments(typeName)
      Option
        .when(erasedTypeName != typeName) {
          localObjectAggregateTypeFullName(erasedTypeName, ownerFullName, aliases).map { resolvedErasedTypeName =>
            replaceObjectTypeName(typeName, erasedTypeName, resolvedErasedTypeName)
          }
        }
        .flatten
    }
  }

  private def ownerResolvedTemplateArgumentTypeFullNames(
    typeName: String,
    ownerFullName: Option[String],
    aliases: Map[String, String]
  ): String = {
    val startIndex = typeName.indexOf('<')
    if (startIndex < 0) {
      typeName
    } else {
      templateArgumentListEndIndex(typeName, startIndex)
        .map { endIndex =>
          val resolvedArguments = splitTemplateArgumentList(typeName.substring(startIndex + 1, endIndex))
            .map(argument => ownerResolvedTypeFullNamePreservingCv(argument, ownerFullName, aliases))
            .mkString(",")
          val prefix = typeName.take(startIndex + 1)
          val suffix = typeName.drop(endIndex)
          s"$prefix$resolvedArguments$suffix"
        }
        .getOrElse(typeName)
    }
  }

  private def unaliasedAggregateObjectTypeName(typeName: String): String = {
    val normalized     = normalizeType(typeName)
    val objectTypeName = stripCxxReference(normalized).stripSuffix("*").stripSuffix("[]")
    stripCxxTypeQualifiers(objectTypeName).trim
  }

  private def replaceObjectTypeName(
    typeName: String,
    objectTypeName: String,
    resolvedObjectTypeName: String
  ): String = {
    val candidates = Seq(objectTypeName, objectTypeName.split('.').lastOption.getOrElse(objectTypeName)).distinct
    candidates
      .flatMap(candidate => objectTypeNameStart(typeName, candidate).map(candidate -> _))
      .headOption
      .map { case (candidate, start) => typeName.patch(start, resolvedObjectTypeName, candidate.length) }
      .getOrElse(typeName)
  }

  private def objectTypeNameStart(typeName: String, candidate: String): Option[Int] = {
    val pattern =
      s"(^|[^A-Za-z0-9_.])(${java.util.regex.Pattern.quote(candidate)})(?=$$|[^A-Za-z0-9_.])".r
    pattern.findFirstMatchIn(typeName).map(_.start(2))
  }

  private def ownerResolvedGlobalTypeFullName(typeName: String, ownerFullName: Option[String]): String = {
    val normalized = normalizeType(typeName)
    arrayTypeSuffix(normalized) match {
      case Some(suffix) =>
        arrayElementTypeFullName(normalized)
          .flatMap(localObjectAggregateTypeFullName(_, ownerFullName))
          .map(elementType => s"$elementType$suffix")
          .getOrElse(normalized)
      case None =>
        localObjectAggregateTypeFullName(normalized, ownerFullName).getOrElse(normalized)
    }
  }

  private def arrayTypeSuffix(typeName: String): Option[String] = {
    val normalized = normalizeType(typeName)
    if (normalized.endsWith("[]") && normalized.length > 2) {
      Option("[]")
    } else {
      val bracketIndex = normalized.lastIndexOf('[')
      Option.when(bracketIndex > 0 && normalized.endsWith("]"))(normalized.drop(bracketIndex))
    }
  }

  private def globalAssignmentTargetCode(global: OxGlobalVariableDecl): String = {
    if (normalizeType(global.typeName).endsWith("[]")) s"${global.name}[]" else global.name
  }

  private def globalArrayElementCount(global: OxGlobalVariableDecl): Option[Int] = {
    val code      = localCodeForGlobal(global)
    val nameIndex = code.indexOf(global.name)
    Option
      .when(nameIndex >= 0)(code.drop(nameIndex + global.name.length).dropWhile(_.isWhitespace))
      .filter(_.startsWith("["))
      .flatMap { suffix =>
        val endIndex = suffix.indexOf(']')
        Option.when(endIndex > 1)(suffix.substring(1, endIndex).trim)
      }
      .flatMap(rawCount => Try(rawCount.toInt).toOption)
      .filter(_ > 0)
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
        val thisTypeName = if (function.isConst) s"const $ownerTypeFullName*" else s"$ownerTypeFullName*"
        val thisType     = registerType(thisTypeName)
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
        Defines.This -> (thisType, thisType, Ast(thisNode), thisNode)
      }
      .toSeq
    val explicitParameters = function.parameters.zipWithIndex.map { case (parameter, index) =>
      val parameterType         = registerType(normalizeType(parameter.typeName))
      val semanticParameterType = registerType(normalizeType(parameter.semanticTypeName))
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
      parameter.name -> (parameterType, semanticParameterType, Ast(parameterNode), parameterNode)
    }
    val parameters = implicitThisParameter ++ explicitParameters

    val previousScope            = scope
    val previousCaptureContext   = functionCaptureContext
    val previousMethodOwner      = currentMethodOwnerTypeFullName
    val previousMethodFullName   = currentMethodFullName
    val previousMethodSimpleName = currentMethodSimpleName
    val previousMethodIsConst    = currentMethodIsConst
    val previousMethodReturnType = currentMethodReturnTypeFullName
    val previousDestructorScopes = localDestructorScopes
    val previousJumpTargets      = jumpCleanupTargets
    val previousGotoLabels       = gotoLabelCleanupDestructors
    val captureContext =
      FunctionCaptureContext(function, methodRefNode(origin, simpleName, fullName, simpleName))
    scope = parameters.map { case (name, (typeName, semanticTypeName, _, node)) =>
      name -> ScopeEntry(typeName, node, semanticTypeFullName = Option(semanticTypeName))
    }.toMap
    functionCaptureContext = Option(captureContext)
    currentMethodOwnerTypeFullName = parentTypeOwner
    currentMethodFullName = Option(fullName)
    currentMethodSimpleName = Option(simpleName)
    currentMethodIsConst = Option(function.isConst)
    currentMethodReturnTypeFullName = Option(returnType)
    localDestructorScopes = Vector.empty[LocalDestructor] :: Nil
    jumpCleanupTargets = Nil
    gotoLabelCleanupDestructors = collectGotoLabelCleanupDestructors(function.body)
    val bodyAsts =
      try {
        val constructorPrefixAsts =
          if (isConstructorMethod(simpleName, parentTypeOwner)) {
            constructorPrefixInitializerAsts(parentTypeOwner.get, function.constructorInitializers, function.line)
          } else {
            function.constructorInitializers.flatMap(constructorInitializerAsts)
          }
        val statementAsts = function.body.flatMap(astsForStatement)
        val destructorAsts =
          Option
            .when(statementsMayCompleteNormally(function.body))(currentLocalDestructors.reverse.map(localDestructorAst))
            .getOrElse(Vector.empty)
        val subobjectDestructorAsts =
          Option
            .when(
              function.isDefinition &&
                isDestructorMethod(simpleName, parentTypeOwner) &&
                statementsMayCompleteNormally(function.body)
            )(automaticSubobjectDestructorAsts(parentTypeOwner.get, function.line))
            .getOrElse(Seq.empty)
        constructorPrefixAsts ++ statementAsts ++ destructorAsts ++ subobjectDestructorAsts
      } finally {
        localDestructorScopes = previousDestructorScopes
        jumpCleanupTargets = previousJumpTargets
        gotoLabelCleanupDestructors = previousGotoLabels
        currentMethodReturnTypeFullName = previousMethodReturnType
        currentMethodIsConst = previousMethodIsConst
        currentMethodSimpleName = previousMethodSimpleName
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
        parameters.map(_._2._3),
        body,
        methodReturn,
        methodModifiers(simpleName, parentTypeOwner, isStaticMethod, isVirtualMethod)
      )

    captureAstForFunction(captureContext).fold(Seq(ast))(captureAst => Seq(ast, captureAst))
  }

  private def constructorPrefixInitializerAsts(
    ownerTypeFullName: String,
    initializers: Seq[OxConstructorInitializer],
    line: Int
  ): Seq[Ast] = {
    val ownerType           = resolveAliasType(ownerTypeFullName)
    val fields              = aggregateFieldsByType.getOrElse(ownerType, Seq.empty)
    val fieldNames          = fields.map(_.name).toSet
    val initializersByField = initializers.groupBy(constructorInitializerFieldName)
    val virtualBaseTypes    = virtualBaseTypesForMostDerived(ownerType)
    val directBaseTypes = aggregateBaseClassesByType
      .getOrElse(ownerType, Seq.empty)
      .filterNot(_.isVirtual)
      .map(_.typeFullName)
    val virtualBaseInitializers = initializers.filter { initializer =>
      val name = constructorInitializerFieldName(initializer)
      !fieldNames.contains(name) && virtualBaseTypes.exists(baseType =>
        constructorInitializerMatchesBase(baseType, name)
      )
    }
    val directBaseInitializers = initializers.filter { initializer =>
      val name = constructorInitializerFieldName(initializer)
      !fieldNames.contains(name) && directBaseTypes.exists(baseType =>
        constructorInitializerMatchesBase(baseType, name)
      )
    }
    val virtualBaseInitializerSteps = virtualBaseTypes.flatMap { baseType =>
      val explicitInitializers =
        virtualBaseInitializers.filter(initializer =>
          constructorInitializerMatchesBase(baseType, constructorInitializerFieldName(initializer))
        )
      if (explicitInitializers.nonEmpty) {
        explicitInitializers.map(initializer => baseConstructorInitializerStep(baseType, initializer))
      } else {
        Seq(defaultBaseConstructorStep(baseType, line))
      }
    }
    val directBaseInitializerSteps = directBaseTypes.flatMap { baseType =>
      val explicitInitializers =
        directBaseInitializers.filter(initializer =>
          constructorInitializerMatchesBase(baseType, constructorInitializerFieldName(initializer))
        )
      if (explicitInitializers.nonEmpty) {
        explicitInitializers.map(initializer => baseConstructorInitializerStep(baseType, initializer))
      } else {
        Seq(defaultBaseConstructorStep(baseType, line))
      }
    }
    val orderedFieldInitializerSteps = fields.flatMap { field =>
      initializersByField
        .get(field.name)
        .map(
          _.flatMap(initializer =>
            memberArrayConstructorInitializerSteps(initializer, field)
              .getOrElse(
                constructorInitializerStep(initializer, Option(field.typeName), constructorFieldSubobjects(field))
              )
          )
        )
        .getOrElse {
          field.initializer
            .map(_ => defaultMemberInitializerSteps(ownerTypeFullName, field))
            .getOrElse(defaultMemberConstructorSteps(field, line))
        }
    }
    val consumedBaseInitializers = (virtualBaseInitializers ++ directBaseInitializers).toSet
    val extraInitializers = initializers.filterNot { initializer =>
      fieldNames.contains(constructorInitializerFieldName(initializer)) || consumedBaseInitializers.contains(
        initializer
      )
    }
    val extraInitializerSteps =
      extraInitializers.flatMap(initializer =>
        constructorInitializerStep(
          initializer,
          constructorInitializerTargetTypeName(ownerTypeFullName, initializer),
          constructed = Seq.empty
        )
      )
    guardedConstructorPrefixStepAsts(
      virtualBaseInitializerSteps ++ directBaseInitializerSteps ++ orderedFieldInitializerSteps ++ extraInitializerSteps
    )
  }

  private def guardedConstructorPrefixStepAsts(steps: Seq[ConstructorPrefixStep]): Seq[Ast] = {
    val (_, asts) = steps.foldLeft((Vector.empty[ConstructorSubobject], Vector.empty[Ast])) {
      case ((constructed, asts), step) =>
        val guardedAsts = constructorInitializerUnwindAsts(step.asts, constructed, step.line)
        (constructed ++ step.constructed, asts ++ guardedAsts)
    }
    asts
  }

  private def constructorInitializerUnwindAsts(
    asts: Seq[Ast],
    constructed: Seq[ConstructorSubobject],
    line: Int
  ): Seq[Ast] = {
    val cleanupAsts = constructed.reverse.flatMap(constructorSubobjectDestructorAsts(_, line))
    if (asts.nonEmpty && cleanupAsts.nonEmpty) {
      val tryNode = controlStructureNode(OxOrigin("try", Option(line)), ControlStructureTypes.TRY, "try")
      val rethrow = Ast(controlStructureNode(OxOrigin("throw;", Option(line)), ControlStructureTypes.THROW, "throw;"))
      val unwindBlock =
        blockAst(
          blockNode(OxOrigin("<constructor-unwind>", Option(line)), "<constructor-unwind>", Defines.Any),
          (cleanupAsts :+ rethrow).toList
        )
      val tryBlock =
        blockAst(blockNode(OxOrigin("try", Option(line)), "try", Defines.Any), (asts :+ unwindBlock).toList)
      Seq(tryCatchAst(tryNode, tryBlock, Seq.empty, None))
    } else {
      asts
    }
  }

  private def constructorSubobjectDestructorAsts(subobject: ConstructorSubobject, line: Int): Seq[Ast] = {
    subobject match {
      case ConstructorBaseSubobject(typeName) =>
        destructorEntryForType(typeName).toSeq.map(entry => baseDestructorAst(entry, line))
      case ConstructorFieldSubobject(field) =>
        destructorEntryForType(field.typeName).toSeq.map(entry => memberDestructorAst(field, entry, line))
      case ConstructorFieldArrayElementSubobject(field, index) =>
        arrayElementTypeFullName(field.typeName).flatMap(destructorEntryForType).toSeq.map { entry =>
          memberArrayElementDestructorAst(field, index, line, entry)
        }
    }
  }

  private def constructorPrefixStep(
    asts: Seq[Ast],
    constructed: Seq[ConstructorSubobject],
    line: Int
  ): ConstructorPrefixStep = {
    val constructedSubobjects =
      if (asts.nonEmpty) constructed.filter(constructorSubobjectHasDestructor) else Seq.empty
    ConstructorPrefixStep(asts, constructedSubobjects, line)
  }

  private def constructorSubobjectHasDestructor(subobject: ConstructorSubobject): Boolean = {
    subobject match {
      case ConstructorBaseSubobject(typeName) => destructorEntryForType(typeName).isDefined
      case ConstructorFieldSubobject(field)   => destructorEntryForType(field.typeName).isDefined
      case ConstructorFieldArrayElementSubobject(field, _) =>
        arrayElementTypeFullName(field.typeName).flatMap(destructorEntryForType).isDefined
    }
  }

  private def constructorFieldSubobjects(field: OxFieldDecl): Seq[ConstructorSubobject] = {
    if (field.isStatic) {
      Seq.empty
    } else {
      fieldArrayElementCount(field)
        .map(count => (0 until count).map(index => ConstructorFieldArrayElementSubobject(field, index)))
        .getOrElse(Seq(ConstructorFieldSubobject(field)))
    }
  }

  private def constructorInitializerStep(
    initializer: OxConstructorInitializer,
    initializedTypeName: Option[String],
    constructed: Seq[ConstructorSubobject]
  ): Seq[ConstructorPrefixStep] = {
    Seq(
      constructorPrefixStep(constructorInitializerAsts(initializer, initializedTypeName), constructed, initializer.line)
    )
  }

  private def baseConstructorInitializerStep(
    baseType: String,
    initializer: OxConstructorInitializer
  ): ConstructorPrefixStep = {
    constructorPrefixStep(
      baseConstructorInitializerAsts(baseType, initializer),
      Seq(ConstructorBaseSubobject(baseType)),
      initializer.line
    )
  }

  private def defaultBaseConstructorStep(baseType: String, line: Int): ConstructorPrefixStep = {
    constructorPrefixStep(defaultBaseConstructorAsts(baseType, line), Seq(ConstructorBaseSubobject(baseType)), line)
  }

  private def defaultMemberInitializerSteps(
    ownerTypeFullName: String,
    field: OxFieldDecl
  ): Seq[ConstructorPrefixStep] = {
    if (field.isStatic) {
      Seq.empty
    } else {
      field.initializer.toSeq.flatMap { initializer =>
        val arraySteps = initializer match {
          case initializerList: OxInitializerList =>
            memberArrayInitializerConstructorSteps(field, initializerList.elements, initializer.line)
          case _ =>
            Seq.empty
        }
        if (arraySteps.nonEmpty) {
          arraySteps
        } else {
          Seq(
            constructorPrefixStep(
              defaultMemberInitializerAsts(ownerTypeFullName, field),
              constructorFieldSubobjects(field),
              initializer.line
            )
          )
        }
      }
    }
  }

  private def defaultMemberConstructorSteps(field: OxFieldDecl, line: Int): Seq[ConstructorPrefixStep] = {
    if (field.isStatic) {
      Seq.empty
    } else {
      val arrayConstructorSteps = memberArrayDefaultConstructorSteps(field, line)
      if (arrayConstructorSteps.nonEmpty) {
        arrayConstructorSteps
      } else {
        Seq(constructorPrefixStep(defaultMemberConstructorAsts(field, line), constructorFieldSubobjects(field), line))
      }
    }
  }

  private def memberArrayConstructorInitializerSteps(
    initializer: OxConstructorInitializer,
    field: OxFieldDecl
  ): Option[Seq[ConstructorPrefixStep]] = {
    Option
      .when(!field.isStatic) {
        memberArrayInitializerConstructorSteps(field, initializer.arguments, initializer.line)
      }
      .filter(_.nonEmpty)
  }

  private def memberArrayDefaultConstructorSteps(field: OxFieldDecl, line: Int): Seq[ConstructorPrefixStep] = {
    for {
      count       <- fieldArrayElementCount(field).toSeq
      elementType <- arrayElementTypeFullName(field.typeName).toSeq
      info        <- constructorInvocationInfo(elementType, Seq.empty, "").toSeq
      index       <- 0 until count
    } yield constructorPrefixStep(
      Seq(memberArrayElementConstructorAssignmentAst(field, index, line, info)),
      Seq(ConstructorFieldArrayElementSubobject(field, index)),
      line
    )
  }

  private def memberArrayInitializerConstructorSteps(
    field: OxFieldDecl,
    initializers: Seq[OxExpression],
    line: Int
  ): Seq[ConstructorPrefixStep] = {
    val count       = fieldArrayElementCount(field)
    val elementType = arrayElementTypeFullName(field.typeName)
    (count, elementType) match {
      case (Some(elementCount), Some(elementTypeName)) =>
        val explicitInitializers = initializers.take(elementCount)
        val explicitConstructorSteps = explicitInitializers.zipWithIndex.map { case (elementInitializer, index) =>
          memberArrayElementInitializerConstructorStep(field, elementTypeName, index, line, elementInitializer)
        }
        if (explicitConstructorSteps.exists(_.isEmpty)) {
          Seq.empty
        } else {
          val defaultConstructorSteps = for {
            info  <- constructorInvocationInfo(elementTypeName, Seq.empty, "").toSeq
            index <- explicitInitializers.size until elementCount
          } yield constructorPrefixStep(
            Seq(memberArrayElementConstructorAssignmentAst(field, index, line, info)),
            Seq(ConstructorFieldArrayElementSubobject(field, index)),
            line
          )
          explicitConstructorSteps.flatten ++ defaultConstructorSteps
        }
      case _ =>
        Seq.empty
    }
  }

  private def memberArrayElementInitializerConstructorStep(
    field: OxFieldDecl,
    elementTypeName: String,
    index: Int,
    line: Int,
    initializer: OxExpression
  ): Seq[ConstructorPrefixStep] = {
    val asts = memberArrayElementInitializerConstructorAsts(field, elementTypeName, index, line, initializer)
    Option
      .when(asts.nonEmpty) {
        constructorPrefixStep(asts, Seq(ConstructorFieldArrayElementSubobject(field, index)), line)
      }
      .toSeq
  }

  private def constructorInitializerAsts(initializer: OxConstructorInitializer): Seq[Ast] = {
    constructorInitializerAsts(initializer, None)
  }

  private def constructorInitializerAsts(
    initializer: OxConstructorInitializer,
    initializedTypeName: Option[String]
  ): Seq[Ast] = {
    memberConstructorInitializerAsts(initializer, initializedTypeName).getOrElse {
      val constructorEntry =
        initializedTypeName.flatMap(typeName => constructorEntryForInitializedType(typeName, initializer.arguments))
      initializer.arguments.flatMap(aggregateAssignmentExpressionAsts) ++
        Seq(constructorInitializerAst(initializer)) ++
        temporaryDestructorAstsForConstructorArguments(initializer.arguments, constructorEntry)
    }
  }

  private def constructorInitializerAst(initializer: OxConstructorInitializer): Ast = {
    val fieldName      = constructorInitializerFieldName(initializer)
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

  private def memberConstructorInitializerAsts(
    initializer: OxConstructorInitializer,
    initializedTypeName: Option[String]
  ): Option[Seq[Ast]] = {
    initializedTypeName
      .flatMap(typeName =>
        constructorInvocationInfo(typeName, initializer.arguments, constructorInitializerInvocationCode(initializer))
      )
      .map { info =>
        val constructorEntry = info.constructor
        val callNode_        = constructorCallNode(OxOrigin(initializer.code, Option(initializer.line)), info)
        val argumentAsts = constructorEntry
          .map(entry => argumentAstsForFunctionEntry(entry, initializer.arguments))
          .getOrElse(initializer.arguments.map(expressionAst))
        val right = constructorInvocationBlockAst(
          OxOrigin(info.code, Option(initializer.line)),
          info.typeName,
          callNode_,
          argumentAsts
        )
        val fieldName      = constructorInitializerFieldName(initializer)
        val assignmentCode = s"${Defines.This}->$fieldName = ${info.code}"
        val left = implicitFieldAccessAst(fieldName, initializer.line).getOrElse(
          identifierAst(fieldName, fieldName, initializer.line)
        )
        initializer.arguments.flatMap(aggregateAssignmentExpressionAsts) ++
          Seq(assignmentAst(OxOrigin(assignmentCode, Option(initializer.line)), left, right, assignmentCode)) ++
          temporaryDestructorAstsForConstructorArguments(initializer.arguments, constructorEntry)
      }
  }

  private def memberArrayConstructorInitializerAsts(
    initializer: OxConstructorInitializer,
    field: OxFieldDecl
  ): Option[Seq[Ast]] = {
    Option
      .when(!field.isStatic) {
        memberArrayInitializerConstructorAsts(field, initializer.arguments, initializer.line)
      }
      .filter(_.nonEmpty)
  }

  private def baseConstructorInitializerAsts(baseType: String, initializer: OxConstructorInitializer): Seq[Ast] = {
    val info = constructorInvocationInfo(baseType, initializer.arguments, constructorInitializerValueCode(initializer))
    val constructorEntry = info.flatMap(_.constructor)
    initializer.arguments.flatMap(aggregateAssignmentExpressionAsts) ++
      info.map { invocationInfo =>
        val callNode_ = constructorCallNode(OxOrigin(initializer.code, Option(initializer.line)), invocationInfo)
        val argumentAsts = constructorEntry
          .map(entry => argumentAstsForFunctionEntry(entry, initializer.arguments))
          .getOrElse(initializer.arguments.map(expressionAst))
        createCallAst(
          callNode_,
          argumentAsts,
          base = Option(identifierAst(Defines.This, Defines.This, initializer.line))
        )
      }.toSeq ++
      temporaryDestructorAstsForConstructorArguments(initializer.arguments, constructorEntry)
  }

  private def defaultMemberInitializerAsts(ownerTypeFullName: String, field: OxFieldDecl): Seq[Ast] = {
    if (field.isStatic) {
      Seq.empty
    } else {
      field.initializer.toSeq.flatMap { initializer =>
        val memberArrayConstructorAsts = initializer match {
          case initializerList: OxInitializerList =>
            memberArrayInitializerConstructorAsts(field, initializerList.elements, initializer.line)
          case _ =>
            Seq.empty
        }
        if (memberArrayConstructorAsts.nonEmpty) {
          memberArrayConstructorAsts
        } else {
          val line           = initializer.line
          val fieldTypeName  = registerType(resolveAliasType(field.typeName))
          val assignmentCode = s"${Defines.This}->${field.name} = ${initializer.code}"
          val left = implicitFieldAccessAst(field.name, line).getOrElse(identifierAst(field.name, field.name, line))
          val right =
            expressionAstWithContextualConversion(initializer, Option(fieldTypeName))
          val assignment = assignmentAst(OxOrigin(assignmentCode, Option(line)), left, right, assignmentCode)
          val fieldAssignments = initializer match {
            case initializerList: OxInitializerList
                if isAggregateFieldType(fieldTypeName) || isArrayLikeType(fieldTypeName) =>
              aggregateInitializerAssignmentAsts(
                AggregateAssignmentRoot(Defines.This, line, scope.get(Defines.This)),
                rootTypeName = ownerTypeFullName,
                typeName = fieldTypeName,
                initializer = initializerList,
                fieldPathPrefix = Seq(AggregateFieldPathSegment(field.name, isIndirect = true))
              )
            case _ =>
              Seq.empty
          }
          val temporaryCleanup =
            temporaryDestructorAstsForLocalInitializer(
              Option(initializer),
              Option(fieldTypeName),
              extendCurrentTemporaryLifetime = false
            )
          aggregateAssignmentExpressionAsts(initializer) ++ Seq(assignment) ++ fieldAssignments ++ temporaryCleanup
        }
      }
    }
  }

  private def defaultBaseConstructorAsts(baseType: String, line: Int): Seq[Ast] = {
    constructorInvocationInfo(baseType, Seq.empty, "").map { info =>
      val callNode_ = constructorCallNode(OxOrigin(info.code, Option(line)), info)
      createCallAst(callNode_, base = Option(identifierAst(Defines.This, Defines.This, line)))
    }.toSeq
  }

  private def defaultMemberConstructorAsts(field: OxFieldDecl, line: Int): Seq[Ast] = {
    if (field.isStatic) {
      Seq.empty
    } else {
      val arrayConstructorAsts = memberArrayDefaultConstructorAsts(field, line)
      if (arrayConstructorAsts.nonEmpty) {
        arrayConstructorAsts
      } else {
        constructorInvocationInfo(field.typeName, Seq.empty, "").map { info =>
          val callNode_ = constructorCallNode(OxOrigin(info.code, Option(line)), info)
          val right =
            constructorInvocationBlockAst(OxOrigin(info.code, Option(line)), info.typeName, callNode_, Seq.empty)
          val assignmentCode = s"${Defines.This}->${field.name} = ${info.code}"
          val left = implicitFieldAccessAst(field.name, line).getOrElse(identifierAst(field.name, field.name, line))
          assignmentAst(OxOrigin(assignmentCode, Option(line)), left, right, assignmentCode)
        }.toSeq
      }
    }
  }

  private def memberArrayDefaultConstructorAsts(field: OxFieldDecl, line: Int): Seq[Ast] = {
    for {
      count       <- fieldArrayElementCount(field).toSeq
      elementType <- arrayElementTypeFullName(field.typeName).toSeq
      info        <- constructorInvocationInfo(elementType, Seq.empty, "").toSeq
      index       <- 0 until count
    } yield memberArrayElementConstructorAssignmentAst(field, index, line, info)
  }

  private def memberArrayInitializerConstructorAsts(
    field: OxFieldDecl,
    initializers: Seq[OxExpression],
    line: Int
  ): Seq[Ast] = {
    val count       = fieldArrayElementCount(field)
    val elementType = arrayElementTypeFullName(field.typeName)
    (count, elementType) match {
      case (Some(elementCount), Some(elementTypeName)) =>
        val explicitInitializers = initializers.take(elementCount)
        val explicitConstructorAsts = explicitInitializers.zipWithIndex.map { case (elementInitializer, index) =>
          memberArrayElementInitializerConstructorAsts(field, elementTypeName, index, line, elementInitializer)
        }
        if (explicitConstructorAsts.exists(_.isEmpty)) {
          Seq.empty
        } else {
          val defaultConstructorAsts = for {
            info  <- constructorInvocationInfo(elementTypeName, Seq.empty, "").toSeq
            index <- explicitInitializers.size until elementCount
          } yield memberArrayElementConstructorAssignmentAst(field, index, line, info)
          explicitConstructorAsts.flatten ++ defaultConstructorAsts
        }
      case _ =>
        Seq.empty
    }
  }

  private def memberArrayElementInitializerConstructorAsts(
    field: OxFieldDecl,
    elementTypeName: String,
    index: Int,
    line: Int,
    initializer: OxExpression
  ): Seq[Ast] = {
    arrayElementConstructorInvocationInfo(elementTypeName, initializer).toSeq.flatMap {
      case (info, arguments, constructorEntry) =>
        arguments.flatMap(aggregateAssignmentExpressionAsts) ++
          Seq(memberArrayElementConstructorAssignmentAst(field, index, line, info)) ++
          temporaryDestructorAstsForConstructorArguments(arguments, constructorEntry)
    }
  }

  private def memberArrayElementConstructorAssignmentAst(
    field: OxFieldDecl,
    index: Int,
    line: Int,
    info: ConstructorInvocationInfo
  ): Ast = {
    val elementCode    = s"${Defines.This}->${field.name}[$index]"
    val assignmentCode = s"$elementCode = ${info.code}"
    val left           = memberArrayElementAccessAst(field, index, line)
    val callNode_      = constructorCallNode(OxOrigin(info.code, Option(line)), info)
    val right = constructorInvocationBlockAst(OxOrigin(info.code, Option(line)), info.typeName, callNode_, Seq.empty)
    assignmentAst(OxOrigin(assignmentCode, Option(line)), left, right, assignmentCode)
  }

  private def constructorInitializerTargetTypeName(
    ownerTypeFullName: String,
    initializer: OxConstructorInitializer
  ): Option[String] = {
    val name = constructorInitializerFieldName(initializer)
    fieldEntryForTypeHierarchy(ownerTypeFullName, name)
      .map { case (_, field) => field.typeName }
      .orElse {
        val ownerType = resolveAliasType(ownerTypeFullName)
        (aggregateBaseTypesByType.getOrElse(ownerType, Seq.empty) ++ virtualBaseTypesForMostDerived(ownerType)).distinct
          .find(baseType => baseType.split('.').lastOption.contains(name) || baseType == name)
      }
  }

  private def constructorInitializerMatchesBase(baseType: String, initializerName: String): Boolean = {
    baseType == initializerName || baseType.split('.').lastOption.contains(initializerName)
  }

  private def constructorEntryForInitializedType(
    typeName: String,
    arguments: Seq[OxExpression]
  ): Option[FunctionEntry] = {
    val normalizedType = normalizeType(resolveAliasType(typeName))
    val aggregateType =
      resolveAggregateTypeFullName(receiverAggregateTypeName(normalizedType)).getOrElse(normalizedType)
    constructorEntry(aggregateType, arguments)
  }

  private def constructorInvocationInfo(
    typeName: String,
    arguments: Seq[OxExpression],
    initCode: String,
    resolvedConstructor: Option[FunctionEntry] = None
  ): Option[ConstructorInvocationInfo] = {
    val normalizedType = normalizeType(resolveAliasType(typeName))
    val aggregateType =
      resolveAggregateTypeFullName(receiverAggregateTypeName(normalizedType)).getOrElse(normalizedType)
    val constructorName = aggregateType.split('.').lastOption.getOrElse(aggregateType)
    val constructor     = resolvedConstructor.orElse(constructorEntry(aggregateType, arguments))
    val implicitSignature =
      Option.when(arguments.isEmpty && hasImplicitDefaultConstructor(aggregateType))("void()")
    val signature = constructor.map(_.function.signature).orElse(implicitSignature)
    signature.map { signature =>
      val methodFullName = constructor
        .map(_.fullName)
        .getOrElse(s"$aggregateType.$constructorName:$signature")
      ConstructorInvocationInfo(
        typeName = aggregateType,
        constructorName = constructorName,
        constructor = constructor,
        signature = Option(signature),
        methodFullName = methodFullName,
        code = s"$aggregateType.$constructorName($initCode)"
      )
    }
  }

  private def constructorCallNode(origin: OxOrigin, info: ConstructorInvocationInfo): NewCall = {
    callNode(
      origin.copy(code = info.code),
      info.code,
      info.constructorName,
      info.methodFullName,
      DispatchTypes.STATIC_DISPATCH,
      info.signature,
      Some(registerType(Defines.Void))
    )
  }

  private def constructorInitializerFieldName(initializer: OxConstructorInitializer): String = {
    qualifiedNameParts(initializer.field).lastOption.getOrElse(initializer.field)
  }

  private def constructorInitializerValueCode(initializer: OxConstructorInitializer): String = {
    initializer.arguments match {
      case Seq(argument) => argument.code
      case arguments     => arguments.map(_.code).mkString("{", ", ", "}")
    }
  }

  private def constructorInitializerInvocationCode(initializer: OxConstructorInitializer): String = {
    if (initializer.arguments.isEmpty) "" else constructorInitializerValueCode(initializer)
  }

  private def registerLocalDestructor(local: OxLocalDecl, typeName: String): Unit = {
    localDestructorsForDecl(local, typeName).foreach(registerLocalDestructor)
  }

  private def localDestructorsForDecl(local: OxLocalDecl, typeName: String): Seq[LocalDestructor] = {
    val arrayDestructors = localArrayElementReceiverCodes(local).flatMap { receiverCode =>
      arrayElementTypeFullName(typeName).flatMap(destructorEntryForType).map { destructor =>
        LocalDestructor(receiverCode, local.line, destructor)
      }
    }
    if (arrayDestructors.nonEmpty) {
      arrayDestructors
    } else {
      localDestructorForName(local.name, typeName, local.line).toSeq
    }
  }

  private def registerLocalDestructor(name: String, typeName: String, line: Int): Unit = {
    localDestructorForName(name, typeName, line).foreach(registerLocalDestructor)
  }

  private def localDestructorForName(name: String, typeName: String, line: Int): Option[LocalDestructor] = {
    destructorEntryForType(typeName).map(destructor => LocalDestructor(name, line, destructor))
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

  private def gotoLocalDestructors(label: String): Seq[LocalDestructor] = {
    gotoLabelCleanupDestructors
      .get(label)
      .map { targetDestructors =>
        val active = activeLocalDestructors
        val commonSuffixLength = active.reverse
          .zip(targetDestructors.reverse)
          .takeWhile { case (left, right) =>
            left == right
          }
          .length
        active.take(active.length - commonSuffixLength)
      }
      .getOrElse(Seq.empty)
  }

  private def localDestructorAst(destructor: LocalDestructor): Ast = {
    localDestructorAst(destructor, identifierAst(destructor.receiverCode, destructor.receiverCode, destructor.line))
  }

  private def localDestructorAst(destructor: LocalDestructor, base: Ast): Ast = {
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
    createCallAst(callNode_, base = Option(base))
  }

  private def currentAutomaticSubobjectDestructorAsts(line: Int): Seq[Ast] = {
    currentMethodOwnerTypeFullName
      .filter(ownerTypeFullName => isDestructorMethod(currentMethodSimpleName.getOrElse(""), Option(ownerTypeFullName)))
      .toSeq
      .flatMap(ownerTypeFullName => automaticSubobjectDestructorAsts(ownerTypeFullName, line))
  }

  private def currentConstructorUnwindSubobjectDestructorAsts(line: Int): Seq[Ast] = {
    currentMethodOwnerTypeFullName
      .filter(ownerTypeFullName =>
        isConstructorMethod(currentMethodSimpleName.getOrElse(""), Option(ownerTypeFullName))
      )
      .toSeq
      .flatMap(ownerTypeFullName => automaticSubobjectDestructorAsts(ownerTypeFullName, line))
  }

  private def automaticSubobjectDestructorAsts(ownerTypeFullName: String, line: Int): Seq[Ast] = {
    val ownerType = resolveAliasType(ownerTypeFullName)
    val fields = aggregateDeclarationsByType
      .get(ownerType)
      .map(_.fields)
      .getOrElse(aggregateFieldsByType.getOrElse(ownerType, Seq.empty))
    val memberDestructorAsts = fields.reverse
      .filterNot(_.isStatic)
      .flatMap(field =>
        memberArrayDestructorAsts(field, line) ++
          destructorEntryForType(field.typeName).map(entry => memberDestructorAst(field, entry, line))
      )
    val directBaseDestructorAsts = aggregateBaseClassesByType
      .getOrElse(ownerType, Seq.empty)
      .filterNot(_.isVirtual)
      .reverse
      .flatMap(baseClass => destructorEntryForType(baseClass.typeFullName).map(entry => baseDestructorAst(entry, line)))
    val virtualBaseDestructorAsts = virtualBaseTypesForMostDerived(ownerType).reverse
      .flatMap(baseType => destructorEntryForType(baseType).map(entry => baseDestructorAst(entry, line)))
    memberDestructorAsts ++ directBaseDestructorAsts ++ virtualBaseDestructorAsts
  }

  private def memberDestructorAst(field: OxFieldDecl, entry: FunctionEntry, line: Int): Ast = {
    val receiverCode = s"${Defines.This}->${field.name}"
    val code         = s"$receiverCode.${entry.simpleName}()"
    val callNode_ =
      callNode(
        OxOrigin(code, Option(line)),
        code,
        entry.simpleName,
        entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(
      callNode_,
      base = Option(implicitFieldAccessAst(field.name, line).getOrElse(identifierAst(field.name, field.name, line)))
    )
  }

  private def memberArrayDestructorAsts(field: OxFieldDecl, line: Int): Seq[Ast] = {
    for {
      count       <- fieldArrayElementCount(field).toSeq
      elementType <- arrayElementTypeFullName(field.typeName).toSeq
      entry       <- destructorEntryForType(elementType).toSeq
      index       <- (0 until count).reverse
    } yield memberArrayElementDestructorAst(field, index, line, entry)
  }

  private def memberArrayElementDestructorAst(field: OxFieldDecl, index: Int, line: Int, entry: FunctionEntry): Ast = {
    val receiverCode = s"${Defines.This}->${field.name}[$index]"
    val code         = s"$receiverCode.${entry.simpleName}()"
    val callNode_ =
      callNode(
        OxOrigin(code, Option(line)),
        code,
        entry.simpleName,
        entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(callNode_, base = Option(memberArrayElementAccessAst(field, index, line)))
  }

  private def baseDestructorAst(entry: FunctionEntry, line: Int): Ast = {
    val code = s"${Defines.This}->${entry.simpleName}()"
    val callNode_ =
      callNode(
        OxOrigin(code, Option(line)),
        code,
        entry.simpleName,
        entry.fullName,
        DispatchTypes.STATIC_DISPATCH,
        Option(entry.function.signature),
        Option(registerType(Defines.Void))
      )
    createCallAst(callNode_, base = Option(identifierAst(Defines.This, Defines.This, line)))
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
        expressionAstsWithRecoveredAggregateAssignments(assignment) ++
          (heapConstructorAstsForExpressions(Seq(assignment)) ++ temporaryDestructorAstsForExpressions(Seq(assignment)))
      case ret: OxReturn =>
        val returnType = currentMethodReturnTypeFullName
        heapConstructorAstsForExpressions(ret.expression.toSeq) ++ ret.expression.toSeq.flatMap(
          aggregateAssignmentExpressionAsts
        ) ++ temporaryDestructorAstsForReturnExpression(ret.expression, returnType) ++ activeLocalDestructors.map(
          localDestructorAst
        ) ++ currentAutomaticSubobjectDestructorAsts(ret.line) :+
          returnAst(
            returnNode(OxOrigin(ret), ret.code),
            ret.expression.toSeq.map(expression => expressionAstWithContextualConversion(expression, returnType))
          )
      case throwStmt: OxThrow =>
        val throwAst = Ast(controlStructureNode(OxOrigin(throwStmt), ControlStructureTypes.THROW, throwStmt.code))
          .withChildren(throwStmt.expression.toSeq.map(expressionAst))
        heapConstructorAstsForExpressions(throwStmt.expression.toSeq) ++ throwStmt.expression.toSeq.flatMap(
          aggregateAssignmentExpressionAsts
        ) ++ temporaryDestructorAstsForExpressions(throwStmt.expression.toSeq) ++ throwLocalDestructors.map(
          localDestructorAst
        ) ++ currentAutomaticSubobjectDestructorAsts(throwStmt.line) ++ currentConstructorUnwindSubobjectDestructorAsts(
          throwStmt.line
        ) :+
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
        val conditionAst              = conditionExpressionAstWithRecoveredAggregateAssignments(doWhileStmt.condition)
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
            Option(conditionAst),
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
          val conditionAsts = forStmt.condition.toSeq.map(conditionExpressionAstWithRecoveredAggregateAssignments(_))
          val conditionTemporaryCleanup = temporaryDestructorAstsForExpressions(forStmt.condition.toSeq)
          val conditionHeapConstructors = heapConstructorAstsForExpressions(forStmt.condition.toSeq)
          val updateAsts = forStmt.update.toSeq.flatMap { update =>
            expressionAstsWithRecoveredAggregateAssignments(update) ++
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
        gotoLocalDestructors(gotoStmt.label).map(localDestructorAst) :+
          Ast(controlStructureNode(OxOrigin(gotoStmt), ControlStructureTypes.GOTO, gotoStmt.code))
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
              aggregateAssignmentExpressionAsts(deleteExpression.argument) ++
              heapDestructorAstsForDelete(deleteExpression) ++
              Seq(expressionAst(deleteExpression)) ++
              temporaryDestructorAstsForExpressions(Seq(deleteExpression.argument))
          case expression =>
            expressionAstsWithRecoveredAggregateAssignments(expression) ++
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
    val origin           = OxOrigin(local)
    val localLambdaInfo  = local.initializer.collect { case lambda: OxLambda => lambdaInfo(lambda) }
    val typeName         = registerType(localTypeFullName(local))
    val semanticTypeName = registerType(localSemanticTypeFullName(local))
    val localCode        = localDeclarationCode(local)
    val localNode        = this.localNode(origin.copy(code = localCode), local.name, localCode, typeName)
    val localScopeEntry =
      ScopeEntry(typeName, localNode, localLambdaInfo, semanticTypeFullName = Option(semanticTypeName))
    scope = scope.updated(local.name, localScopeEntry)
    val isStaticStorageLocal = hasStaticStorageDuration(local)
    if (!isStaticStorageLocal) {
      registerLocalDestructor(local, typeName)
    }
    val extendedTemporaryDestructor =
      Option
        .when(!isStaticStorageLocal)(local.initializer.flatMap(referenceBoundTemporaryDestructor(typeName, _)))
        .flatten
    extendedTemporaryDestructor.foreach(registerLocalDestructor)
    val extendsCurrentInitializerTemporary =
      extendedTemporaryDestructor.isDefined || (isStaticStorageLocal && isCxxReferenceType(typeName))
    val localAst = Ast(localNode)
    val arrayConstructorAsts =
      if (useConstructorInitializers) localArrayConstructorAsts(local, typeName) else Seq.empty
    val localInitializerTemporaryDestructorAsts =
      temporaryDestructorAstsForLocalInitializer(
        local.initializer,
        Option.when(extendsCurrentInitializerTemporary)(typeName),
        extendsCurrentInitializerTemporary
      )
    val initializationAsts = local.initializer match {
      case Some(initializer: OxInitializerList)
          if useConstructorInitializers && isConstructorInitializer(typeName, initializer) =>
        val resolution = constructorInitializerResolution(typeName, initializer)
        resolution.arguments.flatMap(aggregateAssignmentExpressionAsts) ++ Seq(
          constructorAssignmentAst(local, initializer, typeName, resolution)
        ) ++
          temporaryDestructorAstsForConstructorArguments(resolution.arguments, resolution.entry)
      case Some(initializer) if useConstructorInitializers && isCopyConstructorInitializer(typeName, initializer) =>
        val arguments   = Seq(initializer)
        val constructor = constructorEntry(typeName, arguments)
        aggregateAssignmentExpressionAsts(initializer) ++ Seq(
          constructorAssignmentAst(local, arguments, initializer.code, OxOrigin(initializer), typeName)
        ) ++ temporaryDestructorAstsForConstructorArguments(arguments, constructor)
      case Some(_: OxInitializerList) if arrayConstructorAsts.nonEmpty =>
        arrayConstructorAsts
      case Some(initializer) =>
        val (left, targetCode) = localAssignmentTargetAst(local, typeName)
        val assignmentCode     = s"$targetCode = ${initializer.code}"
        val assignment =
          assignmentAst(
            origin.copy(code = assignmentCode),
            left,
            expressionAstWithContextualConversion(initializer, Option(typeName)),
            assignmentCode
          )
        val initializerAggregateAssignments = aggregateAssignmentExpressionAsts(initializer)
        val fieldAssignments                = aggregateInitializerAssignmentAsts(local, initializer, typeName)
        initializerAggregateAssignments ++ Seq(assignment) ++ fieldAssignments ++ heapConstructorAstsForExpressions(
          Seq(initializer)
        ) ++ localInitializerTemporaryDestructorAsts
      case None if arrayConstructorAsts.nonEmpty =>
        arrayConstructorAsts
      case None if useConstructorInitializers && isDefaultConstructorInitializer(typeName) =>
        Seq(constructorAssignmentAst(local, Seq.empty, "", origin, typeName))
      case None =>
        Seq.empty
    }
    val guardedInitializationAsts =
      if (isStaticStorageLocal) staticLocalInitializationAsts(local, typeName, initializationAsts)
      else initializationAsts
    Seq(localAst) ++ guardedInitializationAsts
  }

  private def staticLocalInitializationAsts(
    local: OxLocalDecl,
    typeName: String,
    initializationAsts: Seq[Ast]
  ): Seq[Ast] = {
    Option
      .when(initializationAsts.nonEmpty) {
        val guardName  = s"<static-init>${local.name}"
        val guardCode  = guardName
        val guardType  = registerType("bool")
        val guardLocal = localNode(OxOrigin(guardCode, Option(local.line)), guardName, guardCode, guardType)
        val guardEntry = ScopeEntry(guardType, guardLocal)
        registerStaticLocalStorage(local, typeName)
        val guardCondition = staticLocalGuardConditionAst(guardName, guardEntry, local.line)
        val guardSet       = staticLocalGuardAssignmentAst(guardName, guardEntry, local.line)
        val ifCode         = s"if (!$guardCode)"
        val ifNode = controlStructureNode(OxOrigin(ifCode, Option(local.line)), ControlStructureTypes.IF, ifCode)
        val thenAst =
          blockAst(
            blockNode(OxOrigin("then", Option(local.line)), "then", Defines.Any),
            (initializationAsts :+ guardSet).toList
          )
        Seq(Ast(guardLocal), ifThenElseAst(ifNode, Option(guardCondition), thenAst, None))
      }
      .getOrElse(Seq.empty)
  }

  private def staticLocalGuardConditionAst(guardName: String, guardEntry: ScopeEntry, line: Int): Ast = {
    val guardAst = identifierAstForScopeEntry(guardName, guardName, line, guardEntry)
    operatorCallAst(
      OxOrigin(s"!$guardName", Option(line)),
      s"!$guardName",
      Operators.logicalNot,
      Seq(guardAst),
      registerType("bool")
    )
  }

  private def staticLocalGuardAssignmentAst(guardName: String, guardEntry: ScopeEntry, line: Int): Ast = {
    val assignmentCode = s"$guardName = true"
    val left           = identifierAstForScopeEntry(guardName, guardName, line, guardEntry)
    val right          = expressionAst(OxLiteral("true", "true", line))
    assignmentAst(OxOrigin(assignmentCode, Option(line)), left, right, assignmentCode)
  }

  private def registerStaticLocalStorage(local: OxLocalDecl, typeName: String): Unit = {
    val ownerPrefix = currentMethodSimpleName.map(simpleName => s"$simpleName::").getOrElse("")
    staticLocalStorages = staticLocalStorages :+ StaticLocalStorage(
      local,
      typeName,
      receiverPrefix = s"$ownerPrefix${local.name}",
      guardName = s"<static-init>${local.name}",
      guardReceiverCode = s"$ownerPrefix<static-init>${local.name}"
    )
  }

  private def staticLocalDestructorAsts(): Seq[Ast] = {
    staticLocalStorages.reverse.flatMap(staticLocalDestructorAst)
  }

  private def staticLocalDestructorAst(storage: StaticLocalStorage): Seq[Ast] = {
    val destructorAsts = staticLocalDestructorCallAsts(storage)
    Option
      .when(destructorAsts.nonEmpty) {
        val guardCondition = staticLocalSyntheticIdentifierAst(
          storage.guardName,
          storage.guardReceiverCode,
          storage.local.line,
          registerType("bool")
        )
        val ifCode = s"if (${storage.guardReceiverCode})"
        val ifNode =
          controlStructureNode(OxOrigin(ifCode, Option(storage.local.line)), ControlStructureTypes.IF, ifCode)
        val thenAst =
          blockAst(blockNode(OxOrigin("then", Option(storage.local.line)), "then", Defines.Any), destructorAsts.toList)
        ifThenElseAst(ifNode, Option(guardCondition), thenAst, None)
      }
      .toSeq
  }

  private def staticLocalDestructorCallAsts(storage: StaticLocalStorage): Seq[Ast] = {
    val referenceTemporaryDestructor =
      staticReferenceBoundTemporaryDestructor(storage).map(temporaryDestructorAst).toSeq
    val arrayDestructors = for {
      count       <- localArrayElementCount(storage.local).toSeq
      elementType <- arrayElementTypeFullName(storage.typeName).toSeq
      entry       <- destructorEntryForType(elementType).toSeq
      index       <- (0 until count).reverse
    } yield {
      val receiverCode = s"${storage.receiverPrefix}[$index]"
      localDestructorAst(
        LocalDestructor(receiverCode, storage.local.line, entry),
        base = arrayElementAccessAst(
          storage.local.name,
          storage.typeName,
          index,
          storage.local.line,
          baseCode = Option(storage.receiverPrefix)
        )
      )
    }
    if (referenceTemporaryDestructor.nonEmpty) {
      referenceTemporaryDestructor
    } else if (arrayDestructors.nonEmpty) {
      arrayDestructors
    } else {
      destructorEntryForType(storage.typeName).toSeq.map { entry =>
        localDestructorAst(
          LocalDestructor(storage.receiverPrefix, storage.local.line, entry),
          base = staticLocalSyntheticIdentifierAst(
            storage.local.name,
            storage.receiverPrefix,
            storage.local.line,
            registerType(storage.typeName)
          )
        )
      }
    }
  }

  private def staticReferenceBoundTemporaryDestructor(storage: StaticLocalStorage): Option[TemporaryDestructor] = {
    storage.local.initializer.flatMap { initializer =>
      Option
        .when(isCxxReferenceType(storage.typeName)) {
          temporaryTypeFullNameForExpression(initializer, Option(storage.typeName))
            .flatMap(destructorEntryForType)
            .map(entry =>
              TemporaryDestructor(
                temporaryDestructorCode(initializer, entry, Option(storage.typeName)),
                initializer.line,
                entry
              )
            )
        }
        .flatten
    }
  }

  private def staticLocalSyntheticIdentifierAst(name: String, code: String, line: Int, typeName: String): Ast = {
    Ast(identifierNode(OxOrigin(code, Option(line)), name, code, typeName))
  }

  private def localArrayConstructorAsts(local: OxLocalDecl, typeName: String): Seq[Ast] = {
    local.initializer match {
      case Some(initializer: OxInitializerList) =>
        localArrayInitializerConstructorAsts(local, typeName, initializer)
      case None =>
        localArrayDefaultConstructorAsts(local, typeName)
      case _ =>
        Seq.empty
    }
  }

  private def localArrayDefaultConstructorAsts(local: OxLocalDecl, typeName: String): Seq[Ast] = {
    for {
      count       <- localArrayElementCount(local).toSeq
      elementType <- arrayElementTypeFullName(typeName).toSeq
      info        <- constructorInvocationInfo(elementType, Seq.empty, "").toSeq
      index       <- 0 until count
    } yield localArrayElementConstructorAssignmentAst(local.name, typeName, index, local.line, info)
  }

  private def localArrayInitializerConstructorAsts(
    local: OxLocalDecl,
    typeName: String,
    initializer: OxInitializerList
  ): Seq[Ast] = {
    if (initializer.elements.exists(_.isInstanceOf[OxDesignatedInitializer])) {
      Seq.empty
    } else {
      val count       = localArrayElementCount(local)
      val elementType = arrayElementTypeFullName(typeName)
      (count, elementType) match {
        case (Some(elementCount), Some(elementTypeName)) =>
          val explicitInitializers = initializer.elements.take(elementCount)
          val explicitConstructorAsts = explicitInitializers.zipWithIndex.map { case (elementInitializer, index) =>
            localArrayElementInitializerConstructorAsts(local, typeName, elementTypeName, index, elementInitializer)
          }
          if (explicitConstructorAsts.exists(_.isEmpty)) {
            Seq.empty
          } else {
            val defaultConstructorAsts = for {
              info  <- constructorInvocationInfo(elementTypeName, Seq.empty, "").toSeq
              index <- explicitInitializers.size until elementCount
            } yield localArrayElementConstructorAssignmentAst(local.name, typeName, index, local.line, info)
            explicitConstructorAsts.flatten ++ defaultConstructorAsts
          }
        case _ =>
          Seq.empty
      }
    }
  }

  private def localArrayElementInitializerConstructorAsts(
    local: OxLocalDecl,
    arrayTypeName: String,
    elementTypeName: String,
    index: Int,
    initializer: OxExpression
  ): Seq[Ast] = {
    arrayElementConstructorInvocationInfo(elementTypeName, initializer).toSeq.flatMap {
      case (info, arguments, constructorEntry) =>
        arguments.flatMap(aggregateAssignmentExpressionAsts) ++
          Seq(localArrayElementConstructorAssignmentAst(local.name, arrayTypeName, index, local.line, info)) ++
          temporaryDestructorAstsForConstructorArguments(arguments, constructorEntry)
    }
  }

  private def arrayElementConstructorInvocationInfo(
    elementTypeName: String,
    initializer: OxExpression
  ): Option[(ConstructorInvocationInfo, Seq[OxExpression], Option[FunctionEntry])] = {
    val resolvedElementTypeName =
      resolveAggregateTypeFullName(receiverAggregateTypeName(elementTypeName)).getOrElse(elementTypeName)
    initializer match {
      case initializerList: OxInitializerList if isConstructorInitializer(resolvedElementTypeName, initializerList) =>
        val resolution = constructorInitializerResolution(resolvedElementTypeName, initializerList)
        val initCode = normalizedConstructorInitCode(initializerList.code.trim, resolution.preserveInitializerListCode)
        constructorInvocationInfo(resolvedElementTypeName, resolution.arguments, initCode, resolution.entry)
          .map(info => (info, resolution.arguments, resolution.entry))
      case _ =>
        val arguments   = Seq(initializer)
        val constructor = constructorEntry(resolvedElementTypeName, arguments)
        constructorInvocationInfo(resolvedElementTypeName, arguments, initializer.code, constructor)
          .map(info => (info, arguments, constructor))
    }
  }

  private def localArrayElementConstructorAssignmentAst(
    localName: String,
    arrayTypeName: String,
    index: Int,
    line: Int,
    info: ConstructorInvocationInfo
  ): Ast = {
    val elementCode    = s"$localName[$index]"
    val assignmentCode = s"$elementCode = ${info.code}"
    val left           = arrayElementAccessAst(localName, arrayTypeName, index, line)
    val callNode_      = constructorCallNode(OxOrigin(info.code, Option(line)), info)
    val right = constructorInvocationBlockAst(OxOrigin(info.code, Option(line)), info.typeName, callNode_, Seq.empty)
    assignmentAst(OxOrigin(assignmentCode, Option(line)), left, right, assignmentCode)
  }

  private def arrayElementAccessAst(
    localName: String,
    arrayTypeName: String,
    index: Int,
    line: Int,
    scopeEntry: Option[ScopeEntry] = None,
    baseCode: Option[String] = None
  ): Ast = {
    val arrayCode       = baseCode.getOrElse(localName)
    val elementCode     = s"$arrayCode[$index]"
    val elementTypeName = arrayElementTypeFullName(arrayTypeName).getOrElse(Defines.Any)
    val baseAst = scopeEntry
      .map(identifierAstForScopeEntry(localName, arrayCode, line, _))
      .getOrElse(identifierAst(localName, arrayCode, line))
    operatorCallAst(
      OxOrigin(elementCode, Option(line)),
      elementCode,
      Operators.indirectIndexAccess,
      Seq(baseAst, expressionAst(OxLiteral(index.toString, index.toString, line))),
      registerType(elementTypeName)
    )
  }

  private def localArrayElementReceiverCodes(local: OxLocalDecl): Seq[String] = {
    localArrayElementCount(local).toSeq.flatMap(count => 0 until count).map(index => s"${local.name}[$index]")
  }

  private def localArrayElementCount(local: OxLocalDecl): Option[Int] = {
    val nameIndex = local.code.indexOf(local.name)
    Option
      .when(nameIndex >= 0)(local.code.drop(nameIndex + local.name.length).dropWhile(_.isWhitespace))
      .filter(_.startsWith("["))
      .flatMap { suffix =>
        val endIndex = suffix.indexOf(']')
        Option.when(endIndex > 1)(suffix.substring(1, endIndex).trim)
      }
      .flatMap(rawCount => Try(rawCount.toInt).toOption)
      .filter(_ > 0)
  }

  private def memberArrayElementAccessAst(field: OxFieldDecl, index: Int, line: Int): Ast = {
    val elementCode     = s"${Defines.This}->${field.name}[$index]"
    val elementTypeName = arrayElementTypeFullName(field.typeName).getOrElse(Defines.Any)
    val fieldAst = implicitFieldAccessAst(field.name, line).getOrElse(identifierAst(field.name, field.name, line))
    operatorCallAst(
      OxOrigin(elementCode, Option(line)),
      elementCode,
      Operators.indirectIndexAccess,
      Seq(fieldAst, expressionAst(OxLiteral(index.toString, index.toString, line))),
      registerType(elementTypeName)
    )
  }

  private def fieldArrayElementCount(field: OxFieldDecl): Option[Int] = {
    val nameIndex = field.code.indexOf(field.name)
    Option
      .when(nameIndex >= 0)(field.code.drop(nameIndex + field.name.length).dropWhile(_.isWhitespace))
      .filter(_.startsWith("["))
      .flatMap { suffix =>
        val endIndex = suffix.indexOf(']')
        Option.when(endIndex > 1)(suffix.substring(1, endIndex).trim)
      }
      .flatMap(rawCount => Try(rawCount.toInt).toOption)
      .filter(_ > 0)
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

  private def aggregateInitializerAssignmentAsts(
    local: OxLocalDecl,
    initializer: OxExpression,
    typeName: String
  ): Seq[Ast] = {
    aggregateInitializerAssignmentAsts(AggregateAssignmentRoot(local.name, local.line, None), initializer, typeName)
  }

  private def aggregateInitializerAssignmentAsts(
    root: AggregateAssignmentRoot,
    initializer: OxExpression,
    typeName: String
  ): Seq[Ast] = {
    initializer match {
      case initializerList: OxInitializerList if isAggregateFieldType(typeName) || isArrayLikeType(typeName) =>
        aggregateInitializerAssignmentAsts(root, rootTypeName = typeName, typeName, initializerList, Seq.empty)
      case _ => Seq.empty
    }
  }

  private def aggregateInitializerAssignmentAsts(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    typeName: String,
    initializer: OxInitializerList,
    fieldPathPrefix: Seq[AggregatePathSegment]
  ): Seq[Ast] = {
    val OxInitializerList(_, _, elements) = initializer
    if (isArrayLikeType(typeName)) {
      aggregateArrayInitializerAssignmentAsts(root, rootTypeName, typeName, initializer, fieldPathPrefix)
    } else if (isAggregateFieldType(typeName)) {
      val assignments =
        if (elements.exists(_.isInstanceOf[OxDesignatedInitializer])) {
          elements.flatMap {
            case OxDesignatedInitializer(code, line, designator, value) =>
              aggregateDesignatedFieldAssignmentAsts(
                root,
                rootTypeName,
                typeName,
                fieldPathPrefix,
                code,
                designator,
                value,
                line
              )
            case _ =>
              Seq.empty
          }
        } else {
          aggregatePositionalFieldAssignmentAsts(root, rootTypeName, typeName, fieldPathPrefix, elements)
        }
      assignments
    } else {
      Seq.empty
    }
  }

  private def aggregatePositionalFieldAssignmentAsts(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    typeName: String,
    fieldPathPrefix: Seq[AggregatePathSegment],
    elements: Seq[OxExpression]
  ): Seq[Ast] = {
    var targets = aggregateInitializerSlots(typeName).toList
    elements.flatMap { value =>
      val assignments = mutable.ArrayBuffer.empty[Ast]
      var assigned    = false
      while (!assigned && targets.nonEmpty) {
        targets.head match {
          case AggregateBaseInitializerSlot(baseTypeName) =>
            value match {
              case initializerList: OxInitializerList =>
                assignments ++= aggregateInitializerAssignmentAsts(
                  root,
                  rootTypeName,
                  baseTypeName,
                  initializerList,
                  fieldPathPrefix
                )
                targets = targets.tail
                assigned = true
              case _ =>
                val expandedTargets = aggregateFlattenedFieldInitializerSlots(baseTypeName)
                targets = expandedTargets.toList ++ targets.tail
                if (expandedTargets.isEmpty) {
                  assigned = true
                }
            }
          case AggregateFieldInitializerSlot(ownerTypeName, field) =>
            assignments ++= aggregateFieldAssignmentAsts(
              root,
              rootTypeName,
              ownerTypeName,
              fieldPathPrefix,
              Seq(AggregateFieldPathSegment(field.name)),
              value,
              value.line
            )
            targets = targets.tail
            assigned = true
        }
      }
      assignments.toSeq
    }
  }

  private def aggregateArrayInitializerAssignmentAsts(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    typeName: String,
    initializer: OxInitializerList,
    fieldPathPrefix: Seq[AggregatePathSegment]
  ): Seq[Ast] = {
    val OxInitializerList(_, _, elements) = initializer
    if (elements.exists(_.isInstanceOf[OxDesignatedInitializer])) {
      elements.flatMap {
        case OxDesignatedInitializer(code, line, designator, value) =>
          aggregateDesignatedFieldAssignmentAsts(
            root,
            rootTypeName,
            typeName,
            fieldPathPrefix,
            code,
            designator,
            value,
            line
          )
        case _ =>
          Seq.empty
      }
    } else {
      elements.zipWithIndex.flatMap { case (value, index) =>
        aggregateFieldAssignmentAsts(
          root,
          rootTypeName,
          typeName,
          fieldPathPrefix,
          Seq(AggregateIndexPathSegment(index.toString)),
          value,
          value.line
        )
      }
    }
  }

  private def aggregateInitializerSlots(typeName: String): Seq[AggregateInitializerSlot] = {
    val normalized = aggregateLookupTypeName(typeName)
    val baseSlots  = aggregateBaseTypesByType.getOrElse(normalized, Seq.empty).map(AggregateBaseInitializerSlot.apply)
    val fieldSlots = aggregateDeclarationEntriesByType
      .get(normalized)
      .map { case (structDecl, fullName) => aggregateFieldInitializerSlots(structDecl, fullName) }
      .getOrElse(Seq.empty)
    baseSlots ++ fieldSlots
  }

  private def aggregateFieldInitializerSlots(
    structDecl: OxStructDecl,
    typeName: String
  ): Seq[AggregateFieldInitializerSlot] = {
    aggregateMemberItems(structDecl, typeName).flatMap {
      case AggregateFieldMemberItem(field) =>
        Seq(AggregateFieldInitializerSlot(typeName, field))
      case AggregateAnonymousMemberItem(nestedTypeName, nestedDecl) =>
        val promotedSlots = aggregateFieldInitializerSlots(nestedDecl, nestedTypeName)
        if (isUnionAggregateDecl(nestedDecl)) promotedSlots.take(1) else promotedSlots
    }
  }

  private def aggregateFlattenedFieldInitializerSlots(typeName: String): Seq[AggregateFieldInitializerSlot] = {
    aggregateInitializerSlots(typeName).flatMap {
      case AggregateBaseInitializerSlot(baseTypeName) => aggregateFlattenedFieldInitializerSlots(baseTypeName)
      case fieldSlot: AggregateFieldInitializerSlot   => Seq(fieldSlot)
    }
  }

  private def aggregateDesignatedFieldAssignmentAsts(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    typeName: String,
    fieldPathPrefix: Seq[AggregatePathSegment],
    initializerCode: String,
    designator: OxExpression,
    value: OxExpression,
    line: Int
  ): Seq[Ast] = {
    aggregateDesignatorFieldPath(initializerCode, designator).toSeq.flatMap { fieldPath =>
      aggregateFieldAssignmentAsts(root, rootTypeName, typeName, fieldPathPrefix, fieldPath, value, line)
    }
  }

  private def aggregateFieldAssignmentAsts(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    typeName: String,
    fieldPathPrefix: Seq[AggregatePathSegment],
    fieldPathSuffix: Seq[AggregatePathSegment],
    value: OxExpression,
    line: Int
  ): Seq[Ast] = {
    val fieldPath  = fieldPathPrefix ++ fieldPathSuffix
    val assignment = aggregateFieldAssignmentAst(root, rootTypeName, fieldPath, value, line)
    val nestedAssignments = value match {
      case initializerList: OxInitializerList =>
        fieldPathTypeFullName(typeName, fieldPathSuffix).toSeq.flatMap { fieldType =>
          aggregateInitializerAssignmentAsts(root, rootTypeName, fieldType, initializerList, fieldPath)
        }
      case _ =>
        Seq.empty
    }
    assignment +: nestedAssignments
  }

  private def aggregateDesignatorFieldPath(
    initializerCode: String,
    designator: OxExpression
  ): Option[Seq[AggregatePathSegment]] = {
    val initializerDesignator = initializerCode.takeWhile(_ != '=').trim
    val initializerPath       = aggregateDesignatorFieldPath(initializerDesignator)
    if (hasExplicitDesignatorSyntax(initializerDesignator)) {
      initializerPath
    } else {
      initializerPath.orElse(designator match {
        case designator: OxDesignator => aggregateDesignatorFieldPath(designator)
        case _                        => None
      })
    }
  }

  private def aggregateDesignatorFieldPath(designator: OxDesignator): Option[Seq[AggregatePathSegment]] = {
    val code     = designator.code.trim
    val codePath = aggregateDesignatorFieldPath(code)
    if (hasExplicitDesignatorSyntax(code)) {
      codePath
    } else {
      codePath.orElse(aggregateDesignatorFieldPath(designator.name))
    }
  }

  private def hasExplicitDesignatorSyntax(code: String): Boolean = {
    code.nonEmpty && (code.startsWith(".") || code.contains(".") || code.contains("["))
  }

  private def aggregateDesignatorFieldPath(rawPath: String): Option[Seq[AggregatePathSegment]] = {
    val path = parseAggregateDesignatorPath(rawPath)
    Option.when(path.nonEmpty)(path)
  }

  private def parseAggregateDesignatorPath(rawPath: String): Seq[AggregatePathSegment] = {
    val path     = rawPath.trim
    val segments = mutable.ArrayBuffer.empty[AggregatePathSegment]
    var offset   = 0

    def consumeIdentifier(): Boolean = {
      val start = offset
      while (offset < path.length && isFieldDesignatorSegmentCharacter(path.charAt(offset))) {
        offset += 1
      }
      if (offset > start) {
        val fieldName = path.substring(start, offset)
        if (isFieldDesignatorSegment(fieldName)) {
          segments += AggregateFieldPathSegment(fieldName)
          true
        } else {
          false
        }
      } else {
        false
      }
    }

    while (offset < path.length) {
      path.charAt(offset) match {
        case '.' =>
          offset += 1
          if (!consumeIdentifier()) {
            return Seq.empty
          }
        case '[' =>
          val end = path.indexOf(']', offset + 1)
          if (end <= offset) {
            return Seq.empty
          }
          val indexCode = path.substring(offset + 1, end).trim
          if (!isArrayDesignatorIndex(indexCode)) {
            return Seq.empty
          }
          segments += AggregateIndexPathSegment(indexCode)
          offset = end + 1
        case _ =>
          if (!consumeIdentifier()) {
            return Seq.empty
          }
      }
    }
    segments.toSeq
  }

  private def isFieldDesignatorSegment(segment: String): Boolean = {
    segment.nonEmpty &&
    (segment.head == '_' || segment.head.isLetter) &&
    segment.forall(isFieldDesignatorSegmentCharacter)
  }

  private def isFieldDesignatorSegmentCharacter(character: Char): Boolean = {
    character == '_' || character.isLetterOrDigit
  }

  private def isArrayDesignatorIndex(indexCode: String): Boolean = {
    indexCode.nonEmpty && !indexCode.contains("...") && !indexCode.exists(character =>
      character == '[' || character == ']'
    )
  }

  private def fieldPathTypeFullName(baseTypeFullName: String, fieldPath: Seq[AggregatePathSegment]): Option[String] = {
    fieldPath.foldLeft(Option(resolveAliasType(baseTypeFullName))) {
      case (baseType, AggregateFieldPathSegment(field, _)) =>
        baseType.flatMap(fieldTypeFullName(_, field))
      case (baseType, AggregateIndexPathSegment(_, _)) =>
        baseType.flatMap(arrayElementTypeFullName)
    }
  }

  private def aggregateFieldAssignmentAst(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    fieldPath: Seq[AggregatePathSegment],
    value: OxExpression,
    line: Int
  ): Ast = {
    val (left, fieldCode) = aggregateFieldAccessAst(root, rootTypeName, fieldPath, line)
    val code              = s"$fieldCode = ${value.code}"
    assignmentAst(OxOrigin(code, Option(line)), left, expressionAst(value), code)
  }

  private def aggregateFieldAccessAst(
    root: AggregateAssignmentRoot,
    rootTypeName: String,
    fieldPath: Seq[AggregatePathSegment],
    line: Int
  ): (Ast, String) = {
    val rootAst = root.scopeEntry
      .map(identifierAstForScopeEntry(root.name, root.name, line, _))
      .getOrElse(identifierAst(root.name, root.name, line))
    fieldPath.foldLeft((rootAst, root.name, rootTypeName)) {
      case ((baseAst, baseCode, baseTypeName), AggregateFieldPathSegment(fieldName, isIndirect)) =>
        val operator      = if (isIndirect) "->" else "."
        val fieldCode     = s"$baseCode$operator$fieldName"
        val fieldTypeName = fieldTypeFullName(baseTypeName, fieldName).getOrElse(Defines.Any)
        val accessAst = fieldAccessAstForOperator(
          OxOrigin(fieldCode, Option(line)),
          OxOrigin(fieldName, Option(line)),
          baseAst,
          fieldCode,
          fieldName,
          registerType(fieldTypeName),
          Option(isIndirect)
        )
        (accessAst, fieldCode, fieldTypeName)
      case ((baseAst, baseCode, baseTypeName), AggregateIndexPathSegment(indexCode, indexExpression)) =>
        val indexAccessCode = s"$baseCode[$indexCode]"
        val elementTypeName = arrayElementTypeFullName(baseTypeName).getOrElse(Defines.Any)
        val indexAst =
          indexExpression.map(expressionAst).getOrElse(expressionAst(OxLiteral(indexCode, indexCode, line)))
        val accessAst = operatorCallAst(
          OxOrigin(indexAccessCode, Option(line)),
          indexAccessCode,
          Operators.indirectIndexAccess,
          Seq(baseAst, indexAst),
          registerType(elementTypeName)
        )
        (accessAst, indexAccessCode, elementTypeName)
    } match {
      case (ast, code, _) => ast -> code
    }
  }

  private def isAggregateFieldType(typeName: String): Boolean = {
    aggregateFieldEntriesByType.contains(aggregateLookupTypeName(typeName))
  }

  private def astsForStructuredBinding(binding: OxStructuredBinding): Seq[Ast] = {
    val tempTypeName = if (normalizeType(binding.typeName).startsWith(Defines.Auto)) Defines.Auto else binding.typeName
    val tempLocal = OxLocalDecl(
      name = binding.tempName,
      typeName = tempTypeName,
      semanticTypeName = tempTypeName,
      code = s"$tempTypeName ${binding.tempName}",
      line = binding.line,
      initializer = binding.initializer
    )
    val tempAsts = astsForLocalDecl(tempLocal, useConstructorInitializers = false)
    val tempType = scope.get(binding.tempName).map(_.typeFullName).getOrElse(registerType(Defines.Any))
    tempAsts ++ binding.names.zipWithIndex.flatMap { case (name, index) =>
      val access = structuredBindingAccess(binding.tempName, tempType, name, index, binding.line)
      astsForLocalDecl(
        OxLocalDecl(
          name = name,
          typeName = Defines.Auto,
          semanticTypeName = Defines.Auto,
          code = name,
          line = binding.line,
          initializer = Some(access)
        )
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

  private def arrayElementTypeFullName(typeName: String): Option[String] = {
    val normalized = normalizeType(resolveAliasType(typeName))
    if (normalized.endsWith("[]") && normalized.length > 2) {
      Option(normalized.stripSuffix("[]"))
    } else {
      val bracketIndex = normalized.lastIndexOf('[')
      Option.when(bracketIndex > 0 && normalized.endsWith("]"))(normalized.take(bracketIndex))
    }
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

  private def hasStaticStorageDuration(local: OxLocalDecl): Boolean = {
    """(^|\s)(static|thread_local)\b""".r.findFirstIn(localDeclarationCode(local)).isDefined
  }

  private def stripConstinitSpecifier(code: String): String = {
    code.trim.replaceFirst("""^constinit\s+""", "")
  }

  private def localTypeFullName(local: OxLocalDecl): String = {
    val explicitType = typeFullNameWithStringLiteralLength(local.typeName, local.initializer)
    local.initializer match {
      case Some(lambda: OxLambda) if explicitType == Defines.Auto => lambdaInfo(lambda).fullName
      case Some(initializerList: OxInitializerList)
          if isAutoType(explicitType) && isDirectListInitializer(local, initializerList) =>
        initializerListElementTypeFullName(initializerList)
          .flatMap(typeName => inferredAutoTypeFullName(explicitType, typeName, preserveCv = false))
          .getOrElse(explicitType)
      case Some(initializer) if isAutoType(explicitType) =>
        expressionTypeFullName(initializer)
          .flatMap(typeName => inferredAutoTypeFullName(explicitType, typeName, preserveCv = false))
          .getOrElse(explicitType)
      case _ => explicitType
    }
  }

  private def localSemanticTypeFullName(local: OxLocalDecl): String = {
    val explicitType = typeFullNameWithStringLiteralLength(local.semanticTypeName, local.initializer)
    local.initializer match {
      case Some(lambda: OxLambda) if explicitType == Defines.Auto => lambdaInfo(lambda).fullName
      case Some(initializerList: OxInitializerList)
          if isAutoType(explicitType) && isDirectListInitializer(local, initializerList) =>
        initializerListElementTypeFullName(initializerList)
          .flatMap(typeName => inferredAutoTypeFullName(explicitType, typeName, preserveCv = true))
          .getOrElse(explicitType)
      case Some(initializer) if isAutoType(explicitType) =>
        expressionTypeFullName(initializer)
          .flatMap(typeName => inferredAutoTypeFullName(explicitType, typeName, preserveCv = true))
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

  private def isAutoType(typeName: String): Boolean = {
    stripCxxTypeQualifiers(normalizeType(typeName)).trim.startsWith(Defines.Auto)
  }

  private def isDecltypeAutoType(typeName: String): Boolean = {
    stripCxxTypeQualifiers(normalizeType(typeName)).trim == "decltype(auto)"
  }

  private def inferredAutoTypeFullName(
    explicitType: String,
    initializerType: String,
    preserveCv: Boolean
  ): Option[String] = {
    val explicit                = normalizeType(explicitType)
    val explicitWithoutCv       = stripCxxTypeQualifiers(explicit).trim
    val explicitTypeQualifiers  = cxxTypeQualifiers(explicit)
    val resolvedInitializerType = normalizeType(resolveAliasType(initializerType))
    val initializerObjectType   = stripCxxReference(resolvedInitializerType)
    def valueBase: String = {
      val base = stripCxxTypeQualifiers(initializerObjectType).trim
      if (preserveCv) addMissingCxxTypeQualifiers(base, explicitTypeQualifiers) else base
    }
    def referenceBase: String = {
      val base = if (preserveCv) initializerObjectType else stripCxxTypeQualifiers(initializerObjectType).trim
      if (preserveCv) addMissingCxxTypeQualifiers(base, explicitTypeQualifiers) else base
    }
    explicitWithoutCv match {
      case Defines.Auto =>
        Some(valueBase)
      case "auto*" if resolvedInitializerType.endsWith("*") =>
        Some(
          if (preserveCv) addMissingCxxTypeQualifiers(resolvedInitializerType, explicitTypeQualifiers)
          else stripCxxTypeQualifiers(resolvedInitializerType).trim
        )
      case "auto&" =>
        Some(s"$referenceBase&")
      case "auto&&" =>
        Some(s"$referenceBase&&")
      case _ =>
        None
    }
  }

  private def cxxTypeQualifiers(typeName: String): Seq[String] = {
    typeName.split("\\s+").filter(CxxTypeQualifiers.contains).distinct.toSeq
  }

  private def addMissingCxxTypeQualifiers(typeName: String, qualifiers: Seq[String]): String = {
    val existing = cxxTypeQualifiers(typeName).toSet
    val missing  = qualifiers.filterNot(existing.contains)
    (missing ++ Seq(typeName)).mkString(" ").trim
  }

  private def isConstructorInitializer(typeName: String, initializer: OxInitializerList): Boolean = {
    val initializerCode = initializer.code.trim
    aggregateTypeFullNames.contains(typeName) &&
    (initializerCode.startsWith("(") || (initializerCode
      .startsWith("{") && constructorInitializerResolution(typeName, initializer).entry.isDefined))
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

  private def constructorAssignmentAst(
    local: OxLocalDecl,
    initializer: OxInitializerList,
    typeName: String,
    resolution: ConstructorInitializerResolution
  ): Ast = {
    constructorAssignmentAst(
      local,
      resolution.arguments,
      initializer.code.trim,
      OxOrigin(initializer),
      typeName,
      resolvedConstructor = resolution.entry,
      preserveInitializerListCode = resolution.preserveInitializerListCode
    )
  }

  private def constructorAssignmentAst(
    local: OxLocalDecl,
    arguments: Seq[OxExpression],
    initializerCode: String,
    initializerOrigin: OxOrigin,
    typeName: String,
    resolvedConstructor: Option[FunctionEntry] = None,
    preserveInitializerListCode: Boolean = false
  ): Ast = {
    val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
    val constructor     = resolvedConstructor.orElse(constructorEntry(typeName, arguments))
    val implicitSignature =
      Option.when(arguments.isEmpty && hasImplicitDefaultConstructor(typeName))("void()")
    val signature = constructor.map(_.function.signature).orElse(implicitSignature)
    val methodFullName = constructor
      .map(_.fullName)
      .orElse(signature.map(sig => s"$typeName.$constructorName:$sig"))
      .getOrElse(s"$typeName.$constructorName")
    val initCode        = normalizedConstructorInitCode(initializerCode, preserveInitializerListCode)
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
    val argumentAsts = constructor
      .map(entry => argumentAstsForFunctionEntry(entry, arguments))
      .getOrElse(arguments.map(expressionAst))
    val right =
      constructorInvocationBlockAst(initializerOrigin, typeName, callNode_, argumentAsts)
    assignmentAst(OxOrigin(local).copy(code = assignmentCode), left, right, assignmentCode)
  }

  private def normalizedConstructorInitCode(initializerCode: String, preserveInitializerListCode: Boolean): String = {
    if (preserveInitializerListCode) initializerCode
    else if (initializerCode.startsWith("(") && initializerCode.endsWith(")"))
      initializerCode.stripPrefix("(").stripSuffix(")")
    else if (initializerCode.startsWith("{") && initializerCode.endsWith("}"))
      initializerCode.stripPrefix("{").stripSuffix("}")
    else initializerCode
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

  private def constructorInitializerResolution(
    typeName: String,
    initializer: OxInitializerList
  ): ConstructorInitializerResolution = {
    val initializerListEntry = initializerListConstructorEntry(typeName, initializer)
    initializerListEntry match {
      case Some(entry) =>
        ConstructorInitializerResolution(Seq(initializer), Option(entry), preserveInitializerListCode = true)
      case None =>
        ConstructorInitializerResolution(initializer.elements, constructorEntry(typeName, initializer.elements))
    }
  }

  private def initializerListConstructorEntry(
    typeName: String,
    initializer: OxInitializerList
  ): Option[FunctionEntry] = {
    Option
      .when(initializer.code.trim.startsWith("{")) {
        val candidates = constructorEntriesForType(typeName).filter { entry =>
          entry.function.parameters match {
            case Seq(parameter) => isStdInitializerListType(parameter.typeName)
            case _              => false
          }
        }
        selectFunctionEntry(candidates, Some(Seq(initializer)))
      }
      .flatten
  }

  private def isStdInitializerListType(typeName: String): Boolean = {
    normalizeType(resolveAliasType(typeName)).startsWith("std.initializer_list<")
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
      case OxAssignment(_, _, _, left, right) =>
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
      case OxCast(_, _, _, _, value) =>
        heapConstructorsForExpression(value)
      case OxSizeOf(_, _, value, _) =>
        value.toSeq.flatMap(heapConstructorsForExpression)
      case OxNew(_, _, _, arguments, initializerArguments) =>
        (arguments ++ initializerArguments).flatMap(heapConstructorsForExpression)
      case OxDelete(_, _, argument) =>
        heapConstructorsForExpression(argument)
      case OxLambda(_, _, captures, _, _, _, _, _, _) =>
        captures.flatMap(_.initializer).flatMap(heapConstructorsForExpression)
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
      val resolution = heapConstructorInitializerResolution(typeName, newExpression)
      val initCode =
        if (resolution.preserveInitializerListCode) resolution.arguments.headOption.map(_.code).getOrElse("")
        else newExpression.initializerArguments.map(_.code).mkString(", ")
      constructorInvocationInfo(typeName, resolution.arguments, initCode, resolution.entry).map { info =>
        HeapConstructor(newExpression.line, info, resolution.arguments)
      }
    }
  }

  private def heapConstructorInitializerResolution(
    typeName: String,
    newExpression: OxNew
  ): ConstructorInitializerResolution = {
    heapInitializerList(newExpression).flatMap { initializer =>
      initializerListConstructorEntry(typeName, initializer).map { entry =>
        ConstructorInitializerResolution(Seq(initializer), Option(entry), preserveInitializerListCode = true)
      }
    } match {
      case Some(resolution) => resolution
      case None =>
        ConstructorInitializerResolution(
          newExpression.initializerArguments,
          constructorEntry(typeName, newExpression.initializerArguments)
        )
    }
  }

  private def heapInitializerList(newExpression: OxNew): Option[OxInitializerList] = {
    val code       = newExpression.code.trim
    val braceStart = code.indexOf('{')
    val braceEnd   = code.lastIndexOf('}')
    val parenStart = code.indexOf('(')
    Option.when(braceStart >= 0 && braceEnd > braceStart && (parenStart < 0 || braceStart < parenStart)) {
      OxInitializerList(
        code.substring(braceStart, braceEnd + 1),
        newExpression.line,
        newExpression.initializerArguments
      )
    }
  }

  private def heapConstructorAst(constructor: HeapConstructor): Ast = {
    val callNode_ = constructorCallNode(OxOrigin(constructor.info.code, Option(constructor.line)), constructor.info)
    val argumentAsts = constructor.info.constructor
      .map(entry => argumentAstsForFunctionEntry(entry, constructor.arguments))
      .getOrElse(constructor.arguments.map(expressionAst))
    createCallAst(callNode_, argumentAsts)
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
        val isArray      = isArrayDelete(deleteExpression)
        val destructorCode =
          if (isArray) s"$receiverCode[].${entry.simpleName}()"
          else s"$receiverCode->${entry.simpleName}()"
        HeapDestructor(destructorCode, deleteExpression.line, entry, deleteExpression.argument, isArray)
      }
    }
  }

  private def isArrayDelete(deleteExpression: OxDelete): Boolean = {
    deleteExpression.code.trim.startsWith("delete[]")
  }

  private def heapDestructorAst(destructor: HeapDestructor): Ast = {
    val dispatchType =
      if (!destructor.isArrayDelete && isVirtualFunctionEntry(destructor.entry)) DispatchTypes.DYNAMIC_DISPATCH
      else DispatchTypes.STATIC_DISPATCH
    val callNode_ =
      callNode(
        OxOrigin(destructor.code, Option(destructor.line)),
        destructor.code,
        destructor.entry.simpleName,
        destructor.entry.fullName,
        dispatchType,
        Option(destructor.entry.function.signature),
        Option(registerType(Defines.Void))
      )
    val base = expressionAst(destructor.receiver)
    createCallAst(
      callNode_,
      base = Option(base),
      receiver = Option.when(dispatchType == DispatchTypes.DYNAMIC_DISPATCH)(base)
    )
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

  private def temporaryDestructorAstsForReturnExpression(
    expression: Option[OxExpression],
    expectedTypeFullName: Option[String]
  ): Seq[Ast] = {
    expression.toSeq
      .flatMap(expression =>
        temporaryDestructorsForExpression(
          expression,
          expectedTypeFullName = expectedTypeFullName,
          includeCurrent = !isCurrentReturnedObjectTemporary(expression, expectedTypeFullName)
        )
      )
      .reverse
      .map(temporaryDestructorAst)
  }

  private def temporaryDestructorAstsForLocalInitializer(
    expression: Option[OxExpression],
    expectedTypeFullName: Option[String],
    extendCurrentTemporaryLifetime: Boolean
  ): Seq[Ast] = {
    expression.toSeq
      .flatMap(expression =>
        temporaryDestructorsForExpression(
          expression,
          expectedTypeFullName = expectedTypeFullName,
          includeCurrent = !extendCurrentTemporaryLifetime
        )
      )
      .reverse
      .map(temporaryDestructorAst)
  }

  private def temporaryDestructorAstsForConstructorArguments(
    arguments: Seq[OxExpression],
    entry: Option[FunctionEntry]
  ): Seq[Ast] = {
    arguments.zipWithIndex
      .flatMap { case (argument, index) =>
        val expectedTypeFullName = entry.flatMap(_.function.parameters.lift(index).map(_.typeName))
        temporaryDestructorsForExpression(argument, expectedTypeFullName = expectedTypeFullName)
      }
      .reverse
      .map(temporaryDestructorAst)
  }

  private def temporaryDestructorsForExpression(
    expression: OxExpression,
    expectedTypeFullName: Option[String] = None,
    includeCurrent: Boolean = true
  ): Seq[TemporaryDestructor] = {
    val nested = expression match {
      case OxBinary(_, _, _, left, right) =>
        Seq(left, right).flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxAssignment(_, _, _, left, right) =>
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
      case cast @ OxCast(_, _, _, _, value) =>
        val castType            = temporaryTypeFullNameForExpression(cast)
        val valueType           = temporaryTypeFullNameForExpression(value)
        val includeValueCurrent = castType.isEmpty || castType != valueType
        temporaryDestructorsForExpression(value, includeCurrent = includeValueCurrent)
      case OxSizeOf(_, _, value, _) =>
        value.toSeq.flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxNew(_, _, _, arguments, initializerArguments) =>
        (arguments ++ initializerArguments).flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxDelete(_, _, argument) =>
        temporaryDestructorsForExpression(argument)
      case OxLambda(_, _, captures, _, _, _, _, _, _) =>
        captures.flatMap(_.initializer).flatMap(expression => temporaryDestructorsForExpression(expression))
      case OxCall(_, _, _, callee, arguments) =>
        temporaryDestructorsForExpression(callee) ++ temporaryDestructorsForCallArguments(expression)
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
      case expression => currentTemporaryDestructorsForExpression(expression, expectedTypeFullName, includeCurrent)
    }
    nested ++ current
  }

  private def currentTemporaryDestructorsForExpression(
    expression: OxExpression,
    expectedTypeFullName: Option[String],
    includeCurrent: Boolean
  ): Seq[TemporaryDestructor] = {
    contextualConversionTemporaryTypeFullName(expression, expectedTypeFullName) match {
      case Some(conversionTypeFullName) =>
        val sourceTemporary = temporaryTypeFullNameForExpression(expression)
          .flatMap(destructorEntryForType)
          .map(entry => TemporaryDestructor(temporaryDestructorCode(expression, entry), expression.line, entry))
          .toSeq
        val conversionTemporary =
          Option
            .when(includeCurrent)(conversionTypeFullName)
            .flatMap(destructorEntryForType)
            .map(entry =>
              TemporaryDestructor(
                temporaryDestructorCode(expression, entry, expectedTypeFullName),
                expression.line,
                entry
              )
            )
            .toSeq
        sourceTemporary ++ conversionTemporary
      case None if includeCurrent =>
        temporaryTypeFullNameForExpression(expression)
          .flatMap(destructorEntryForType)
          .map(entry => TemporaryDestructor(temporaryDestructorCode(expression, entry), expression.line, entry))
          .toSeq
      case None =>
        Seq.empty
    }
  }

  private def temporaryDestructorsForCallArguments(call: OxExpression): Seq[TemporaryDestructor] = {
    call match {
      case call: OxCall =>
        targetEntryForCallArguments(call) match {
          case Some(entry) =>
            call.arguments.zipWithIndex.flatMap { case (argument, index) =>
              val expectedType = entry.function.parameters.lift(index).map(_.typeName)
              temporaryDestructorsForExpression(argument, expectedTypeFullName = expectedType)
            }
          case None =>
            call.arguments.flatMap(expression => temporaryDestructorsForExpression(expression))
        }
      case _ =>
        Seq.empty
    }
  }

  private def isCurrentReturnedObjectTemporary(
    expression: OxExpression,
    expectedTypeFullName: Option[String]
  ): Boolean = {
    val currentReturnType = currentMethodReturnedObjectTypeFullName
    val expressionType    = temporaryTypeFullNameForExpression(expression, expectedTypeFullName)
    currentReturnType.isDefined && currentReturnType == expressionType
  }

  private def referenceBoundTemporaryDestructor(
    localTypeName: String,
    expression: OxExpression
  ): Option[LocalDestructor] = {
    Option
      .when(isCxxReferenceType(localTypeName)) {
        temporaryTypeFullNameForExpression(expression, Option(localTypeName))
          .flatMap(destructorEntryForType)
          .map(entry =>
            LocalDestructor(temporaryDestructorReceiverCode(expression, Option(localTypeName)), expression.line, entry)
          )
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

  private def temporaryTypeFullNameForExpression(
    expression: OxExpression,
    expectedTypeFullName: Option[String] = None
  ): Option[String] = {
    contextualConversionTemporaryTypeFullName(expression, expectedTypeFullName).orElse {
      expression match {
        case call: OxCall               => temporaryTypeFullNameForCall(call)
        case conditional: OxConditional => conditionalTemporaryTypeFullName(conditional)
        case cast: OxCast               => castTemporaryTypeFullName(cast)
        case binary: OxBinary           => overloadedBinaryTemporaryTypeFullName(binary)
        case assignment: OxAssignment   => overloadedAssignmentTemporaryTypeFullName(assignment)
        case index: OxIndexAccess       => overloadedIndexTemporaryTypeFullName(index)
        case unary: OxUnary             => overloadedUnaryTemporaryTypeFullName(unary)
        case _                          => None
      }
    }
  }

  private def temporaryTypeFullNameForCall(call: OxCall): Option[String] = {
    constructorTemporaryTypeFullName(call).orElse(returnedObjectTemporaryTypeFullName(call))
  }

  private def conditionalTemporaryTypeFullName(conditional: OxConditional): Option[String] = {
    conditional.consequence.flatMap { consequence =>
      val branchTypes =
        Seq(consequence, conditional.alternative).map(expression => temporaryTypeFullNameForExpression(expression))
      Option
        .when(branchTypes.forall(_.isDefined)) {
          branchTypes.flatten.distinct
        }
        .collect { case Seq(typeName) => typeName }
    }
  }

  private def castTemporaryTypeFullName(cast: OxCast): Option[String] = {
    Option(normalizeType(resolveAliasType(cast.semanticTypeName))).flatMap(returnedObjectTypeFullName)
  }

  private def overloadedBinaryTemporaryTypeFullName(binary: OxBinary): Option[String] = {
    overloadedBinaryOperatorTarget(binary)
      .map(target => functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target)))
      .flatMap(returnedObjectTypeFullName)
  }

  private def overloadedAssignmentTemporaryTypeFullName(assignment: OxAssignment): Option[String] = {
    overloadedAssignmentOperatorTarget(assignment)
      .map(target => functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target)))
      .flatMap(returnedObjectTypeFullName)
  }

  private def overloadedUnaryTemporaryTypeFullName(unary: OxUnary): Option[String] = {
    overloadedUnaryOperatorTarget(unary)
      .map(target => functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target)))
      .flatMap(returnedObjectTypeFullName)
  }

  private def overloadedIndexTemporaryTypeFullName(indexAccess: OxIndexAccess): Option[String] = {
    overloadedIndexOperatorTarget(indexAccess)
      .map(target => functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target)))
      .flatMap(returnedObjectTypeFullName)
  }

  private def temporaryDestructorCode(
    expression: OxExpression,
    entry: FunctionEntry,
    expectedTypeFullName: Option[String] = None
  ): String = {
    s"${temporaryDestructorReceiverCode(expression, expectedTypeFullName)}.${entry.simpleName}()"
  }

  private def temporaryDestructorReceiverCode(
    expression: OxExpression,
    expectedTypeFullName: Option[String] = None
  ): String = {
    contextualConversionOperatorTarget(expression, expectedTypeFullName)
      .map(conversionOperatorCallCode(expression, _))
      .getOrElse {
        expression match {
          case _: OxBinary | _: OxAssignment | _: OxConditional | _: OxCast | _: OxUnary => s"(${expression.code})"
          case _                                                                         => expression.code
        }
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

  private def expressionHasAggregateObjectOrReferenceType(expression: OxExpression): Boolean = {
    expressionTypeFullName(expression).exists { typeName =>
      val normalizedType = normalizeType(resolveAliasType(typeName))
      !normalizedType.endsWith("*") &&
      !normalizedType.endsWith("[]") &&
      resolveAggregateTypeFullName(receiverAggregateTypeName(normalizedType)).isDefined
    }
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
    val typeName         = registerType(normalizeType(parameter.typeName))
    val semanticTypeName = registerType(normalizeType(parameter.semanticTypeName))
    val node = localNode(OxOrigin(parameter.code, Option(parameter.line)), parameter.name, parameter.code, typeName)
    scope = scope.updated(parameter.name, ScopeEntry(typeName, node, semanticTypeFullName = Option(semanticTypeName)))
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
      case labelStmt: OxLabel => statementsMayCompleteNormally(labelStmt.body)
      case caseStmt: OxCase   => statementsMayCompleteNormally(caseStmt.body)
      case _                  => true
    }
  }

  // Snapshot destructor stacks at labels so goto cleanup only destroys scopes it actually leaves.
  private def collectGotoLabelCleanupDestructors(statements: Seq[OxStatement]): Map[String, Seq[LocalDestructor]] = {
    val labels = mutable.LinkedHashMap.empty[String, Seq[LocalDestructor]]

    def activeDestructors(stack: List[Vector[LocalDestructor]]): Seq[LocalDestructor] = {
      stack.flatMap(_.reverse)
    }

    def addDestructors(
      stack: List[Vector[LocalDestructor]],
      destructors: Seq[LocalDestructor]
    ): List[Vector[LocalDestructor]] = {
      stack match {
        case current :: rest => (current ++ destructors) :: rest
        case Nil             => stack
      }
    }

    def localDestructors(local: OxLocalDecl): Seq[LocalDestructor] = {
      val typeName = registerType(localTypeFullName(local))
      if (hasStaticStorageDuration(local)) {
        Seq.empty
      } else {
        localDestructorsForDecl(local, typeName) ++
          local.initializer.flatMap(referenceBoundTemporaryDestructor(typeName, _)).toSeq
      }
    }

    def catchParameterDestructors(parameter: OxParameterDecl): Seq[LocalDestructor] = {
      localDestructorForName(parameter.name, registerType(normalizeType(parameter.typeName)), parameter.line).toSeq
    }

    def visitNestedStatements(statements: Seq[OxStatement], stack: List[Vector[LocalDestructor]]): Unit = {
      visitStatements(statements, Vector.empty[LocalDestructor] :: stack)
      ()
    }

    def visitStatements(
      statements: Seq[OxStatement],
      stack: List[Vector[LocalDestructor]]
    ): List[Vector[LocalDestructor]] = {
      statements.foldLeft(stack) { case (currentStack, statement) =>
        visitStatement(statement, currentStack)
      }
    }

    def visitStatement(statement: OxStatement, stack: List[Vector[LocalDestructor]]): List[Vector[LocalDestructor]] = {
      statement match {
        case local: OxLocalDecl =>
          addDestructors(stack, localDestructors(local))
        case binding: OxStructuredBinding =>
          val tempTypeName =
            if (normalizeType(binding.typeName).startsWith(Defines.Auto)) Defines.Auto else binding.typeName
          val tempLocal = OxLocalDecl(
            name = binding.tempName,
            typeName = tempTypeName,
            semanticTypeName = tempTypeName,
            code = s"$tempTypeName ${binding.tempName}",
            line = binding.line,
            initializer = binding.initializer
          )
          addDestructors(stack, localDestructors(tempLocal))
        case tryStmt: OxTry =>
          visitNestedStatements(tryStmt.body, stack)
          tryStmt.catches.foreach { catchClause =>
            val catchStack =
              addDestructors(
                Vector.empty[LocalDestructor] :: stack,
                catchClause.parameter.toSeq.flatMap(catchParameterDestructors)
              )
            visitNestedStatements(catchClause.body, catchStack)
          }
          stack
        case ifStmt: OxIf =>
          val scopedStack =
            if (ifStmt.initializer.nonEmpty || ifStmt.conditionInitializer.nonEmpty) {
              visitStatements(ifStmt.initializer ++ ifStmt.conditionInitializer, Vector.empty[LocalDestructor] :: stack)
            } else {
              stack
            }
          visitNestedStatements(ifStmt.thenBody, scopedStack)
          visitNestedStatements(ifStmt.elseBody, scopedStack)
          stack
        case whileStmt: OxWhile =>
          val scopedStack =
            if (whileStmt.initializer.nonEmpty || whileStmt.conditionInitializer.nonEmpty) {
              visitStatements(
                whileStmt.initializer ++ whileStmt.conditionInitializer,
                Vector.empty[LocalDestructor] :: stack
              )
            } else {
              stack
            }
          visitNestedStatements(whileStmt.body, scopedStack)
          stack
        case doWhileStmt: OxDoWhile =>
          visitNestedStatements(doWhileStmt.body, stack)
          stack
        case forStmt: OxFor =>
          val scopedStack = visitStatements(forStmt.initializer, Vector.empty[LocalDestructor] :: stack)
          visitNestedStatements(forStmt.body, scopedStack)
          stack
        case switchStmt: OxSwitch =>
          val scopedStack =
            visitStatements(
              switchStmt.initializer ++ switchStmt.conditionInitializer,
              Vector.empty[LocalDestructor] :: stack
            )
          visitStatements(switchStmt.body, scopedStack)
          stack
        case caseStmt: OxCase =>
          visitStatements(caseStmt.body, stack)
        case labelStmt: OxLabel =>
          labels.update(labelStmt.label, activeDestructors(stack))
          visitStatements(labelStmt.body, stack)
        case _: OxReturn | _: OxThrow | _: OxBreak | _: OxContinue | _: OxGoto | _: OxUnknownStatement |
            _: OxUsingEnumStatement | _: OxAssignment | _: OxExpressionStatement =>
          stack
      }
    }

    visitStatements(statements, Vector.empty[LocalDestructor] :: Nil)
    labels.toMap
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
    val initializerAsts = initializers.flatMap(astsForStatement)
    val conditionAst    = if (wrapTruthy) conditionExpressionAst(expression) else expressionAst(expression)
    val conditionAsts   = conditionExpressionAstsWithRecoveredAggregateAssignments(expression, conditionAst)

    if (initializers.isEmpty && conditionAsts.size == 1) {
      conditionAst
    } else {
      val heapConstructorAsts =
        if (initializers.nonEmpty) heapConstructorAstsForExpressions(Seq(expression)) else Seq.empty
      val conditionCode = conditionAst.root
        .collect { case expressionNode: ExpressionNew =>
          expressionNode.code
        }
        .getOrElse(expression.code)
      blockAst(
        blockNode(OxOrigin(conditionCode, Option(expression.line)), conditionCode, Defines.Any),
        (initializerAsts ++ heapConstructorAsts ++ conditionAsts).toList
      )
    }
  }

  private def conditionExpressionAstWithRecoveredAggregateAssignments(
    expression: OxExpression,
    wrapTruthy: Boolean = true
  ): Ast = {
    conditionExpressionAstWithInitializers(Seq.empty, expression, wrapTruthy)
  }

  private def conditionExpressionAstsWithRecoveredAggregateAssignments(
    expression: OxExpression,
    conditionAst: Ast
  ): Seq[Ast] = {
    expression match {
      case assignment @ OxAssignment("=", _, _, _, _: OxInitializerList) =>
        conditionAst +: aggregateAssignmentExpressionAsts(assignment)
      case _ =>
        aggregateAssignmentExpressionAsts(expression) :+ conditionAst
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
      case assignment: OxAssignment =>
        assignmentExpressionAst(assignment)
      case unary: OxUnary =>
        overloadedUnaryOperatorAst(unary).getOrElse(
          operatorCallAst(
            OxOrigin(unary),
            unary.code,
            unaryOperatorFor(unary.operator, unary.prefix),
            Seq(unaryOperandAst(unary))
          )
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

  private def expressionAstsWithRecoveredAggregateAssignments(expression: OxExpression): Seq[Ast] = {
    expression match {
      case assignment @ OxAssignment("=", _, _, _, _: OxInitializerList) =>
        expressionAst(assignment) +: aggregateAssignmentExpressionAsts(assignment)
      case _ =>
        aggregateAssignmentExpressionAsts(expression) :+ expressionAst(expression)
    }
  }

  private def assignmentExpressionAst(assignment: OxAssignment): Ast = {
    overloadedAssignmentOperatorAst(assignment).getOrElse {
      val left  = expressionAst(assignment.left)
      val right = expressionAst(assignment.right)
      if (assignment.operator == "=") {
        assignmentAst(assignmentOrigin(assignment), left, right, assignment.code)
      } else {
        operatorCallAst(
          assignmentOrigin(assignment),
          assignment.code,
          operatorFor(assignment.operator),
          Seq(left, right)
        )
      }
    }
  }

  private def aggregateAssignmentExpressionAsts(assignment: OxAssignment): Seq[Ast] = {
    assignment match {
      case OxAssignment("=", _, _, target, initializer: OxInitializerList) =>
        aggregateAssignmentTarget(target).toSeq.flatMap { assignmentTarget =>
          aggregateInitializerAssignmentAsts(
            assignmentTarget.root,
            assignmentTarget.rootTypeName,
            assignmentTarget.targetTypeName,
            initializer,
            assignmentTarget.fieldPathPrefix
          )
        }
      case _ =>
        Seq.empty
    }
  }

  private def aggregateAssignmentTarget(target: OxExpression): Option[AggregateAssignmentTarget] = {
    target match {
      case identifier: OxIdentifier =>
        expressionTypeFullName(identifier).map { typeName =>
          AggregateAssignmentTarget(
            AggregateAssignmentRoot(identifier.name, identifier.line, aggregateRootScopeEntry(identifier.name)),
            typeName,
            typeName,
            Seq.empty
          )
        }
      case fieldAccess @ OxFieldAccess(field, _, _, base) =>
        aggregateAssignmentTarget(base).flatMap { baseTarget =>
          fieldTypeFullName(baseTarget.targetTypeName, field).map { fieldType =>
            baseTarget.copy(
              targetTypeName = fieldType,
              fieldPathPrefix =
                baseTarget.fieldPathPrefix :+ AggregateFieldPathSegment(field, fieldAccessUsesIndirect(fieldAccess))
            )
          }
        }
      case OxIndexAccess(_, _, base, index) =>
        aggregateAssignmentTarget(base).flatMap { baseTarget =>
          arrayElementTypeFullName(baseTarget.targetTypeName).map { elementType =>
            baseTarget.copy(
              targetTypeName = elementType,
              fieldPathPrefix = baseTarget.fieldPathPrefix :+ AggregateIndexPathSegment(index.code, Option(index))
            )
          }
        }
      case _ =>
        None
    }
  }

  private def aggregateRootScopeEntry(name: String): Option[ScopeEntry] = {
    scope.get(name).orElse(globalScopeByName.get(name))
  }

  private def fieldAccessUsesIndirect(fieldAccess: OxFieldAccess): Boolean = {
    val code     = fieldAccess.code.trim
    val baseCode = fieldAccess.base.code.trim
    if (baseCode.nonEmpty && code.startsWith(baseCode)) {
      code.drop(baseCode.length).trim.startsWith("->")
    } else {
      code.contains("->") && !code.split("->").lastOption.exists(_.contains("."))
    }
  }

  private def aggregateAssignmentExpressionAsts(expression: OxExpression): Seq[Ast] = {
    val nested = expression match {
      case OxBinary(_, _, _, left, right) =>
        Seq(left, right).flatMap(aggregateAssignmentExpressionAsts)
      case OxAssignment(_, _, _, left, right) =>
        Seq(left, right).flatMap(aggregateAssignmentExpressionAsts)
      case OxUnary(_, _, _, _, argument) =>
        aggregateAssignmentExpressionAsts(argument)
      case OxConditional(_, _, condition, consequence, alternative) =>
        aggregateAssignmentExpressionAsts(condition) ++
          consequence.toSeq.flatMap(aggregateAssignmentExpressionAsts) ++
          aggregateAssignmentExpressionAsts(alternative)
      case OxFold(_, _, _, left, right) =>
        left.toSeq.flatMap(aggregateAssignmentExpressionAsts) ++ right.toSeq.flatMap(aggregateAssignmentExpressionAsts)
      case OxPackExpansion(_, _, pattern) =>
        aggregateAssignmentExpressionAsts(pattern)
      case OxTypeOf(_, _, argument) =>
        aggregateAssignmentExpressionAsts(argument)
      case OxCast(_, _, _, _, value) =>
        aggregateAssignmentExpressionAsts(value)
      case OxSizeOf(_, _, value, _) =>
        value.toSeq.flatMap(aggregateAssignmentExpressionAsts)
      case OxNew(_, _, _, arguments, initializerArguments) =>
        (arguments ++ initializerArguments).flatMap(aggregateAssignmentExpressionAsts)
      case OxDelete(_, _, argument) =>
        aggregateAssignmentExpressionAsts(argument)
      case OxLambda(_, _, captures, _, _, _, _, _, _) =>
        captures.flatMap(_.initializer).flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxCall(_, _, _, callee, arguments) =>
        aggregateAssignmentExpressionAsts(callee) ++ arguments.flatMap(aggregateAssignmentExpressionAsts)
      case OxFieldAccess(_, _, _, base) =>
        aggregateAssignmentExpressionAsts(base)
      case OxIndexAccess(_, _, base, index) =>
        Seq(base, index).flatMap(aggregateAssignmentExpressionAsts)
      case OxInitializerList(_, _, elements) =>
        elements.flatMap(aggregateAssignmentExpressionAsts)
      case OxDesignatedInitializer(_, _, designator, value) =>
        Seq(designator, value).flatMap(aggregateAssignmentExpressionAsts)
      case _: OxIdentifier | _: OxLiteral | _: OxDesignator =>
        Seq.empty
    }
    val current = expression match {
      case assignment: OxAssignment => aggregateAssignmentExpressionAsts(assignment)
      case _                        => Seq.empty
    }
    current ++ nested
  }

  private def lambdaCaptureInitializerAssignmentAsts(expression: OxExpression): Seq[Ast] = {
    val nested = expression match {
      case OxBinary(_, _, _, left, right) =>
        Seq(left, right).flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxAssignment(_, _, _, left, right) =>
        Seq(left, right).flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxUnary(_, _, _, _, argument) =>
        lambdaCaptureInitializerAssignmentAsts(argument)
      case OxConditional(_, _, condition, consequence, alternative) =>
        lambdaCaptureInitializerAssignmentAsts(condition) ++
          consequence.toSeq.flatMap(lambdaCaptureInitializerAssignmentAsts) ++
          lambdaCaptureInitializerAssignmentAsts(alternative)
      case OxFold(_, _, _, left, right) =>
        left.toSeq.flatMap(lambdaCaptureInitializerAssignmentAsts) ++
          right.toSeq.flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxPackExpansion(_, _, pattern) =>
        lambdaCaptureInitializerAssignmentAsts(pattern)
      case OxTypeOf(_, _, argument) =>
        lambdaCaptureInitializerAssignmentAsts(argument)
      case OxCast(_, _, _, _, value) =>
        lambdaCaptureInitializerAssignmentAsts(value)
      case OxSizeOf(_, _, value, _) =>
        value.toSeq.flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxNew(_, _, _, arguments, initializerArguments) =>
        (arguments ++ initializerArguments).flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxDelete(_, _, argument) =>
        lambdaCaptureInitializerAssignmentAsts(argument)
      case OxLambda(_, _, captures, _, _, _, _, _, _) =>
        captures.flatMap(_.initializer).flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxCall(_, _, _, callee, arguments) =>
        lambdaCaptureInitializerAssignmentAsts(callee) ++ arguments.flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxFieldAccess(_, _, _, base) =>
        lambdaCaptureInitializerAssignmentAsts(base)
      case OxIndexAccess(_, _, base, index) =>
        Seq(base, index).flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxInitializerList(_, _, elements) =>
        elements.flatMap(lambdaCaptureInitializerAssignmentAsts)
      case OxDesignatedInitializer(_, _, designator, value) =>
        Seq(designator, value).flatMap(lambdaCaptureInitializerAssignmentAsts)
      case _: OxIdentifier | _: OxLiteral | _: OxDesignator =>
        Seq.empty
    }
    val current = expression match {
      case assignment: OxAssignment => expressionAst(assignment) +: aggregateAssignmentExpressionAsts(assignment)
      case _                        => Seq.empty
    }
    current ++ nested
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

  private def expressionAstWithContextualConversion(
    expression: OxExpression,
    expectedTypeFullName: Option[String]
  ): Ast = {
    contextualConversionOperatorTarget(expression, expectedTypeFullName)
      .map(target =>
        astForResolvedOperatorCall(
          OxOrigin(conversionOperatorCallCode(expression, target), Option(expression.line)),
          conversionOperatorCallCode(expression, target),
          target
        )
      )
      .getOrElse(expressionAst(expression))
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
        val name               = nextClosureName()
        val owner              = currentMethodFullName.getOrElse(globalNamespaceBlock().fullName)
        val returnType         = registerType(normalizeType(lambda.returnType))
        val semanticReturnType = registerType(ownerResolvedTypeFullNamePreservingCv(lambda.semanticReturnType, None))
        val signature          = lambda.signature
        val fullName           = s"$owner.$name:$signature"
        lambdaReturnTypesByFullName.update(fullName, returnType)
        lambdaSemanticReturnTypesByFullName.update(fullName, semanticReturnType)
        lambdaSignaturesByFullName.update(fullName, signature)
        LambdaInfo(name, fullName, signature, returnType, semanticReturnType)
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
        case OxLocalDecl(name, _, _, _, _, initializer) =>
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
        case OxAssignment(_, _, _, left, right) =>
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
        case OxCast(_, _, _, _, value) =>
          visitExpression(value)
        case OxSizeOf(_, _, value, _) =>
          value.foreach(visitExpression)
        case OxNew(_, _, _, arguments, initializerArguments) =>
          arguments.foreach(visitExpression)
          initializerArguments.foreach(visitExpression)
        case OxDelete(_, _, argument) =>
          visitExpression(argument)
        case OxLambda(_, _, captures, _, _, _, _, _, _) =>
          captures.flatMap(_.initializer).foreach(visitExpression)
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
      val parameterType         = registerType(normalizeType(parameter.typeName))
      val semanticParameterType = registerType(normalizeType(parameter.semanticTypeName))
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
      parameter.name -> (parameterType, semanticParameterType, Ast(node), node)
    }

    val previousScope            = scope
    val previousCaptureContext   = functionCaptureContext
    val previousMethodOwner      = currentMethodOwnerTypeFullName
    val previousMethodFullName   = currentMethodFullName
    val previousMethodSimpleName = currentMethodSimpleName
    val previousMethodIsConst    = currentMethodIsConst
    val previousMethodReturnType = currentMethodReturnTypeFullName
    val previousDestructorScopes = localDestructorScopes
    val previousJumpTargets      = jumpCleanupTargets
    val previousGotoLabels       = gotoLabelCleanupDestructors
    scope = (captures.map(capture => capture.name -> capture.scopeEntry) ++
      parameterEntries.map { case (name, (typeName, semanticTypeName, _, node)) =>
        name -> ScopeEntry(typeName, node, semanticTypeFullName = Option(semanticTypeName))
      }).toMap
    functionCaptureContext = None
    currentMethodOwnerTypeFullName = None
    currentMethodFullName = Option(info.fullName)
    currentMethodSimpleName = Option(info.name)
    currentMethodIsConst = None
    currentMethodReturnTypeFullName = Option(info.returnType)
    localDestructorScopes = Vector.empty[LocalDestructor] :: Nil
    jumpCleanupTargets = Nil
    gotoLabelCleanupDestructors = collectGotoLabelCleanupDestructors(lambda.body)
    val bodyAsts =
      try {
        lambda.body.flatMap(astsForStatement)
      } finally {
        localDestructorScopes = previousDestructorScopes
        jumpCleanupTargets = previousJumpTargets
        gotoLabelCleanupDestructors = previousGotoLabels
        currentMethodReturnTypeFullName = previousMethodReturnType
        currentMethodIsConst = previousMethodIsConst
        currentMethodSimpleName = previousMethodSimpleName
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
      methodAst(method, parameterEntries.map(_._2._3), body, methodReturn, modifiers)

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
      signature          <- lambdaSignaturesByFullName.get(typeFullName)
      returnType         <- lambdaReturnTypesByFullName.get(typeFullName)
      semanticReturnType <- lambdaSemanticReturnTypesByFullName.get(typeFullName)
    } yield LambdaInfo(
      typeFullName.split('.').lastOption.getOrElse(typeFullName).takeWhile(_ != ':'),
      typeFullName,
      signature,
      returnType,
      semanticReturnType
    )
  }

  private def booleanConversionOperatorAst(expression: OxExpression): Option[Ast] = {
    booleanConversionOperatorTarget(expression).map { target =>
      astForResolvedOperatorCall(OxOrigin(expression), s"${expression.code}.${target.name}()", target)
    }
  }

  private def booleanConversionOperatorTarget(expression: OxExpression): Option[ResolvedOperatorCall] = {
    val operatorName = "operator bool"
    selectFunctionEntry(
      memberFunctionCandidates(expression, operatorName),
      Some(Seq.empty),
      expressionTypeFullName(expression)
    )
      .filter(entry => normalizeType(entry.function.returnType) == "bool")
      .map(entry => ResolvedOperatorCall(entry, operatorName, Option(expression), Seq.empty))
  }

  private def contextualConversionOperatorTarget(
    expression: OxExpression,
    expectedTypeFullName: Option[String]
  ): Option[ResolvedOperatorCall] = {
    expectedTypeFullName.filterNot(expressionDirectlyCompatibleWithExpected(expression, _)).flatMap { expectedType =>
      val candidates = (contextualConversionObjectTypeFullName(expectedType).toSeq
        .flatMap(conversionOperatorNamesForType)
        .flatMap(operatorName => memberFunctionCandidates(expression, operatorName)) ++
        conversionOperatorCandidates(expression)).distinct
      candidates.zipWithIndex
        .flatMap { case (entry, index) =>
          conversionOperatorCompatibilityScore(expectedType, entry).map(score => (entry, score, index))
        }
        .maxByOption { case (_, score, index) => (score, index) }
        .map { case (entry, _, _) =>
          ResolvedOperatorCall(entry, entry.simpleName, Option(expression), Seq.empty)
        }
    }
  }

  private def expressionDirectlyCompatibleWithExpected(
    expression: OxExpression,
    expectedTypeFullName: String
  ): Boolean = {
    expressionTypeFullName(expression).exists { argumentType =>
      directTypeCompatibilityScore(expectedTypeFullName, argumentType, expressionIsRvalue(expression)) > 0
    }
  }

  private def contextualConversionTemporaryTypeFullName(
    expression: OxExpression,
    expectedTypeFullName: Option[String]
  ): Option[String] = {
    contextualConversionOperatorTarget(expression, expectedTypeFullName)
      .flatMap(target => conversionOperatorReturnObjectTypeFullName(target.entry))
  }

  private def contextualConversionObjectTypeFullName(typeFullName: String): Option[String] = {
    val normalized = stripCxxTypeQualifiers(stripCxxReference(normalizeType(resolveAliasType(typeFullName)))).trim
    returnedObjectTypeFullName(normalized)
  }

  private def expressionObjectTypeFullName(expression: OxExpression): Option[String] = {
    expressionTypeFullName(expression).flatMap(contextualConversionObjectTypeFullName)
  }

  private def conversionOperatorReturnObjectTypeFullName(entry: FunctionEntry): Option[String] = {
    Option(functionSemanticReturnTypeFullName(entry)).flatMap(contextualConversionObjectTypeFullName)
  }

  private def conversionOperatorCandidates(expression: OxExpression): Seq[FunctionEntry] = {
    expressionTypeFullName(expression).toSeq.flatMap { receiverTypeFullName =>
      val receiverType = receiverAggregateTypeName(receiverTypeFullName)
      val candidates = typeAndBaseTypeFullNames(receiverType)
        .flatMap(typeName => resolveAggregateTypeFullName(typeName).toSeq :+ typeName)
        .distinct
        .iterator
        .map(conversionOperatorsDeclaredForType)
        .find(_.nonEmpty)
        .getOrElse(Seq.empty)
      filterMemberFunctionCandidatesForReceiver(candidates, receiverTypeFullName)
    }
  }

  private def conversionOperatorsDeclaredForType(typeName: String): Seq[FunctionEntry] = {
    functionEntries.filter(entry => entry.ownerFullName.contains(typeName) && entry.simpleName.startsWith("operator "))
  }

  private def conversionOperatorNamesForType(typeFullName: String): Seq[String] = {
    val localName    = typeFullName.split('.').lastOption.getOrElse(typeFullName)
    val cxxQualified = typeFullName.replace(".", "::")
    Seq(localName, cxxQualified, typeFullName).map(typeName => s"operator $typeName").distinct
  }

  private def conversionOperatorCallCode(expression: OxExpression, target: ResolvedOperatorCall): String = {
    s"${expression.code}.${conversionOperatorCodeName(target.name)}()"
  }

  private def conversionOperatorCodeName(name: String): String = {
    if (name.startsWith("operator ")) s"operator ${name.stripPrefix("operator ").replace(".", "::")}" else name
  }

  private def overloadedBinaryOperatorAst(binary: OxBinary): Option[Ast] = {
    overloadedBinaryOperatorTarget(binary).map(target =>
      astForResolvedOperatorCall(OxOrigin(binary), binary.code, target)
    )
  }

  private def overloadedUnaryOperatorAst(unary: OxUnary): Option[Ast] = {
    overloadedUnaryOperatorTarget(unary).map(target => astForResolvedOperatorCall(OxOrigin(unary), unary.code, target))
  }

  private def overloadedUnaryOperatorTarget(unary: OxUnary): Option[ResolvedOperatorCall] = {
    Option
      .when(expressionHasAggregateObjectOrReferenceType(unary.argument))(unary)
      .flatMap(cxxUnaryOperatorFunctionName)
      .flatMap { operatorName =>
        if (isPostfixUnaryOperatorWithDummyParameter(unary)) {
          overloadedPostfixUnaryOperatorTarget(unary, operatorName)
        } else {
          val memberTarget =
            selectFunctionEntry(
              memberFunctionCandidates(unary.argument, operatorName),
              Some(Seq.empty),
              expressionTypeFullName(unary.argument)
            )
              .map(entry => ResolvedOperatorCall(entry, operatorName, Option(unary.argument), Seq.empty))
          memberTarget.orElse {
            selectFunctionEntry(freeFunctionCandidatesByName(operatorName), Some(Seq(unary.argument)))
              .map(entry => ResolvedOperatorCall(entry, operatorName, None, Seq(unary.argument)))
          }
        }
      }
  }

  private def overloadedPostfixUnaryOperatorTarget(
    unary: OxUnary,
    operatorName: String
  ): Option[ResolvedOperatorCall] = {
    val dummyArgument = postfixUnaryDummyArgument(unary.line)
    val memberTarget =
      selectFunctionEntry(
        memberFunctionCandidates(unary.argument, operatorName),
        Some(Seq(dummyArgument)),
        expressionTypeFullName(unary.argument)
      )
        .map(entry => ResolvedOperatorCall(entry, operatorName, Option(unary.argument), Seq.empty))
    memberTarget.orElse {
      selectFunctionEntry(freeFunctionCandidatesByName(operatorName), Some(Seq(unary.argument, dummyArgument)))
        .map(entry => ResolvedOperatorCall(entry, operatorName, None, Seq(unary.argument)))
    }
  }

  private def postfixUnaryDummyArgument(line: Int): OxExpression = {
    OxLiteral("0", "0", line)
  }

  private def isPostfixUnaryOperatorWithDummyParameter(unary: OxUnary): Boolean = {
    !unary.prefix && CxxPostfixUnaryOperatorsWithDummyParameter.contains(unary.operator)
  }

  private def overloadedBinaryOperatorTarget(binary: OxBinary): Option[ResolvedOperatorCall] = {
    cxxOperatorFunctionName(binary.operator).flatMap { operatorName =>
      val memberTarget =
        selectFunctionEntry(
          memberFunctionCandidates(binary.left, operatorName),
          Some(Seq(binary.right)),
          expressionTypeFullName(binary.left)
        )
          .map(entry => ResolvedOperatorCall(entry, operatorName, Option(binary.left), Seq(binary.right)))
      memberTarget.orElse {
        selectFunctionEntry(freeFunctionCandidatesByName(operatorName), Some(Seq(binary.left, binary.right)))
          .map(entry => ResolvedOperatorCall(entry, operatorName, None, Seq(binary.left, binary.right)))
      }
    }
  }

  private def overloadedAssignmentOperatorAst(assignment: OxAssignment): Option[Ast] = {
    overloadedAssignmentOperatorTarget(assignment).map(target =>
      astForResolvedOperatorCall(assignmentOrigin(assignment), assignment.code, target)
    )
  }

  private def assignmentOrigin(assignment: OxAssignment): OxOrigin = {
    OxOrigin(assignment.code, Option(assignment.line))
  }

  private def overloadedAssignmentOperatorTarget(assignment: OxAssignment): Option[ResolvedOperatorCall] = {
    cxxOperatorFunctionName(assignment.operator).flatMap { operatorName =>
      val memberTarget =
        selectFunctionEntry(
          memberFunctionCandidates(assignment.left, operatorName),
          Some(Seq(assignment.right)),
          expressionTypeFullName(assignment.left)
        )
          .map(entry => ResolvedOperatorCall(entry, operatorName, Option(assignment.left), Seq(assignment.right)))
      memberTarget.orElse {
        Option
          .when(assignment.operator != "=")(freeFunctionCandidatesByName(operatorName))
          .flatMap(candidates => selectFunctionEntry(candidates, Some(Seq(assignment.left, assignment.right))))
          .map(entry => ResolvedOperatorCall(entry, operatorName, None, Seq(assignment.left, assignment.right)))
      }
    }
  }

  private def overloadedIndexOperatorAst(indexAccess: OxIndexAccess): Option[Ast] = {
    overloadedIndexOperatorTarget(indexAccess).map(target =>
      astForResolvedOperatorCall(OxOrigin(indexAccess), indexAccess.code, target)
    )
  }

  private def overloadedIndexOperatorTarget(indexAccess: OxIndexAccess): Option[ResolvedOperatorCall] = {
    val operatorName = "operator[]"
    selectFunctionEntry(
      memberFunctionCandidates(indexAccess.base, operatorName),
      Some(Seq(indexAccess.index)),
      expressionTypeFullName(indexAccess.base)
    )
      .map(entry => ResolvedOperatorCall(entry, operatorName, Option(indexAccess.base), Seq(indexAccess.index)))
  }

  private def overloadedCallOperatorAst(call: OxCall): Option[Ast] = {
    overloadedCallOperatorTarget(call).map(target => astForResolvedOperatorCall(OxOrigin(call), call.code, target))
  }

  private def overloadedCallOperatorTarget(call: OxCall): Option[ResolvedOperatorCall] = {
    val operatorName = "operator()"
    selectFunctionEntry(
      memberFunctionCandidates(call.callee, operatorName),
      Some(call.arguments),
      expressionTypeFullName(call.callee)
    )
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
        Option(registerType(operatorCallReturnTypeFullName(target)))
      )
    val base = target.base.map(expressionAst)
    createCallAst(
      callNode_,
      argumentAstsForFunctionEntry(target.entry, target.arguments, receiverTypeFullName(target)),
      base = base,
      receiver = if (dispatchType == DispatchTypes.DYNAMIC_DISPATCH) base else None
    )
  }

  private def operatorCallReturnTypeFullName(target: ResolvedOperatorCall): String = {
    val semanticReturnType =
      functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
    Option.when(typeNameHasCxxQualifier(semanticReturnType))(semanticReturnType).getOrElse {
      specializeFunctionTypeName(
        normalizeType(target.entry.function.returnType),
        target.entry,
        target.arguments,
        receiverTypeFullName(target)
      )
    }
  }

  private def receiverTypeFullName(target: ResolvedOperatorCall): Option[String] = {
    target.base.flatMap(expressionTypeFullName)
  }

  private def typeNameHasCxxQualifier(typeName: String): Boolean = {
    normalizeType(typeName).split("\\s+").exists(CxxTypeQualifiers.contains)
  }

  private def memberFunctionCandidates(receiver: OxExpression, name: String): Seq[FunctionEntry] = {
    expressionTypeFullName(receiver).toSeq
      .flatMap(receiverType => memberFunctionCandidatesForReceiverType(receiverType, name))
  }

  private def memberFunctionCandidatesForReceiverType(
    receiverTypeFullName: String,
    name: String
  ): Seq[FunctionEntry] = {
    val receiverType = receiverAggregateTypeName(receiverTypeFullName)
    val candidates   = memberFunctionCandidatesForType(receiverType, name)
    filterMemberFunctionCandidatesForReceiver(candidates, receiverTypeFullName)
  }

  private def memberFunctionCandidatesForType(receiverType: String, name: String): Seq[FunctionEntry] = {
    typeAndBaseTypeFullNames(receiverType)
      .flatMap(typeName => resolveAggregateTypeFullName(typeName).toSeq :+ typeName)
      .distinct
      .iterator
      .map(typeName => memberFunctionCandidatesDeclaredOrUsing(typeName, name))
      .find(_.nonEmpty)
      .getOrElse(Seq.empty)
  }

  private def filterMemberFunctionCandidatesForReceiver(
    candidates: Seq[FunctionEntry],
    receiverTypeFullName: String
  ): Seq[FunctionEntry] = {
    if (receiverObjectTypeIsConst(receiverTypeFullName)) {
      candidates.filter(entry => entry.function.isConst || entry.function.isStatic)
    } else {
      val (constMembers, otherMembers) =
        candidates.partition(entry => entry.function.isConst && !entry.function.isStatic)
      constMembers ++ otherMembers
    }
  }

  private def receiverObjectTypeIsConst(typeName: String): Boolean = {
    val normalized          = normalizeType(resolveAliasType(typeName))
    val referencedObject    = stripCxxReference(normalized)
    val pointerObject       = if (referencedObject.endsWith("*")) referencedObject.dropRight(1) else referencedObject
    val arrayElementOrValue = pointerObject.stripSuffix("[]")
    arrayElementOrValue.split("\\s+").contains("const")
  }

  private def memberFunctionCandidatesDeclaredOrUsing(typeName: String, name: String): Seq[FunctionEntry] = {
    val declared = functionCandidatesByQualifiedName(s"$typeName.$name")
    val usingTargets = aggregateUsingDeclarationsByType
      .getOrElse(typeName, Seq.empty)
      .filter(_.name == name)
      .flatMap(usingDecl => qualifiedMemberFunctionCandidates(usingDecl.target, Option(typeName)))
    (declared ++ usingTargets).distinct
  }

  private def qualifiedMemberFunctionCandidates(name: String, receiverType: Option[String]): Seq[FunctionEntry] = {
    qualifiedMemberFunctionName(name).toSeq.flatMap { case (ownerName, simpleName) =>
      qualifiedMemberOwnerTypeFullNames(ownerName, receiverType)
        .flatMap(ownerTypeName => functionCandidatesByQualifiedName(s"$ownerTypeName.$simpleName"))
    }
  }

  private def qualifiedMemberOwnerTypeFullNames(ownerName: String, receiverType: Option[String]): Seq[String] = {
    val normalizedOwner = normalizedQualifiedName(ownerName)
    def typeHierarchy(typeName: String): Seq[String] = {
      typeAndBaseTypeFullNames(typeName)
        .flatMap(candidate => resolveAggregateTypeFullName(candidate).toSeq :+ candidate)
        .distinct
    }
    def matchesOwner(typeName: String): Boolean = {
      typeName == normalizedOwner || typeName.endsWith(s".$normalizedOwner")
    }

    val receiverCandidates = receiverType.toSeq.flatMap(typeHierarchy).filter(matchesOwner)
    val currentOwnerCandidates = currentMethodOwnerTypeFullName.toSeq
      .flatMap(typeHierarchy)
      .filter(matchesOwner)
    val globalCandidates = resolveAggregateTypeFullName(normalizedOwner).toSeq
    (receiverCandidates ++ currentOwnerCandidates ++ globalCandidates).distinct
  }

  private def qualifiedMemberFunctionName(name: String): Option[(String, String)] = {
    val parts = qualifiedNameParts(name)
    Option.when(parts.size > 1)(parts.dropRight(1).mkString(".") -> parts.last)
  }

  private def receiverAggregateTypeName(typeName: String): String = {
    stripTemplateArguments(aggregateLookupTypeName(typeName))
  }

  private def aggregateLookupTypeName(typeName: String): String = {
    val normalized          = normalizeType(typeName)
    val objectTypeName      = stripCxxReference(normalized).stripSuffix("*").stripSuffix("[]")
    val unqualifiedTypeName = stripCxxTypeQualifiers(objectTypeName).trim
    normalizeType(resolveAliasType(unqualifiedTypeName))
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

  private def templateArgumentTypeNames(typeName: String): Seq[String] = {
    templateArgumentList(typeName).map(splitTemplateArgumentList).getOrElse(Seq.empty)
  }

  private def templateArgumentList(typeName: String): Option[String] = {
    val startIndex = typeName.indexOf('<')
    Option.when(startIndex >= 0)(startIndex).flatMap { startIndex =>
      templateArgumentListEndIndex(typeName, startIndex).map(endIndex => typeName.substring(startIndex + 1, endIndex))
    }
  }

  private def templateArgumentListEndIndex(typeName: String, startIndex: Int): Option[Int] = {
    var depth = 0
    var index = startIndex
    while (index < typeName.length) {
      typeName.charAt(index) match {
        case '<' =>
          depth += 1
        case '>' =>
          depth -= 1
          if (depth == 0) {
            return Option(index)
          }
        case _ =>
      }
      index += 1
    }
    None
  }

  private def splitTemplateArgumentList(argumentList: String): Seq[String] = {
    val arguments  = mutable.ArrayBuffer.empty[String]
    val current    = new StringBuilder
    var angleDepth = 0
    var parenDepth = 0
    argumentList.foreach {
      case '<' =>
        angleDepth += 1
        current.append('<')
      case '>' if angleDepth > 0 =>
        angleDepth -= 1
        current.append('>')
      case '(' =>
        parenDepth += 1
        current.append('(')
      case ')' if parenDepth > 0 =>
        parenDepth -= 1
        current.append(')')
      case ',' if angleDepth == 0 && parenDepth == 0 =>
        arguments += normalizeType(current.toString)
        current.clear()
      case ch =>
        current.append(ch)
    }
    arguments += normalizeType(current.toString)
    arguments.toSeq.filter(_.nonEmpty)
  }

  private def stripCxxReference(typeName: String): String = {
    if (typeName.endsWith("&&")) typeName.dropRight(2)
    else if (typeName.endsWith("&")) typeName.dropRight(1)
    else typeName
  }

  private def stripCxxTypeQualifiers(typeName: String): String = {
    typeName
      .split("\\s+")
      .filterNot(CxxTypeQualifiers.contains)
      .mkString(" ")
  }

  private def cxxOperatorFunctionName(operator: String): Option[String] = {
    Option.when(CxxOverloadableBinaryOperators.contains(operator))(s"operator$operator")
  }

  private def cxxUnaryOperatorFunctionName(unary: OxUnary): Option[String] = {
    Option.when(CxxOverloadableUnaryOperators.contains(unary.operator))(s"operator${unary.operator}")
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
      targetEntryForCallArguments(call)
        .map(entry =>
          argumentAstsForFunctionEntry(
            entry,
            call.arguments,
            receiverTypeFullName(call),
            explicitTemplateArgumentTypeNames(call)
          )
        )
        .getOrElse(call.arguments.map(expressionAst)),
      base = base,
      receiver = if (dispatchType == DispatchTypes.DYNAMIC_DISPATCH) base else None
    )
  }

  private def argumentAstsForFunctionEntry(
    entry: FunctionEntry,
    arguments: Seq[OxExpression],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): Seq[Ast] = {
    val templateBindings =
      templateBindingsForFunctionCall(entry, arguments, receiverTypeFullName, explicitTemplateArguments)
    arguments.zipWithIndex.map { case (argument, index) =>
      val parameterType = entry.function.parameters
        .lift(index)
        .map(parameter => substituteTemplateTypeNames(parameter.semanticTypeName, templateBindings))
      expressionAstWithContextualConversion(argument, parameterType)
    }
  }

  private def functionCallTypeFullName(
    entry: FunctionEntry,
    arguments: Seq[OxExpression],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): String = {
    val semanticReturnType =
      functionSemanticReturnTypeFullName(entry, arguments, receiverTypeFullName, explicitTemplateArguments)
    val syntacticReturnType =
      specializeFunctionTypeName(
        normalizeType(entry.function.returnType),
        entry,
        arguments,
        receiverTypeFullName,
        explicitTemplateArguments
      )
    Option
      .when(
        isAutoType(syntacticReturnType) || isDecltypeAutoType(syntacticReturnType) ||
          typeNameHasCxxQualifier(semanticReturnType) ||
          functionTemplateParametersInType(entry, entry.function.returnType, receiverTypeFullName).nonEmpty
      )(semanticReturnType)
      .getOrElse(syntacticReturnType)
  }

  private def targetEntryForCallArguments(call: OxCall): Option[FunctionEntry] = {
    constructorTemporaryEntry(call)
      .map(_._2)
      .orElse(overloadedCallOperatorTarget(call).map(_.entry))
      .orElse(functionEntryForCall(call))
  }

  private def memberCallBaseAst(call: OxCall): Option[Ast] = {
    call.callee match {
      case OxFieldAccess(_, _, _, base) => Option(expressionAst(base))
      case _                            => None
    }
  }

  private def receiverTypeFullName(call: OxCall): Option[String] = {
    call.callee match {
      case OxFieldAccess(_, _, _, base) => expressionTypeFullName(base)
      case _                            => None
    }
  }

  private def explicitTemplateArgumentTypeNames(call: OxCall): Seq[String] = {
    val calleeName = call.callee match {
      case OxFieldAccess(field, _, _, _) => field
      case _                             => call.name
    }
    templateArgumentTypeNames(calleeName)
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
      case _: OxBinary | _: OxAssignment | _: OxConditional | _: OxFold | _: OxTypeOf | _: OxSizeOf | _: OxNew |
          _: OxDelete =>
        false
    }
  }

  private def callReturnTypeFullName(call: OxCall): Option[String] = {
    lambdaCallableInfo(call.callee)
      .map(_.semanticReturnType)
      .orElse(constructorTemporaryTypeFullName(call))
      .orElse(
        overloadedCallOperatorTarget(call)
          .map(target =>
            functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
          )
          .orElse(
            expressionTypeFullName(call.callee)
              .flatMap(returnTypeFromFunctionPointer)
              .orElse(
                functionEntryForCall(call)
                  .map(entry =>
                    functionSemanticReturnTypeFullName(
                      entry,
                      call.arguments,
                      receiverTypeFullName(call),
                      explicitTemplateArgumentTypeNames(call)
                    )
                  )
              )
          )
      )
  }

  private def functionSemanticReturnTypeFullName(entry: FunctionEntry): String = {
    ownerResolvedTypeFullNamePreservingCv(
      entry.function.semanticReturnType,
      entry.ownerFullName.orElse(entry.lexicalOwnerFullName)
    )
  }

  private def functionSemanticReturnTypeFullName(
    entry: FunctionEntry,
    arguments: Seq[OxExpression],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): String = {
    val argumentInfos =
      arguments.map(argument => ArgumentInfo(argument, expressionTypeFullName(argument), isRvalue = false))
    functionSemanticReturnTypeFullNameForArgumentInfos(
      entry,
      argumentInfos,
      receiverTypeFullName,
      explicitTemplateArguments
    )
  }

  private def functionSemanticReturnTypeFullNameForArgumentInfos(
    entry: FunctionEntry,
    argumentInfos: Seq[ArgumentInfo],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): String = {
    val templateBindings =
      templateBindingsForFunctionArgumentInfos(entry, argumentInfos, receiverTypeFullName, explicitTemplateArguments)
    val specializedReturnType = substituteTemplateTypeNames(functionSemanticReturnTypeFullName(entry), templateBindings)
    if (isAutoType(specializedReturnType)) {
      functionAutoReturnTypeFullName(entry, specializedReturnType, templateBindings).getOrElse(specializedReturnType)
    } else if (isDecltypeAutoType(specializedReturnType)) {
      functionDecltypeAutoReturnTypeFullName(entry, templateBindings).getOrElse(specializedReturnType)
    } else {
      specializedReturnType
    }
  }

  private def functionAutoReturnTypeFullName(
    entry: FunctionEntry,
    explicitReturnType: String,
    templateBindings: Map[String, String]
  ): Option[String] = {
    val inferenceKey = autoReturnInferenceKey(entry, templateBindings)
    if (!autoReturnInferenceStack.add(inferenceKey)) {
      None
    } else {
      try {
        val returnExpressions = functionReturnExpressions(entry, templateBindings)
        val inferredReturnTypes = returnExpressions.map { expression =>
          functionReturnExpressionTypeFullName(entry, expression.expression, templateBindings, expression.localTypes)
            .flatMap(typeName => inferredAutoTypeFullName(explicitReturnType, typeName, preserveCv = true))
        }
        Option
          .when(returnExpressions.nonEmpty && inferredReturnTypes.forall(_.isDefined)) {
            inferredReturnTypes.flatten.map(normalizeType).distinct
          }
          .collect { case Seq(returnType) => returnType }
      } finally {
        autoReturnInferenceStack.remove(inferenceKey)
      }
    }
  }

  private def autoReturnInferenceKey(entry: FunctionEntry, templateBindings: Map[String, String]): String = {
    val bindingKey =
      templateBindings.toSeq.sortBy(_._1).map { case (name, typeName) => s"$name=$typeName" }.mkString(",")
    s"${entry.fullName}:$bindingKey"
  }

  private def functionDecltypeAutoReturnTypeFullName(
    entry: FunctionEntry,
    templateBindings: Map[String, String]
  ): Option[String] = {
    val inferenceKey = autoReturnInferenceKey(entry, templateBindings)
    if (!autoReturnInferenceStack.add(inferenceKey)) {
      None
    } else {
      try {
        val returnExpressions = functionReturnExpressions(entry, templateBindings)
        val inferredReturnTypes = returnExpressions.map { expression =>
          functionDecltypeAutoReturnExpressionTypeFullName(
            entry,
            expression.expression,
            expression.returnCode,
            templateBindings,
            expression.localTypes
          )
        }
        Option
          .when(returnExpressions.nonEmpty && inferredReturnTypes.forall(_.isDefined)) {
            inferredReturnTypes.flatten.map(normalizeType).distinct
          }
          .collect { case Seq(returnType) => returnType }
      } finally {
        autoReturnInferenceStack.remove(inferenceKey)
      }
    }
  }

  private def functionReturnExpressions(
    entry: FunctionEntry,
    templateBindings: Map[String, String]
  ): Seq[FunctionReturnExpression] = {
    returnExpressionsInStatements(entry, templateBindings, entry.function.body, Map.empty)._2
  }

  private def returnExpressionsInStatements(
    entry: FunctionEntry,
    templateBindings: Map[String, String],
    statements: Seq[OxStatement],
    localTypes: Map[String, String]
  ): (Map[String, String], Seq[FunctionReturnExpression]) = {
    statements.foldLeft(localTypes -> Seq.empty[FunctionReturnExpression]) {
      case ((currentLocalTypes, returnExpressions), statement) =>
        val (nextLocalTypes, statementReturnExpressions) =
          returnExpressionsInStatement(entry, templateBindings, statement, currentLocalTypes)
        nextLocalTypes -> (returnExpressions ++ statementReturnExpressions)
    }
  }

  private def returnExpressionsInStatement(
    entry: FunctionEntry,
    templateBindings: Map[String, String],
    statement: OxStatement,
    localTypes: Map[String, String]
  ): (Map[String, String], Seq[FunctionReturnExpression]) = {
    statement match {
      case local: OxLocalDecl =>
        localTypes.updated(local.name, functionLocalTypeFullName(entry, local, templateBindings, localTypes)) ->
          Seq.empty
      case OxReturn(code, _, Some(expression)) =>
        localTypes -> Seq(FunctionReturnExpression(expression, localTypes, code))
      case OxTry(_, _, body, catches) =>
        val bodyReturns = returnExpressionsInStatements(entry, templateBindings, body, localTypes)._2
        val catchReturns = catches.flatMap(catchClause =>
          returnExpressionsInStatements(entry, templateBindings, catchClause.body, localTypes)._2
        )
        localTypes -> (bodyReturns ++ catchReturns)
      case OxIf(_, _, initializer, conditionInitializer, _, thenBody, elseBody) =>
        val (initializerLocalTypes, initializerReturns) =
          returnExpressionsInStatements(entry, templateBindings, initializer, localTypes)
        val (conditionLocalTypes, conditionReturns) =
          returnExpressionsInStatements(entry, templateBindings, conditionInitializer, initializerLocalTypes)
        val thenReturns = returnExpressionsInStatements(entry, templateBindings, thenBody, conditionLocalTypes)._2
        val elseReturns = returnExpressionsInStatements(entry, templateBindings, elseBody, conditionLocalTypes)._2
        localTypes -> (initializerReturns ++ conditionReturns ++ thenReturns ++ elseReturns)
      case OxWhile(_, _, initializer, conditionInitializer, _, body) =>
        val (initializerLocalTypes, initializerReturns) =
          returnExpressionsInStatements(entry, templateBindings, initializer, localTypes)
        val (conditionLocalTypes, conditionReturns) =
          returnExpressionsInStatements(entry, templateBindings, conditionInitializer, initializerLocalTypes)
        val bodyReturns = returnExpressionsInStatements(entry, templateBindings, body, conditionLocalTypes)._2
        localTypes -> (initializerReturns ++ conditionReturns ++ bodyReturns)
      case OxDoWhile(_, _, _, body) =>
        localTypes -> returnExpressionsInStatements(entry, templateBindings, body, localTypes)._2
      case OxFor(_, _, initializer, _, _, body) =>
        val (initializerLocalTypes, initializerReturns) =
          returnExpressionsInStatements(entry, templateBindings, initializer, localTypes)
        val bodyReturns = returnExpressionsInStatements(entry, templateBindings, body, initializerLocalTypes)._2
        localTypes -> (initializerReturns ++ bodyReturns)
      case OxLabel(_, _, _, body) =>
        returnExpressionsInStatements(entry, templateBindings, body, localTypes)
      case OxSwitch(_, _, initializer, conditionInitializer, _, body) =>
        val (initializerLocalTypes, initializerReturns) =
          returnExpressionsInStatements(entry, templateBindings, initializer, localTypes)
        val (conditionLocalTypes, conditionReturns) =
          returnExpressionsInStatements(entry, templateBindings, conditionInitializer, initializerLocalTypes)
        val bodyReturns = returnExpressionsInStatements(entry, templateBindings, body, conditionLocalTypes)._2
        localTypes -> (initializerReturns ++ conditionReturns ++ bodyReturns)
      case OxCase(_, _, _, body) =>
        returnExpressionsInStatements(entry, templateBindings, body, localTypes)
      case _ =>
        localTypes -> Seq.empty
    }
  }

  private def functionLocalTypeFullName(
    entry: FunctionEntry,
    local: OxLocalDecl,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): String = {
    val explicitType =
      functionScopedTypeFullName(
        entry,
        typeFullNameWithStringLiteralLength(local.semanticTypeName, local.initializer),
        templateBindings
      )
    Option
      .when(isAutoType(explicitType))(local.initializer)
      .flatten
      .flatMap(expression => functionReturnExpressionTypeFullName(entry, expression, templateBindings, localTypes))
      .flatMap(typeName => inferredAutoTypeFullName(explicitType, typeName, preserveCv = true))
      .getOrElse(explicitType)
  }

  private def functionReturnExpressionTypeFullName(
    entry: FunctionEntry,
    expression: OxExpression,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    expression match {
      case OxIdentifier(name, _, _) =>
        localTypes
          .get(name)
          .orElse(functionParameterTypeFullName(entry, name, templateBindings))
          .orElse(functionThisTypeFullName(entry).filter(_ => name == Defines.This))
      case OxLiteral(value, _, _) =>
        Option(literalType(value))
      case OxCast(_, semanticTypeName, _, _, _) =>
        Option(functionScopedTypeFullName(entry, semanticTypeName, templateBindings))
      case OxFieldAccess(field, _, _, base) =>
        functionReturnExpressionTypeFullName(entry, base, templateBindings, localTypes).flatMap(
          fieldTypeFullName(_, field)
        )
      case OxUnary("*", _, _, _, argument) =>
        functionReturnExpressionTypeFullName(entry, argument, templateBindings, localTypes).map(
          dereferencedTypeFullName
        )
      case OxUnary("&", _, _, _, argument) =>
        functionReturnExpressionTypeFullName(entry, argument, templateBindings, localTypes).map { typeName =>
          s"${stripCxxReference(normalizeType(resolveAliasType(typeName)))}*"
        }
      case assignment: OxAssignment =>
        functionScopedAssignmentExpressionTypeFullName(entry, assignment, templateBindings, localTypes)
      case unary: OxUnary =>
        functionScopedUnaryExpressionTypeFullName(entry, unary, templateBindings, localTypes)
      case binary: OxBinary =>
        functionScopedBinaryExpressionTypeFullName(entry, binary, templateBindings, localTypes)
      case OxConditional(_, _, _, Some(consequence), alternative) =>
        val branchTypes = Seq(consequence, alternative).map { branch =>
          functionReturnExpressionTypeFullName(entry, branch, templateBindings, localTypes)
        }
        Option.when(branchTypes.forall(_.isDefined))(branchTypes.flatten.map(normalizeType).distinct).collect {
          case Seq(branchType) => branchType
        }
      case indexAccess: OxIndexAccess =>
        functionScopedIndexAccessTypeFullName(entry, indexAccess, templateBindings, localTypes)
      case initializerList: OxInitializerList =>
        functionScopedInitializerListTypeFullName(entry, initializerList, templateBindings, localTypes)
      case call: OxCall =>
        functionScopedCallReturnTypeFullName(entry, call, templateBindings, localTypes)
      case _ =>
        None
    }
  }

  private def functionDecltypeAutoReturnExpressionTypeFullName(
    entry: FunctionEntry,
    expression: OxExpression,
    returnCode: String,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    functionReturnExpressionTypeFullName(entry, expression, templateBindings, localTypes).map { typeName =>
      val normalizedType = normalizeType(typeName)
      if (
        returnExpressionCodeHasOuterParentheses(returnCode) &&
        !functionReturnExpressionIsRvalue(entry, expression, templateBindings, localTypes) &&
        !normalizedType.endsWith("&")
      ) {
        s"$normalizedType&"
      } else {
        normalizedType
      }
    }
  }

  private def returnExpressionCodeHasOuterParentheses(returnCode: String): Boolean = {
    val returnedCode = returnCode.trim.stripPrefix("return").stripSuffix(";").trim
    expressionCodeHasOuterParentheses(returnedCode)
  }

  private def expressionCodeHasOuterParentheses(code: String): Boolean = {
    val trimmed = code.trim
    trimmed.startsWith("(") && trimmed.endsWith(")") && matchingOuterParenthesisEnd(trimmed).contains(
      trimmed.length - 1
    )
  }

  private def matchingOuterParenthesisEnd(code: String): Option[Int] = {
    if (!code.startsWith("(")) {
      None
    } else {
      var depth = 0
      var index = 0
      while (index < code.length) {
        code.charAt(index) match {
          case '(' =>
            depth += 1
          case ')' =>
            depth -= 1
            if (depth == 0) {
              return Option(index)
            }
          case _ =>
        }
        index += 1
      }
      None
    }
  }

  private def functionScopedIndexAccessTypeFullName(
    entry: FunctionEntry,
    indexAccess: OxIndexAccess,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    val baseTypeFullName =
      functionReturnExpressionTypeFullName(entry, indexAccess.base, templateBindings, localTypes)
    val argumentInfos = Seq(
      ArgumentInfo(
        indexAccess.index,
        functionReturnExpressionTypeFullName(entry, indexAccess.index, templateBindings, localTypes),
        functionReturnExpressionIsRvalue(entry, indexAccess.index, templateBindings, localTypes)
      )
    )
    baseTypeFullName
      .flatMap { receiverTypeFullName =>
        selectFunctionEntryForArgumentInfos(
          memberFunctionCandidatesForReceiverType(receiverTypeFullName, "operator[]"),
          argumentInfos,
          Option(receiverTypeFullName)
        ).map { targetEntry =>
          functionSemanticReturnTypeFullNameForArgumentInfos(targetEntry, argumentInfos, Option(receiverTypeFullName))
        }
      }
      .orElse(baseTypeFullName.map(_.stripSuffix("[]")))
  }

  private def functionScopedInitializerListTypeFullName(
    entry: FunctionEntry,
    initializerList: OxInitializerList,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    functionScopedInitializerListElementTypeFullName(entry, initializerList, templateBindings, localTypes)
      .map(typeName => s"std.initializer_list<$typeName>")
  }

  private def functionScopedInitializerListElementTypeFullName(
    entry: FunctionEntry,
    initializerList: OxInitializerList,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    val elementTypes = initializerList.elements.map { element =>
      functionReturnExpressionTypeFullName(entry, element, templateBindings, localTypes)
    }
    Option
      .when(elementTypes.nonEmpty && elementTypes.forall(_.isDefined)) {
        elementTypes.flatten.map(normalizeType).distinct
      }
      .collect { case Seq(typeName) => typeName }
  }

  private def functionScopedAssignmentExpressionTypeFullName(
    entry: FunctionEntry,
    assignment: OxAssignment,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    functionScopedAssignmentOperatorTypeFullName(entry, assignment, templateBindings, localTypes)
      .orElse(functionReturnExpressionTypeFullName(entry, assignment.left, templateBindings, localTypes))
  }

  private def functionScopedAssignmentOperatorTypeFullName(
    entry: FunctionEntry,
    assignment: OxAssignment,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    cxxOperatorFunctionName(assignment.operator).flatMap { operatorName =>
      val leftInfo = ArgumentInfo(
        assignment.left,
        functionReturnExpressionTypeFullName(entry, assignment.left, templateBindings, localTypes),
        functionReturnExpressionIsRvalue(entry, assignment.left, templateBindings, localTypes)
      )
      val rightInfo = ArgumentInfo(
        assignment.right,
        functionReturnExpressionTypeFullName(entry, assignment.right, templateBindings, localTypes),
        functionReturnExpressionIsRvalue(entry, assignment.right, templateBindings, localTypes)
      )
      val memberTarget =
        leftInfo.typeFullName.flatMap { receiverTypeFullName =>
          selectFunctionEntryForArgumentInfos(
            memberFunctionCandidatesForReceiverType(receiverTypeFullName, operatorName),
            Seq(rightInfo),
            Option(receiverTypeFullName)
          ).map(targetEntry =>
            functionSemanticReturnTypeFullNameForArgumentInfos(
              targetEntry,
              Seq(rightInfo),
              Option(receiverTypeFullName)
            )
          )
        }
      memberTarget.orElse {
        Option
          .when(assignment.operator != "=") {
            val argumentInfos = Seq(leftInfo, rightInfo)
            selectFunctionEntryForArgumentInfos(freeFunctionCandidatesByName(operatorName), argumentInfos)
              .map(targetEntry => functionSemanticReturnTypeFullNameForArgumentInfos(targetEntry, argumentInfos))
          }
          .flatten
      }
    }
  }

  private def functionScopedUnaryExpressionTypeFullName(
    entry: FunctionEntry,
    unary: OxUnary,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    functionScopedUnaryOperatorTypeFullName(entry, unary, templateBindings, localTypes)
      .orElse(functionScopedBuiltinUnaryExpressionTypeFullName(entry, unary, templateBindings, localTypes))
  }

  private def functionScopedUnaryOperatorTypeFullName(
    entry: FunctionEntry,
    unary: OxUnary,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    cxxUnaryOperatorFunctionName(unary).flatMap { operatorName =>
      val argumentInfo = ArgumentInfo(
        unary.argument,
        functionReturnExpressionTypeFullName(entry, unary.argument, templateBindings, localTypes),
        functionReturnExpressionIsRvalue(entry, unary.argument, templateBindings, localTypes)
      )
      val dummyArgumentInfo =
        Option.when(isPostfixUnaryOperatorWithDummyParameter(unary)) {
          val dummyArgument = postfixUnaryDummyArgument(unary.line)
          ArgumentInfo(dummyArgument, Option(literalType("0")), isRvalue = true)
        }
      val memberArgumentInfos = dummyArgumentInfo.toSeq
      val memberTarget =
        argumentInfo.typeFullName.flatMap { receiverTypeFullName =>
          selectFunctionEntryForArgumentInfos(
            memberFunctionCandidatesForReceiverType(receiverTypeFullName, operatorName),
            memberArgumentInfos,
            Option(receiverTypeFullName)
          ).map(targetEntry =>
            functionSemanticReturnTypeFullNameForArgumentInfos(
              targetEntry,
              memberArgumentInfos,
              Option(receiverTypeFullName)
            )
          )
        }
      memberTarget.orElse {
        val freeArgumentInfos = argumentInfo +: dummyArgumentInfo.toSeq
        selectFunctionEntryForArgumentInfos(freeFunctionCandidatesByName(operatorName), freeArgumentInfos)
          .map(targetEntry => functionSemanticReturnTypeFullNameForArgumentInfos(targetEntry, freeArgumentInfos))
      }
    }
  }

  private def functionScopedBuiltinUnaryExpressionTypeFullName(
    entry: FunctionEntry,
    unary: OxUnary,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    unary.operator match {
      case "*" =>
        functionReturnExpressionTypeFullName(entry, unary.argument, templateBindings, localTypes).map(
          dereferencedTypeFullName
        )
      case "&" =>
        functionReturnExpressionTypeFullName(entry, unary.argument, templateBindings, localTypes).map { typeName =>
          s"${stripCxxReference(normalizeType(resolveAliasType(typeName)))}*"
        }
      case "!" | "not" =>
        Option(registerType("bool"))
      case "+" | "-" | "~" =>
        functionReturnExpressionTypeFullName(entry, unary.argument, templateBindings, localTypes)
          .map(arithmeticUnaryResultTypeFullName)
      case _ =>
        None
    }
  }

  private def arithmeticUnaryResultTypeFullName(typeName: String): String = {
    val normalized = stripCxxTypeQualifiers(stripCxxReference(normalizeType(resolveAliasType(typeName)))).trim
    val canonical  = canonicalArithmeticType(normalized)
    if (CxxIntegralPromotionSources.contains(canonical)) "int" else normalized
  }

  private def functionScopedBinaryExpressionTypeFullName(
    entry: FunctionEntry,
    binary: OxBinary,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    functionScopedBinaryOperatorTypeFullName(entry, binary, templateBindings, localTypes)
      .orElse {
        binaryExpressionTypeFullName(
          functionReturnExpressionTypeFullName(entry, binary.left, templateBindings, localTypes),
          functionReturnExpressionTypeFullName(entry, binary.right, templateBindings, localTypes)
        )
      }
  }

  private def functionScopedBinaryOperatorTypeFullName(
    entry: FunctionEntry,
    binary: OxBinary,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    cxxOperatorFunctionName(binary.operator).flatMap { operatorName =>
      val leftInfo = ArgumentInfo(
        binary.left,
        functionReturnExpressionTypeFullName(entry, binary.left, templateBindings, localTypes),
        functionReturnExpressionIsRvalue(entry, binary.left, templateBindings, localTypes)
      )
      val rightInfo = ArgumentInfo(
        binary.right,
        functionReturnExpressionTypeFullName(entry, binary.right, templateBindings, localTypes),
        functionReturnExpressionIsRvalue(entry, binary.right, templateBindings, localTypes)
      )
      val memberTarget =
        leftInfo.typeFullName.flatMap { receiverTypeFullName =>
          selectFunctionEntryForArgumentInfos(
            memberFunctionCandidatesForReceiverType(receiverTypeFullName, operatorName),
            Seq(rightInfo),
            Option(receiverTypeFullName)
          ).map(targetEntry =>
            functionSemanticReturnTypeFullNameForArgumentInfos(
              targetEntry,
              Seq(rightInfo),
              Option(receiverTypeFullName)
            )
          )
        }
      memberTarget.orElse {
        val argumentInfos = Seq(leftInfo, rightInfo)
        selectFunctionEntryForArgumentInfos(freeFunctionCandidatesByName(operatorName), argumentInfos)
          .map(targetEntry => functionSemanticReturnTypeFullNameForArgumentInfos(targetEntry, argumentInfos))
      }
    }
  }

  private def functionScopedCallReturnTypeFullName(
    entry: FunctionEntry,
    call: OxCall,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Option[String] = {
    val argumentInfos = call.arguments.map { argument =>
      ArgumentInfo(
        argument,
        functionReturnExpressionTypeFullName(entry, argument, templateBindings, localTypes),
        functionReturnExpressionIsRvalue(entry, argument, templateBindings, localTypes)
      )
    }
    val explicitTemplateArguments = explicitTemplateArgumentTypeNames(call)
    functionEntryForScopedCall(entry, call, templateBindings, localTypes, argumentInfos, explicitTemplateArguments)
      .map { case (targetEntry, receiverTypeFullName) =>
        functionSemanticReturnTypeFullNameForArgumentInfos(
          targetEntry,
          argumentInfos,
          receiverTypeFullName,
          explicitTemplateArguments
        )
      }
      .orElse(macroForUse(call.name, call.line).map(macroReturnTypeFullName))
  }

  private def functionEntryForScopedCall(
    entry: FunctionEntry,
    call: OxCall,
    templateBindings: Map[String, String],
    localTypes: Map[String, String],
    argumentInfos: Seq[ArgumentInfo],
    explicitTemplateArguments: Seq[String]
  ): Option[(FunctionEntry, Option[String])] = {
    call.callee match {
      case OxFieldAccess(field, _, _, base) =>
        val receiverTypeFullName =
          functionReturnExpressionTypeFullName(entry, base, templateBindings, localTypes)
        val receiverType              = receiverTypeFullName.map(receiverAggregateTypeName)
        val qualifiedMemberCandidates = qualifiedMemberFunctionCandidates(field, receiverType)
        val unqualifiedMemberCandidates = receiverTypeFullName.toSeq.flatMap { receiverType =>
          memberFunctionCandidatesForReceiverType(receiverType, field)
        }
        val unfilteredCandidates =
          if (qualifiedMemberCandidates.nonEmpty) qualifiedMemberCandidates else unqualifiedMemberCandidates
        val candidates = receiverTypeFullName
          .map(receiverType => filterMemberFunctionCandidatesForReceiver(unfilteredCandidates, receiverType))
          .getOrElse(unfilteredCandidates)
        selectFunctionEntryForArgumentInfos(candidates, argumentInfos, receiverTypeFullName, explicitTemplateArguments)
          .map(_ -> receiverTypeFullName)
      case _ =>
        val lookupName                = stripTemplateArguments(call.name)
        val qualifiedName             = normalizedQualifiedName(lookupName)
        val qualifiedMemberCandidates = qualifiedMemberFunctionCandidates(lookupName, None)
        val candidates =
          if (qualifiedMemberCandidates.nonEmpty) {
            qualifiedMemberCandidates
          } else if (qualifiedNameParts(call.name).size > 1) {
            val qualifiedCandidates = functionCandidatesByQualifiedName(qualifiedName)
            if (qualifiedCandidates.nonEmpty) qualifiedCandidates else functionCandidatesByName(lookupName)
          } else {
            val ownerCandidates     = scopedCurrentOwnerFunctionCandidates(entry, lookupName)
            val qualifiedCandidates = functionCandidatesByQualifiedName(qualifiedName)
            if (ownerCandidates.nonEmpty) ownerCandidates
            else if (qualifiedCandidates.nonEmpty) qualifiedCandidates
            else functionCandidatesByName(lookupName)
          }
        selectFunctionEntryForArgumentInfos(
          candidates,
          argumentInfos,
          explicitTemplateArguments = explicitTemplateArguments
        ).map(_ -> None)
    }
  }

  private def scopedCurrentOwnerFunctionCandidates(entry: FunctionEntry, name: String): Seq[FunctionEntry] = {
    entry.ownerFullName
      .filter(aggregateTypeFullNames.contains)
      .toSeq
      .flatMap { ownerTypeFullName =>
        val receiverType =
          if (entry.function.isConst) s"const $ownerTypeFullName*" else s"$ownerTypeFullName*"
        memberFunctionCandidatesForReceiverType(receiverType, name)
      }
  }

  private def functionReturnExpressionIsRvalue(
    entry: FunctionEntry,
    expression: OxExpression,
    templateBindings: Map[String, String],
    localTypes: Map[String, String]
  ): Boolean = {
    expression match {
      case OxIdentifier(name, _, _) =>
        !(localTypes.contains(name) || functionParameterTypeFullName(entry, name, templateBindings).isDefined)
      case _: OxFieldAccess =>
        false
      case OxUnary("*", _, _, _, _) =>
        false
      case OxCast(_, semanticTypeName, _, _, _) =>
        typeNameIsRvalue(functionScopedTypeFullName(entry, semanticTypeName, templateBindings))
      case indexAccess: OxIndexAccess =>
        functionScopedIndexAccessTypeFullName(entry, indexAccess, templateBindings, localTypes)
          .map(typeNameIsRvalue)
          .getOrElse(false)
      case binary: OxBinary =>
        functionScopedBinaryExpressionTypeFullName(entry, binary, templateBindings, localTypes)
          .map(typeNameIsRvalue)
          .getOrElse(true)
      case assignment: OxAssignment =>
        functionScopedAssignmentOperatorTypeFullName(entry, assignment, templateBindings, localTypes)
          .map(typeNameIsRvalue)
          .getOrElse(false)
      case unary: OxUnary =>
        functionScopedUnaryExpressionTypeFullName(entry, unary, templateBindings, localTypes)
          .map(typeNameIsRvalue)
          .getOrElse(true)
      case call: OxCall =>
        functionScopedCallReturnTypeFullName(entry, call, templateBindings, localTypes)
          .map(typeNameIsRvalue)
          .getOrElse(true)
      case OxConditional(_, _, _, Some(consequence), alternative) =>
        Seq(consequence, alternative).forall { branch =>
          functionReturnExpressionIsRvalue(entry, branch, templateBindings, localTypes)
        }
      case _ =>
        true
    }
  }

  private def functionParameterTypeFullName(
    entry: FunctionEntry,
    name: String,
    templateBindings: Map[String, String]
  ): Option[String] = {
    entry.function.parameters
      .find(_.name == name)
      .map(parameter => functionScopedTypeFullName(entry, parameter.semanticTypeName, templateBindings))
  }

  private def functionThisTypeFullName(entry: FunctionEntry): Option[String] = {
    entry.ownerFullName
      .filter(aggregateTypeFullNames.contains)
      .map(ownerTypeFullName => if (entry.function.isConst) s"const $ownerTypeFullName*" else s"$ownerTypeFullName*")
  }

  private def functionScopedTypeFullName(
    entry: FunctionEntry,
    typeName: String,
    templateBindings: Map[String, String]
  ): String = {
    val scopedTypeName =
      ownerResolvedTypeFullNamePreservingCv(typeName, entry.ownerFullName.orElse(entry.lexicalOwnerFullName))
    resolveAliasType(substituteTemplateTypeNames(scopedTypeName, templateBindings))
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
          .map(entry => resolveAliasType(entry.expressionTypeFullName))
          .orElse(staticFieldTypeFullName(name))
          .orElse(implicitFieldTypeFullName(name))
          .orElse(globalScopeByName.get(name).map(entry => resolveAliasType(entry.expressionTypeFullName)))
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
      case unary: OxUnary =>
        overloadedUnaryOperatorTarget(unary)
          .map(target =>
            functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
          )
          .orElse(unaryExpressionTypeFullName(unary))
      case OxCast(_, semanticTypeName, _, _, _) =>
        Option(resolveAliasType(semanticTypeName))
      case OxNew(typeName, _, _, _, _) =>
        Option(s"${normalizeType(resolveAliasType(typeName))}*")
      case lambda: OxLambda =>
        Option(lambdaInfo(lambda).fullName)
      case indexAccess: OxIndexAccess =>
        overloadedIndexOperatorTarget(indexAccess)
          .map(target =>
            functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
          )
          .orElse(expressionTypeFullName(indexAccess.base).map(_.stripSuffix("[]")))
      case initializerList: OxInitializerList =>
        initializerListTypeFullName(initializerList)
      case call: OxCall =>
        callReturnTypeFullName(call)
      case binary: OxBinary =>
        overloadedBinaryOperatorTarget(binary)
          .map(target =>
            functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
          )
          .orElse(binaryExpressionTypeFullName(binary))
      case assignment: OxAssignment =>
        assignmentExpressionTypeFullName(assignment)
      case _ =>
        None
    }
  }

  private def assignmentExpressionTypeFullName(assignment: OxAssignment): Option[String] = {
    overloadedAssignmentOperatorTarget(assignment)
      .map(target => functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target)))
      .orElse(expressionTypeFullName(assignment.left))
  }

  private def binaryExpressionTypeFullName(binary: OxBinary): Option[String] = {
    binaryExpressionTypeFullName(expressionTypeFullName(binary.left), expressionTypeFullName(binary.right))
  }

  private def binaryExpressionTypeFullName(leftType: Option[String], rightType: Option[String]): Option[String] = {
    (leftType, rightType) match {
      case (Some(left), Some(right)) if left == right => Some(left)
      case (Some("int"), _)                           => Some("int")
      case (_, Some("int"))                           => Some("int")
      case _                                          => None
    }
  }

  private def unaryExpressionTypeFullName(unary: OxUnary): Option[String] = {
    unary match {
      case OxUnary("*", _, _, _, argument) =>
        expressionTypeFullName(argument).map(dereferencedTypeFullName)
      case OxUnary("&", _, _, _, argument) =>
        expressionTypeFullName(argument).map(typeName =>
          s"${stripCxxReference(normalizeType(resolveAliasType(typeName)))}*"
        )
      case _ =>
        None
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
    fieldEntryForTypeHierarchy(baseTypeFullName, field).map { case (ownerTypeFullName, fieldDecl) =>
      substituteTemplateTypeNames(
        fieldSemanticTypeFullName(fieldDecl),
        receiverTemplateBindingsForOwner(baseTypeFullName, ownerTypeFullName)
      )
    }
  }

  private def fieldSemanticTypeFullName(field: OxFieldDecl): String = {
    resolveAliasType(field.semanticTypeName)
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
    staticFieldTarget(name).map { case (_, field) => fieldSemanticTypeFullName(field) }
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
    fieldTypeFullName: String,
    isIndirect: Option[Boolean] = None
  ): Ast = {
    val operatorName =
      if (isIndirect.getOrElse(code.contains("->"))) Operators.indirectFieldAccess else Operators.fieldAccess
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
        registerType(fieldSemanticTypeFullName(field))
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
                  if (isVirtualFunctionEntry(functionEntry) && !isExplicitQualifiedMemberCall(call, functionEntry)) {
                    DispatchTypes.DYNAMIC_DISPATCH
                  } else DispatchTypes.STATIC_DISPATCH
                (
                  callName(call),
                  functionEntry.fullName,
                  Option(functionEntry.function.signature),
                  functionCallTypeFullName(
                    functionEntry,
                    call.arguments,
                    receiverTypeFullName(call),
                    explicitTemplateArgumentTypeNames(call)
                  ),
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

  private def isExplicitQualifiedMemberCall(call: OxCall, entry: FunctionEntry): Boolean = {
    entry.ownerFullName.exists(aggregateTypeFullNames.contains) &&
    explicitQualifiedMemberName(call).isDefined
  }

  private def explicitQualifiedMemberName(call: OxCall): Option[String] = {
    call.callee match {
      case OxFieldAccess(field, _, _, _) => qualifiedMemberFunctionName(field).map(_._1)
      case _                             => qualifiedMemberFunctionName(stripTemplateArguments(call.name)).map(_._1)
    }
  }

  private def functionEntryForCall(call: OxCall): Option[FunctionEntry] = {
    val explicitTemplateArguments = explicitTemplateArgumentTypeNames(call)
    call.callee match {
      case OxFieldAccess(field, _, _, base) =>
        val receiverTypeFullName      = expressionTypeFullName(base)
        val receiverType              = receiverTypeFullName.map(receiverAggregateTypeName)
        val qualifiedMemberCandidates = qualifiedMemberFunctionCandidates(field, receiverType)
        val unqualifiedMemberCandidates = receiverTypeFullName.toSeq.flatMap { receiverType =>
          memberFunctionCandidatesForReceiverType(receiverType, field)
        }
        val unfilteredCandidates =
          if (qualifiedMemberCandidates.nonEmpty) qualifiedMemberCandidates else unqualifiedMemberCandidates
        val candidates = receiverTypeFullName
          .map(receiverType => filterMemberFunctionCandidatesForReceiver(unfilteredCandidates, receiverType))
          .getOrElse(unfilteredCandidates)
        selectFunctionEntry(candidates, Some(call.arguments), receiverTypeFullName, explicitTemplateArguments)
      case _ =>
        val lookupName                = stripTemplateArguments(call.name)
        val qualifiedName             = normalizedQualifiedName(lookupName)
        val qualifiedMemberCandidates = qualifiedMemberFunctionCandidates(lookupName, None)
        if (qualifiedMemberCandidates.nonEmpty) {
          selectFunctionEntry(
            qualifiedMemberCandidates,
            Some(call.arguments),
            explicitTemplateArguments = explicitTemplateArguments
          )
        } else if (qualifiedNameParts(call.name).size > 1) {
          val candidates = functionCandidatesByQualifiedName(qualifiedName)
          selectFunctionEntry(
            if (candidates.nonEmpty) candidates else functionCandidatesByName(lookupName),
            Some(call.arguments),
            explicitTemplateArguments = explicitTemplateArguments
          )
        } else {
          val ownerCandidates     = currentOwnerFunctionCandidates(lookupName)
          val qualifiedCandidates = functionCandidatesByQualifiedName(qualifiedName)
          val candidates =
            if (ownerCandidates.nonEmpty) ownerCandidates
            else if (qualifiedCandidates.nonEmpty) qualifiedCandidates
            else functionCandidatesByName(lookupName)
          selectFunctionEntry(candidates, Some(call.arguments), explicitTemplateArguments = explicitTemplateArguments)
        }
    }
  }

  private def currentOwnerFunctionCandidates(name: String): Seq[FunctionEntry] = {
    currentMethodOwnerTypeFullName.toSeq.flatMap { ownerTypeFullName =>
      val receiverType =
        if (currentMethodIsConst.contains(true)) s"const $ownerTypeFullName*" else s"$ownerTypeFullName*"
      memberFunctionCandidatesForReceiverType(receiverType, name)
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
    arguments: Option[Seq[OxExpression]],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): Option[FunctionEntry] = {
    arguments match {
      case Some(arguments) =>
        val argumentInfos =
          arguments.map(argument =>
            ArgumentInfo(argument, expressionTypeFullName(argument), expressionIsRvalue(argument))
          )
        selectFunctionEntryForArgumentInfos(candidates, argumentInfos, receiverTypeFullName, explicitTemplateArguments)
      case None =>
        candidates.lastOption
    }
  }

  private def selectFunctionEntryForArgumentInfos(
    candidates: Seq[FunctionEntry],
    argumentInfos: Seq[ArgumentInfo],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): Option[FunctionEntry] = {
    val viableByArity = candidates.filter(candidate => functionArityIsViable(candidate, argumentInfos.size))
    val arityMatches  = candidates.filter(_.function.parameters.size == argumentInfos.size)
    val pool =
      if (viableByArity.nonEmpty) viableByArity else if (arityMatches.nonEmpty) arityMatches else candidates
    val scoredPool = pool.zipWithIndex.map { case (candidate, index) =>
      ScoredOverload(
        candidate,
        overloadScore(candidate, argumentInfos, receiverTypeFullName, explicitTemplateArguments),
        index
      )
    }
    if (scoredPool.exists(_.score.isViable)) {
      selectBestViableFunctionEntry(removeDominatedOverloads(scoredPool.filter(_.score.isViable)))
    } else {
      scoredPool
        .maxByOption(scored => (scored.score.score, scored.index))
        .map(_.candidate)
    }
  }

  private def selectBestViableFunctionEntry(scoredOverloads: Seq[ScoredOverload]): Option[FunctionEntry] = {
    scoredOverloads
      .map(_.score.score)
      .maxOption
      .flatMap { bestScore =>
        val best = scoredOverloads.filter(_.score.score == bestScore)
        if (best.size == 1) Some(best.head.candidate)
        else selectMutableMemberOverload(best)
      }
  }

  private def selectMutableMemberOverload(scoredOverloads: Seq[ScoredOverload]): Option[FunctionEntry] = {
    val nonConstMembers = scoredOverloads.filterNot(scored => scored.candidate.function.isConst)
    Option
      .when(
        nonConstMembers.size == 1 &&
          scoredOverloads.exists(_.candidate.function.isConst) &&
          scoredOverloads.forall(scored => sameMemberSignature(scored.candidate, nonConstMembers.head.candidate))
      )(nonConstMembers.head.candidate)
  }

  private def sameMemberSignature(left: FunctionEntry, right: FunctionEntry): Boolean = {
    left.ownerFullName == right.ownerFullName &&
    left.simpleName == right.simpleName &&
    left.function.parameters.map(_.semanticTypeName) == right.function.parameters.map(_.semanticTypeName)
  }

  private def removeDominatedOverloads(scoredOverloads: Seq[ScoredOverload]): Seq[ScoredOverload] = {
    scoredOverloads.filterNot { scored =>
      scoredOverloads.exists(other => other != scored && overloadDominates(other.score, scored.score))
    }
  }

  private def overloadDominates(left: OverloadScore, right: OverloadScore): Boolean = {
    left.argumentScores.size == right.argumentScores.size &&
    left.argumentScores.zip(right.argumentScores).forall { case (leftScore, rightScore) => leftScore >= rightScore } &&
    left.argumentScores.zip(right.argumentScores).exists { case (leftScore, rightScore) => leftScore > rightScore }
  }

  private def functionArityIsViable(candidate: FunctionEntry, argumentCount: Int): Boolean = {
    val parameters    = candidate.function.parameters
    val hasVariadic   = parameters.lastOption.exists(_.isVariadic)
    val requiredCount = parameters.takeWhile(parameter => !parameter.hasDefault && !parameter.isVariadic).size
    val maxCount      = Option.when(!hasVariadic)(parameters.size)
    argumentCount >= requiredCount && maxCount.forall(argumentCount <= _)
  }

  private def overloadScore(
    candidate: FunctionEntry,
    argumentInfos: Seq[ArgumentInfo],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): OverloadScore = {
    val receiverTemplateBindings =
      receiverTemplateBindingsForOwner(
        receiverTypeFullName,
        candidate.ownerFullName.orElse(candidate.lexicalOwnerFullName)
      )
    val explicitTemplateBindingResult =
      explicitTemplateBindingsForFunction(candidate, explicitTemplateArguments, receiverTemplateBindings)
    val explicitTemplateBindings = explicitTemplateBindingResult.getOrElse(Map.empty)
    val templateParameters =
      templateParameterNames(candidate.function).diff(
        receiverTemplateBindings.keySet ++ explicitTemplateBindings.keySet
      )
    val templateBindingResult = templateBindingsForArgumentInfos(candidate, argumentInfos, templateParameters)
    val templateBindings =
      receiverTemplateBindings ++ explicitTemplateBindings ++ templateBindingResult.getOrElse(Map.empty)
    val arityAdjustment = overloadArityAdjustment(candidate, argumentInfos.size)
    val invalidExplicitTemplatePenalty = Option
      .when(explicitTemplateBindingResult.isEmpty)(-10000)
      .getOrElse(0)
    val invalidTemplatePenalty = Option
      .when(templateBindingResult.isEmpty)(-10000)
      .getOrElse(0)
    val templatePenalty = Option.when(templateParameters.nonEmpty)(-5).getOrElse(0)
    val parameterCompatibilityScores = candidate.function.parameters
      .zip(argumentInfos)
      .map { case (parameter, argumentInfo) =>
        typeCompatibilityScore(substituteTemplateTypeNames(parameter.semanticTypeName, templateBindings), argumentInfo)
      }
    val score =
      arityAdjustment + invalidExplicitTemplatePenalty + invalidTemplatePenalty + templatePenalty +
        parameterCompatibilityScores.sum
    val isViable =
      functionArityIsViable(candidate, argumentInfos.size) &&
        explicitTemplateBindingResult.nonEmpty &&
        templateBindingResult.nonEmpty &&
        parameterCompatibilityScores.forall(_ > 0)
    OverloadScore(score, parameterCompatibilityScores, isViable)
  }

  private def overloadArityAdjustment(candidate: FunctionEntry, argumentCount: Int): Int = {
    val parameters         = candidate.function.parameters
    val hasVariadic        = parameters.lastOption.exists(_.isVariadic)
    val missingDefaultArgs = parameters.drop(argumentCount).count(_.hasDefault)
    val extraVariadicArgs  = if (hasVariadic) math.max(0, argumentCount - parameters.size + 1) else 0
    val invalidPenalty     = Option.when(!functionArityIsViable(candidate, argumentCount))(-1000).getOrElse(0)
    invalidPenalty - (missingDefaultArgs * 3) - (extraVariadicArgs * 5)
  }

  private def specializeFunctionTypeName(
    typeName: String,
    entry: FunctionEntry,
    arguments: Seq[OxExpression],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): String = {
    substituteTemplateTypeNames(
      typeName,
      templateBindingsForFunctionCall(entry, arguments, receiverTypeFullName, explicitTemplateArguments)
    )
  }

  private def templateBindingsForFunctionCall(
    entry: FunctionEntry,
    arguments: Seq[OxExpression],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): Map[String, String] = {
    val argumentInfos =
      arguments.map(argument => ArgumentInfo(argument, expressionTypeFullName(argument), isRvalue = false))
    templateBindingsForFunctionArgumentInfos(entry, argumentInfos, receiverTypeFullName, explicitTemplateArguments)
  }

  private def templateBindingsForFunctionArgumentInfos(
    entry: FunctionEntry,
    argumentInfos: Seq[ArgumentInfo],
    receiverTypeFullName: Option[String] = None,
    explicitTemplateArguments: Seq[String] = Seq.empty
  ): Map[String, String] = {
    val receiverTemplateBindings =
      receiverTemplateBindingsForOwner(receiverTypeFullName, entry.ownerFullName.orElse(entry.lexicalOwnerFullName))
    val explicitTemplateBindings =
      explicitTemplateBindingsForFunction(entry, explicitTemplateArguments, receiverTemplateBindings).getOrElse(
        Map.empty
      )
    val functionTemplateParameters =
      templateParameterNames(entry.function).diff(receiverTemplateBindings.keySet ++ explicitTemplateBindings.keySet)
    receiverTemplateBindings ++ explicitTemplateBindings ++
      templateBindingsForArgumentInfos(entry, argumentInfos, functionTemplateParameters).getOrElse(Map.empty)
  }

  private def explicitTemplateBindingsForFunction(
    entry: FunctionEntry,
    explicitTemplateArguments: Seq[String],
    receiverTemplateBindings: Map[String, String]
  ): Option[Map[String, String]] = {
    if (explicitTemplateArguments.isEmpty) {
      Some(Map.empty)
    } else {
      val parameters = orderedTemplateParameterNames(entry.function).filterNot(receiverTemplateBindings.contains)
      Option
        .when(parameters.nonEmpty && explicitTemplateArguments.size <= parameters.size) {
          parameters.zip(explicitTemplateArguments).toMap
        }
    }
  }

  private def receiverTemplateBindingsForOwner(
    receiverTypeFullName: Option[String],
    ownerTypeFullName: Option[String]
  ): Map[String, String] = {
    (receiverTypeFullName, ownerTypeFullName) match {
      case (Some(receiverType), Some(ownerType)) => receiverTemplateBindingsForOwner(receiverType, ownerType)
      case _                                     => Map.empty
    }
  }

  private def receiverTemplateBindingsForOwner(
    receiverTypeFullName: String,
    ownerTypeFullName: String
  ): Map[String, String] = {
    receiverTemplateBindingsForOwnerPath(receiverTypeFullName, ownerTypeFullName, Set.empty).getOrElse(Map.empty)
  }

  private def receiverTemplateBindingsForOwnerPath(
    receiverTypeFullName: String,
    ownerTypeFullName: String,
    seen: Set[String]
  ): Option[Map[String, String]] = {
    val receiverType          = aggregateLookupTypeName(receiverTypeFullName)
    val receiverAggregateType = receiverAggregateTypeName(receiverType)
    val currentOwnerCandidates =
      (resolveAggregateTypeFullName(receiverAggregateType).toSeq :+ receiverAggregateType).map(normalizeType).distinct
    val currentOwnerKey =
      currentOwnerCandidates.find(aggregateDeclarationEntriesByType.contains).getOrElse(receiverAggregateType)
    if (seen.contains(currentOwnerKey)) {
      None
    } else if (currentOwnerCandidates.contains(normalizeType(ownerTypeFullName))) {
      Option(receiverTemplateBindingsForExactOwner(receiverType, ownerTypeFullName))
    } else {
      val currentBindings = receiverTemplateBindingsForExactOwner(receiverType, currentOwnerKey)
      aggregateBaseClassesByType
        .getOrElse(currentOwnerKey, Seq.empty)
        .iterator
        .flatMap { baseClass =>
          val concreteBaseType = substituteTemplateTypeNames(baseClass.typeFullName, currentBindings)
          receiverTemplateBindingsForOwnerPath(concreteBaseType, ownerTypeFullName, seen + currentOwnerKey)
        }
        .toSeq
        .headOption
    }
  }

  private def receiverTemplateBindingsForExactOwner(
    receiverTypeFullName: String,
    ownerTypeFullName: String
  ): Map[String, String] = {
    val arguments = templateArgumentTypeNames(receiverTypeFullName)
    val parameters = aggregateDeclarationEntriesByType
      .get(normalizeType(ownerTypeFullName))
      .map { case (structDecl, _) => templateParameterNames(structDecl) }
      .getOrElse(Seq.empty)
    if (parameters.nonEmpty && parameters.size == arguments.size) {
      parameters.zip(arguments).toMap
    } else {
      Map.empty
    }
  }

  private def templateBindingsForArgumentInfos(
    entry: FunctionEntry,
    argumentInfos: Seq[ArgumentInfo],
    templateParameters: Set[String]
  ): Option[Map[String, String]] = {
    if (templateParameters.isEmpty) {
      Some(Map.empty)
    } else {
      entry.function.parameters
        .zip(argumentInfos)
        .foldLeft(Option(Map.empty[String, String])) { case (bindingsOption, (parameter, argumentInfo)) =>
          bindingsOption.flatMap { bindings =>
            val updates = argumentInfo.typeFullName
              .map { argumentTypeName =>
                val parameterTypeName =
                  ownerResolvedTypeFullNamePreservingCv(
                    parameter.semanticTypeName,
                    entry.ownerFullName.orElse(entry.lexicalOwnerFullName)
                  )
                templateBindingsForParameterType(parameterTypeName, argumentTypeName, templateParameters)
              }
              .getOrElse(Seq.empty)
            mergeTemplateBindings(bindings, updates)
          }
        }
    }
  }

  private def mergeTemplateBindings(
    bindings: Map[String, String],
    updates: Seq[(String, String)]
  ): Option[Map[String, String]] = {
    updates.foldLeft(Option(bindings)) { case (bindingsOption, (name, typeName)) =>
      bindingsOption.flatMap { current =>
        current.get(name) match {
          case Some(existing) if normalizeType(existing) != normalizeType(typeName) => None
          case _ => Some(current.updated(name, typeName))
        }
      }
    }
  }

  private def templateBindingsForParameterType(
    parameterTypeName: String,
    argumentTypeName: String,
    templateParameters: Set[String]
  ): Seq[(String, String)] = {
    templateParameters.toSeq.flatMap { templateParameter =>
      Option
        .when(typeNameContainsToken(parameterTypeName, templateParameter))(
          deduceTemplateBinding(parameterTypeName, argumentTypeName, templateParameter)
        )
        .flatten
        .map(templateParameter -> _)
    }
  }

  private def deduceTemplateBinding(
    parameterTypeName: String,
    argumentTypeName: String,
    templateParameter: String
  ): Option[String] = {
    val parameterType = normalizeType(resolveAliasType(parameterTypeName))
    val argumentType  = normalizeType(resolveAliasType(argumentTypeName))
    templateDeductionTypePairs(parameterType, argumentType).iterator
      .flatMap { case (parameterCandidate, argumentCandidate) =>
        matchTemplateBinding(parameterCandidate, argumentCandidate, templateParameter)
      }
      .toSeq
      .headOption
  }

  private def templateDeductionTypePairs(parameterType: String, argumentType: String): Seq[(String, String)] = {
    val parameterWithoutReference  = stripCxxReference(parameterType)
    val argumentWithoutReference   = stripCxxReference(argumentType)
    val argumentWithoutCvReference = stripCxxTypeQualifiers(argumentWithoutReference).trim
    val pairs                      = mutable.ArrayBuffer(parameterType -> argumentType)
    if (parameterWithoutReference != parameterType) {
      pairs += parameterWithoutReference -> argumentWithoutReference
      if (stripCxxTypeQualifiers(parameterWithoutReference).trim != parameterWithoutReference) {
        pairs += parameterWithoutReference -> s"const $argumentWithoutCvReference"
      }
    } else {
      pairs += parameterType -> argumentWithoutCvReference
    }
    pairs.distinct.toSeq
  }

  private def matchTemplateBinding(
    parameterTypeName: String,
    argumentTypeName: String,
    templateParameter: String
  ): Option[String] = {
    val tokenPattern = Pattern.compile(s"\\b${Pattern.quote(templateParameter)}\\b")
    val tokenMatcher = tokenPattern.matcher(parameterTypeName)
    val regexBuilder = new StringBuilder("^")
    var lastIndex    = 0
    var groupCount   = 0
    while (tokenMatcher.find()) {
      regexBuilder.append(Pattern.quote(parameterTypeName.substring(lastIndex, tokenMatcher.start())))
      regexBuilder.append("(.+?)")
      lastIndex = tokenMatcher.end()
      groupCount += 1
    }
    if (groupCount == 0) {
      None
    } else {
      regexBuilder.append(Pattern.quote(parameterTypeName.substring(lastIndex))).append("$")
      val matcher = Pattern.compile(regexBuilder.toString).matcher(argumentTypeName)
      Option
        .when(matcher.matches()) {
          val bindings = (1 to groupCount).map(index => normalizeType(matcher.group(index))).filter(_.nonEmpty)
          bindings.headOption.filter(first => bindings.forall(_ == first))
        }
        .flatten
    }
  }

  private def substituteTemplateTypeNames(typeName: String, bindings: Map[String, String]): String = {
    bindings.foldLeft(typeName) { case (current, (name, replacement)) =>
      Pattern
        .compile(s"\\b${Pattern.quote(name)}\\b")
        .matcher(current)
        .replaceAll(Matcher.quoteReplacement(replacement))
    }
  }

  private def functionTemplateParametersInType(
    entry: FunctionEntry,
    typeName: String,
    receiverTypeFullName: Option[String]
  ): Set[String] = {
    val receiverTemplateBindings =
      receiverTemplateBindingsForOwner(receiverTypeFullName, entry.ownerFullName.orElse(entry.lexicalOwnerFullName))
    templateParameterNames(entry.function)
      .diff(receiverTemplateBindings.keySet)
      .filter(typeNameContainsToken(typeName, _))
  }

  private def templateParameterNames(function: OxFunctionDecl): Set[String] = {
    orderedTemplateParameterNames(function).toSet
  }

  private def orderedTemplateParameterNames(function: OxFunctionDecl): Seq[String] = {
    val namesFromTemplatePrefix = TemplateParameterListPattern
      .findAllMatchIn(function.code)
      .flatMap(templateMatch => TemplateTypeParameterPattern.findAllMatchIn(templateMatch.group(1)).map(_.group(1)))
      .toSeq
    if (namesFromTemplatePrefix.nonEmpty) {
      namesFromTemplatePrefix.distinct
    } else {
      val typeNames = function.semanticReturnType +: function.parameters.map(_.semanticTypeName)
      typeNames
        .flatMap(templateParameterTokens)
        .filterNot(typeName => resolveAggregateTypeFullName(typeName).isDefined)
        .distinct
    }
  }

  private def templateParameterNames(structDecl: OxStructDecl): Seq[String] = {
    val namesFromTemplatePrefix = TemplateParameterListPattern
      .findAllMatchIn(structDecl.code)
      .flatMap(templateMatch => TemplateTypeParameterPattern.findAllMatchIn(templateMatch.group(1)).map(_.group(1)))
      .toSeq
    if (namesFromTemplatePrefix.nonEmpty) {
      namesFromTemplatePrefix.distinct
    } else {
      val fieldTypes = structDecl.fields.map(_.semanticTypeName)
      val baseTypes  = structDecl.baseClassDeclarations.map(_.name)
      val functionTypes = structDecl.nestedDeclarations.collect { case function: OxFunctionDecl =>
        function.semanticReturnType +: function.parameters.map(_.semanticTypeName)
      }.flatten
      (fieldTypes ++ baseTypes ++ functionTypes)
        .flatMap(templateParameterTokens)
        .filterNot(typeName => resolveAggregateTypeFullName(typeName).isDefined)
        .distinct
    }
  }

  private def templateParameterTokens(typeName: String): Seq[String] = {
    IdentifierTokenPattern.findAllIn(typeName).filter(isTemplateParameterComparableType).toSeq
  }

  private def typeNameContainsToken(typeName: String, token: String): Boolean = {
    Pattern.compile(s"\\b${Pattern.quote(token)}\\b").matcher(typeName).find()
  }

  private def typeCompatibilityScore(parameterTypeName: String, argumentInfo: ArgumentInfo): Int = {
    val directScore = argumentInfo.typeFullName
      .map(argumentTypeName => directTypeCompatibilityScore(parameterTypeName, argumentTypeName, argumentInfo.isRvalue))
      .getOrElse(1)
    if (directScore > 0) {
      directScore
    } else {
      contextualConversionOperatorTarget(argumentInfo.expression, Option(parameterTypeName))
        .flatMap(target => conversionOperatorCompatibilityScore(parameterTypeName, target.entry))
        .getOrElse(0)
    }
  }

  private def conversionOperatorCompatibilityScore(parameterTypeName: String, entry: FunctionEntry): Option[Int] = {
    val returnType  = functionSemanticReturnTypeFullName(entry)
    val directScore = directTypeCompatibilityScore(parameterTypeName, returnType, typeNameIsRvalue(returnType))
    Option.when(directScore > 0)(10 + math.max(0, math.min(directScore - 50, 9)))
  }

  private def directTypeCompatibilityScore(
    parameterTypeName: String,
    argumentTypeName: String,
    argumentIsRvalue: Boolean
  ): Int = {
    val parameterType       = overloadComparableType(parameterTypeName)
    val argumentType        = overloadComparableType(argumentTypeName)
    val arithmeticConverts  = arithmeticConversionScore(parameterType, argumentType).isDefined
    val rejectsNonConstBind = arithmeticConverts && nonConstLvalueReferenceTypeName(parameterTypeName)
    overloadBaseCompatibilityScore(parameterType, argumentType)
      .filter(_ => !rejectsNonConstBind)
      .flatMap(baseScore => typeBindingScore(parameterTypeName, argumentTypeName, argumentIsRvalue).map(baseScore + _))
      .getOrElse(0)
  }

  private def overloadBaseCompatibilityScore(parameterType: String, argumentType: String): Option[Int] = {
    if (isTemplateParameterComparableType(parameterType)) Some(20)
    else if (parameterType == Defines.Any || argumentType == Defines.Any) Some(10)
    else if (parameterType == argumentType) Some(60)
    else if (parameterType.endsWith(s".$argumentType") || argumentType.endsWith(s".$parameterType")) Some(55)
    else
      nullPointerConversionScore(parameterType, argumentType)
        .orElse(arrayToPointerConversionScore(parameterType, argumentType))
        .orElse(pointerConversionScore(parameterType, argumentType))
        .orElse(arithmeticConversionScore(parameterType, argumentType))
        .orElse {
          inheritanceDistanceFromArgumentToParameter(argumentType, parameterType)
            .filter(_ > 0)
            .map(distance => 50 - math.min(distance, 40))
        }
  }

  private def nullPointerConversionScore(parameterType: String, argumentType: String): Option[Int] = {
    Option.when(argumentType == "std.nullptr_t" && parameterType.endsWith("*"))(45)
  }

  private def pointerConversionScore(parameterType: String, argumentType: String): Option[Int] = {
    Option
      .when(parameterType.endsWith("*") && argumentType.endsWith("*")) {
        val parameterPointee = stripCxxTypeQualifiers(parameterType.stripSuffix("*")).trim
        val argumentPointee  = stripCxxTypeQualifiers(argumentType.stripSuffix("*")).trim
        Option.when(parameterPointee == Defines.Void && argumentPointee != Defines.Void)(45)
      }
      .flatten
  }

  private def arrayToPointerConversionScore(parameterType: String, argumentType: String): Option[Int] = {
    Option
      .when(parameterType.endsWith("*") && isArrayLikeType(argumentType)) {
        val parameterPointee = stripCxxTypeQualifiers(parameterType.stripSuffix("*")).trim
        val argumentElement  = arrayElementTypeFullName(argumentType).map(stripCxxTypeQualifiers).map(_.trim)
        argumentElement.flatMap { argumentPointee =>
          if (parameterPointee == argumentPointee) Some(59)
          else Option.when(parameterPointee == Defines.Void)(45)
        }
      }
      .flatten
  }

  private def arithmeticConversionScore(parameterType: String, argumentType: String): Option[Int] = {
    val parameter = canonicalArithmeticType(parameterType)
    val argument  = canonicalArithmeticType(argumentType)
    if (!CxxArithmeticTypes.contains(parameter) || !CxxArithmeticTypes.contains(argument) || parameter == argument) {
      None
    } else if (parameter == "int" && CxxIntegralPromotionSources.contains(argument)) {
      Some(58)
    } else if (parameter == "double" && argument == "float") {
      Some(58)
    } else {
      Some(45)
    }
  }

  private def canonicalArithmeticType(typeName: String): String = {
    val parts = stripCxxTypeQualifiers(normalizeType(typeName)).trim.split("\\s+").filter(_.nonEmpty).toSeq
    parts match {
      case Seq("signed", rest*)   => canonicalArithmeticType(rest.mkString(" "))
      case Seq("short", "int")    => "short"
      case Seq("unsigned")        => "unsigned int"
      case Seq("unsigned", "int") => "unsigned int"
      case Seq("unsigned", "short") | Seq("unsigned", "short", "int") =>
        "unsigned short"
      case Seq("long", "int") => "long"
      case Seq("unsigned", "long") | Seq("unsigned", "long", "int") =>
        "unsigned long"
      case Seq("long", "long") | Seq("long", "long", "int") =>
        "long long"
      case Seq("unsigned", "long", "long") | Seq("unsigned", "long", "long", "int") =>
        "unsigned long long"
      case _ => parts.mkString(" ")
    }
  }

  private def nonConstLvalueReferenceTypeName(typeName: String): Boolean = {
    val normalized = normalizeType(resolveAliasType(typeName))
    normalized.endsWith("&") && !normalized.endsWith("&&") && !receiverObjectTypeIsConst(normalized)
  }

  private def inheritanceDistanceFromArgumentToParameter(argumentType: String, parameterType: String): Option[Int] = {
    val parameterCandidates = aggregateFullNameCandidates(parameterType)
    typeAndBaseTypeFullNames(argumentType).zipWithIndex.collectFirst {
      case (candidate, distance) if parameterCandidates.contains(candidate) => distance
    }
  }

  private def aggregateFullNameCandidates(typeName: String): Set[String] = {
    val aggregateType = receiverAggregateTypeName(typeName)
    (resolveAggregateTypeFullName(aggregateType).toSeq :+ aggregateType).toSet
  }

  private def isTemplateParameterComparableType(typeName: String): Boolean = {
    typeName.matches("[A-Z][0-9]?")
  }

  private def typeBindingScore(
    parameterTypeName: String,
    argumentTypeName: String,
    argumentIsRvalue: Boolean
  ): Option[Int] = {
    val parameterType          = normalizeType(resolveAliasType(parameterTypeName))
    val argumentType           = normalizeType(resolveAliasType(argumentTypeName))
    val parameterObjectIsConst = receiverObjectTypeIsConst(parameterType)
    val argumentObjectIsConst  = receiverObjectTypeIsConst(argumentTypeName)
    if (parameterType.endsWith("*") && argumentType.endsWith("*")) {
      Option.when(parameterObjectIsConst || !argumentObjectIsConst) {
        if (parameterObjectIsConst && !argumentObjectIsConst) 1 else 2
      }
    } else if (parameterType.endsWith("*") && isArrayLikeType(argumentType)) {
      Option.when(parameterObjectIsConst || !argumentObjectIsConst) {
        if (parameterObjectIsConst && !argumentObjectIsConst) 1 else 2
      }
    } else if (parameterType.endsWith("&&")) {
      Option.when(argumentIsRvalue && (!argumentObjectIsConst || parameterObjectIsConst))(3)
    } else if (parameterType.endsWith("&")) {
      Option.when(parameterObjectIsConst || (!argumentObjectIsConst && !argumentIsRvalue)) {
        if (argumentIsRvalue) 0
        else if (parameterObjectIsConst && !argumentObjectIsConst) 1
        else 2
      }
    } else {
      Some(0)
    }
  }

  private def expressionIsRvalue(expression: OxExpression): Boolean = {
    expression match {
      case _: OxIdentifier | _: OxFieldAccess => false
      case indexAccess: OxIndexAccess =>
        overloadedIndexOperatorTarget(indexAccess)
          .map(target =>
            typeNameIsRvalue(
              functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
            )
          )
          .getOrElse(false)
      case assignment: OxAssignment =>
        overloadedAssignmentOperatorTarget(assignment)
          .map(target =>
            typeNameIsRvalue(
              functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
            )
          )
          .getOrElse(false)
      case unary: OxUnary =>
        overloadedUnaryOperatorTarget(unary)
          .map(target =>
            typeNameIsRvalue(
              functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
            )
          )
          .getOrElse(unary.operator != "*")
      case binary: OxBinary =>
        overloadedBinaryOperatorTarget(binary)
          .map(target =>
            typeNameIsRvalue(
              functionSemanticReturnTypeFullName(target.entry, target.arguments, receiverTypeFullName(target))
            )
          )
          .getOrElse(true)
      case OxPackExpansion(_, _, pattern) => expressionIsRvalue(pattern)
      case _: OxTypeOf                    => true
      case call: OxCall =>
        callReturnTypeFullName(call).map(typeNameIsRvalue).getOrElse(true)
      case OxCast(_, semanticTypeName, _, _, _) =>
        typeNameIsRvalue(semanticTypeName)
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
      case OxFieldAccess(field, _, _, _) =>
        stripTemplateArguments(qualifiedNameParts(field).lastOption.getOrElse(field))
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
    splitConversionOperatorFunctionName(function.name)
      .map(_._2)
      .getOrElse(qualifiedNameParts(function.name).lastOption.getOrElse(function.name))
  }

  private def functionOwnerFullName(function: OxFunctionDecl, ownerFullName: Option[String]): Option[String] = {
    splitConversionOperatorFunctionName(function.name) match {
      case Some((Some(localOwner), _)) =>
        ownerFullName.map(owner => s"$owner.$localOwner").orElse(Option(localOwner))
      case Some((None, _)) =>
        ownerFullName
      case None =>
        val parts = qualifiedNameParts(function.name)
        if (parts.size > 1) {
          val localOwner = parts.dropRight(1).mkString(".")
          ownerFullName.map(owner => s"$owner.$localOwner").orElse(Option(localOwner))
        } else {
          ownerFullName
        }
    }
  }

  private def splitConversionOperatorFunctionName(name: String): Option[(Option[String], String)] = {
    val operatorIndex = name.indexOf("operator ")
    Option.when(operatorIndex >= 0) {
      val owner = name.take(operatorIndex).trim.stripSuffix("::").trim
      val simpleName =
        name.drop(operatorIndex).trim match {
          case operatorName if operatorName.startsWith("operator ") =>
            s"operator ${normalizeType(operatorName.stripPrefix("operator ").trim)}"
          case operatorName => operatorName
        }
      Option(owner).filter(_.nonEmpty).map(normalizedQualifiedName) -> simpleName
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
      virtualFunctionInTypeHierarchy(ownerTypeFullName, function, Set.empty)
    }
  }

  private def virtualFunctionInTypeHierarchy(
    ownerTypeFullName: String,
    function: OxFunctionDecl,
    seen: Set[String]
  ): Boolean = {
    val normalizedOwner = receiverAggregateTypeName(resolveAliasType(ownerTypeFullName))
    if (seen.contains(normalizedOwner)) {
      false
    } else {
      val directVirtual = functionEntries.exists(entry =>
        entry.ownerFullName.contains(normalizedOwner) &&
          isSameVirtualSlot(entry, function) &&
          entry.function.isVirtual
      )
      directVirtual || aggregateBaseTypesByType
        .getOrElse(normalizedOwner, Seq.empty)
        .exists(baseType => virtualFunctionInTypeHierarchy(baseType, function, seen + normalizedOwner))
    }
  }

  private def isSameVirtualSlot(entry: FunctionEntry, function: OxFunctionDecl): Boolean = {
    entry.simpleName == functionSimpleName(function) &&
    virtualSlotSignature(entry.function) == virtualSlotSignature(function)
  }

  private def virtualSlotSignature(function: OxFunctionDecl): String = {
    val parameterStart = function.signature.indexOf('(')
    if (parameterStart >= 0) {
      function.signature.drop(parameterStart)
    } else {
      val parameters =
        function.parameters.map(parameter => normalizeType(resolveAliasType(parameter.typeName))).mkString(",")
      s"($parameters)${Option.when(function.isConst)("<const>").getOrElse("")}"
    }
  }

  private def methodModifiers(
    simpleName: String,
    parentTypeOwner: Option[String],
    isStaticMethod: Boolean,
    isVirtualMethod: Boolean
  ): Seq[NewModifier] = {
    val isConstructor = isConstructorMethod(simpleName, parentTypeOwner)
    Option.when(isConstructor)(NewModifier().modifierType(ModifierTypes.CONSTRUCTOR)).toSeq ++
      Option.when(isStaticMethod)(NewModifier().modifierType(ModifierTypes.STATIC)).toSeq ++
      Option.when(isVirtualMethod)(NewModifier().modifierType(ModifierTypes.VIRTUAL)).toSeq
  }

  private def isConstructorMethod(simpleName: String, parentTypeOwner: Option[String]): Boolean = {
    parentTypeOwner
      .flatMap(_.split('.').lastOption)
      .contains(simpleName)
  }

  private def isDestructorMethod(simpleName: String, parentTypeOwner: Option[String]): Boolean = {
    parentTypeOwner
      .flatMap(_.split('.').lastOption)
      .exists(localTypeName => simpleName == s"~$localTypeName")
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
      aliases
        .get(normalized)
        .orElse(resolveAliasTypeWithCvQualifiers(normalized, aliases))
        .getOrElse(normalized)
    }
  }

  private def resolveAliasTypeWithCvQualifiers(typeName: String, aliases: Map[String, String]): Option[String] = {
    val parts       = typeName.split("\\s+").filter(_.nonEmpty)
    val objectParts = parts.filterNot(CxxTypeQualifiers.contains)
    Option
      .when(objectParts.size == 1)(objectParts.head)
      .flatMap(aliasName => aliases.get(aliasName).map(aliasName -> _))
      .map { case (aliasName, resolvedTypeName) =>
        parts.map(part => if (part == aliasName) resolvedTypeName else part).mkString(" ")
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
