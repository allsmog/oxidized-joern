package io.joern.c2cpg.oxidized

import io.joern.c2cpg.Config
import io.joern.c2cpg.astcreation.Defines
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
  private final case class FunctionCaptureContext(
    function: OxFunctionDecl,
    methodRef: NewMethodRef,
    capturedGlobals: mutable.LinkedHashMap[String, CapturedGlobal] = mutable.LinkedHashMap.empty
  )

  private val usedTypes: mutable.Set[String]           = mutable.Set(Defines.Any, Defines.Void)
  private lazy val functionEntries: Seq[FunctionEntry] = collectFunctionEntries(document.declarations, None)
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
  private lazy val aggregateBaseTypesByType: Map[String, Seq[String]] =
    aggregateDeclarations.flatMap { case (structDecl, parentFullName) =>
      val localName = normalizeType(structDecl.name)
      val fullName  = parentFullName.map(parent => s"$parent.$localName").getOrElse(localName)
      val baseTypes = structDecl.baseClasses.map(baseClass => resolveBaseTypeFullName(baseClass, parentFullName))
      Seq(localName, fullName).distinct.map(typeName => typeName -> baseTypes)
    }.toMap
  private val IntegerLiteralPattern = """[+-]?(?:0[xX][0-9a-fA-F]+|\d+)[uUlL]*""".r
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
    Ast(typeDecl).withChildren(fieldAsts ++ nestedAsts ++ outOfClassMethodAsts)
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
      val normalized = normalizeType(resolveAliasType(current))
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
          isVariadic = false,
          EvaluationStrategies.BY_VALUE,
          parameterType
        )
      parameter.name -> (parameterType, Ast(parameterNode), parameterNode)
    }
    val parameters = implicitThisParameter ++ explicitParameters

    val previousScope          = scope
    val previousCaptureContext = functionCaptureContext
    val previousMethodOwner    = currentMethodOwnerTypeFullName
    val captureContext =
      FunctionCaptureContext(function, methodRefNode(origin, simpleName, fullName, simpleName))
    scope = parameters.map { case (name, (typeName, _, node)) => name -> ScopeEntry(typeName, node) }.toMap
    functionCaptureContext = Option(captureContext)
    currentMethodOwnerTypeFullName = parentTypeOwner
    val bodyAsts =
      try function.constructorInitializers.map(constructorInitializerAst) ++ function.body.flatMap(astsForStatement)
      finally {
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

  private def astsForStatement(statement: OxStatement): Seq[Ast] = {
    statement match {
      case local: OxLocalDecl =>
        val origin    = OxOrigin(local)
        val typeName  = registerType(normalizeType(local.typeName))
        val localCode = localDeclarationCode(local)
        val localNode = this.localNode(origin.copy(code = localCode), local.name, localCode, typeName)
        scope = scope.updated(local.name, ScopeEntry(typeName, localNode))
        val localAst = Ast(localNode)
        local.initializer match {
          case Some(initializer: OxInitializerList) if isConstructorInitializer(typeName, initializer) =>
            Seq(localAst, constructorAssignmentAst(local, initializer, typeName))
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
        Seq {
          overloadedAssignmentOperatorAst(assignment).getOrElse {
            val left  = expressionAst(assignment.left)
            val right = expressionAst(assignment.right)
            if (assignment.operator == "=") {
              assignmentAst(OxOrigin(assignment), left, right, assignment.code)
            } else {
              operatorCallAst(OxOrigin(assignment), assignment.code, operatorFor(assignment.operator), Seq(left, right))
            }
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

  private def localDeclarationCode(local: OxLocalDecl): String = {
    local.initializer match {
      case Some(_) if local.code.contains("=") => local.code.takeWhile(_ != '=').trim
      case Some(initializer)                   => local.code.stripSuffix(initializer.code).trim
      case None                                => local.code
    }
  }

  private def isConstructorInitializer(typeName: String, initializer: OxInitializerList): Boolean = {
    aggregateTypeFullNames.contains(typeName) && initializer.code.trim.startsWith("(")
  }

  private def constructorAssignmentAst(local: OxLocalDecl, initializer: OxInitializerList, typeName: String): Ast = {
    val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
    val constructor     = constructorEntry(typeName, initializer.elements)
    val signature       = constructor.map(_.function.signature)
    val methodFullName  = constructor.map(_.fullName).getOrElse(s"$typeName.$constructorName")
    val initCode        = initializer.code.trim.stripPrefix("(").stripSuffix(")")
    val constructorCode = s"$typeName.$constructorName($initCode)"
    val callNode_ = callNode(
      OxOrigin(initializer).copy(code = constructorCode),
      constructorCode,
      constructorName,
      methodFullName,
      DispatchTypes.STATIC_DISPATCH,
      signature,
      Some(registerType(Defines.Void))
    )
    val assignmentCode = s"${local.name} = $constructorCode"
    val left           = identifierAst(local.name, local.name, local.line)
    val right          = callAst(callNode_, initializer.elements.map(expressionAst))
    assignmentAst(OxOrigin(local).copy(code = assignmentCode), left, right, assignmentCode)
  }

  private def constructorEntry(typeName: String, arguments: Seq[OxExpression]): Option[FunctionEntry] = {
    val constructorName = typeName.split('.').lastOption.getOrElse(typeName)
    val candidates      = functionEntries.filter(entry => entry.qualifiedName == s"$typeName.$constructorName")
    selectFunctionEntry(candidates, Some(arguments))
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
        overloadedBinaryOperatorAst(binary).getOrElse(
          operatorCallAst(
            OxOrigin(binary),
            binary.code,
            operatorFor(binary.operator),
            Seq(expressionAst(binary.left), expressionAst(binary.right))
          )
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
    overloadedCallOperatorAst(call).getOrElse {
      if (isPointerCall(call)) pointerCallAst(call) else directCallAst(call)
    }
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
        selectFunctionEntry(functionCandidatesByName(operatorName), Some(Seq(binary.left, binary.right)))
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
        typeAndBaseTypeFullNames(receiverType).reverse.flatMap(typeName =>
          functionCandidatesByQualifiedName(s"$typeName.$name")
        )
      )
  }

  private def receiverAggregateTypeName(typeName: String): String = {
    val normalized = normalizeType(resolveAliasType(typeName))
    stripCxxTypeQualifiers(stripCxxReference(normalized).stripSuffix("*").stripSuffix("[]"))
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
      case _: OxFieldAccess             => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case OxUnary("*", _, _, _, _)     => true
      case _: OxUnary                   => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxIdentifier | _: OxCast  => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxCall | _: OxIndexAccess => expressionTypeFullName(call.callee).exists(isFunctionPointerType)
      case _: OxInitializerList         => false
      case _: OxDesignatedInitializer   => false
      case _: OxDesignator              => false
      case _: OxLiteral                 => false
      case _: OxBinary | _: OxConditional | _: OxSizeOf | _: OxNew | _: OxDelete => false
    }
  }

  private def callReturnTypeFullName(call: OxCall): Option[String] = {
    overloadedCallOperatorTarget(call)
      .map(target => normalizeType(target.entry.function.returnType))
      .orElse(
        expressionTypeFullName(call.callee)
          .flatMap(returnTypeFromFunctionPointer)
          .orElse(functionEntryForCall(call).map(entry => normalizeType(entry.function.returnType)))
      )
  }

  private def expressionTypeFullName(expression: OxExpression): Option[String] = {
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
      case OxFieldAccess(field, _, _, base) =>
        expressionTypeFullName(base).flatMap(typeName => fieldTypeFullName(typeName, field))
      case OxUnary("*", _, _, _, argument) =>
        expressionTypeFullName(argument)
      case OxCast(typeName, _, _, _) =>
        Option(resolveAliasType(typeName))
      case indexAccess: OxIndexAccess =>
        overloadedIndexOperatorTarget(indexAccess)
          .map(target => normalizeType(target.entry.function.returnType))
          .orElse(expressionTypeFullName(indexAccess.base).map(_.stripSuffix("[]")))
      case call: OxCall =>
        callReturnTypeFullName(call)
      case binary: OxBinary =>
        overloadedBinaryOperatorTarget(binary).map(target => normalizeType(target.entry.function.returnType))
      case _ =>
        None
    }
  }

  private def fieldTypeFullName(baseTypeFullName: String, field: String): Option[String] = {
    fieldEntryForTypeHierarchy(baseTypeFullName, field).map { case (_, fieldDecl) =>
      resolveAliasType(fieldDecl.typeName)
    }
  }

  private def fieldEntryForTypeHierarchy(baseTypeFullName: String, field: String): Option[(String, OxFieldDecl)] = {
    val normalized = resolveAliasType(baseTypeFullName)
    val candidates = Seq(normalized, normalized.stripSuffix("*"), normalized.stripSuffix("[]")).distinct
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
    val normalized = normalizeType(typeName)
    val candidates = currentMethodOwnerTypeFullName
      .filter(_.split('.').lastOption.contains(normalized))
      .toSeq ++ Seq(normalized) ++ aggregateTypeFullNames.filter(_.endsWith(s".$normalized")).toSeq.sorted
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
            (callName(call), normalizedQualifiedName(call.name), None, Defines.Any, DispatchTypes.STATIC_DISPATCH)
        }
    }
  }

  private def functionEntryForCall(call: OxCall): Option[FunctionEntry] = {
    call.callee match {
      case OxFieldAccess(field, _, _, base) =>
        val candidates = expressionTypeFullName(base)
          .map(typeName => normalizeType(resolveAliasType(typeName)).stripSuffix("*").stripSuffix("[]"))
          .toSeq
          .flatMap(receiverType =>
            typeAndBaseTypeFullNames(receiverType).reverse.flatMap(typeName =>
              functionCandidatesByQualifiedName(s"$typeName.$field")
            )
          )
        selectFunctionEntry(candidates, Some(call.arguments))
      case _ =>
        val qualifiedName = normalizedQualifiedName(call.name)
        if (qualifiedNameParts(call.name).size > 1) {
          val candidates = functionCandidatesByQualifiedName(qualifiedName)
          selectFunctionEntry(
            if (candidates.nonEmpty) candidates else functionCandidatesByName(call.name),
            Some(call.arguments)
          )
        } else {
          val ownerCandidates     = currentOwnerFunctionCandidates(call.name)
          val qualifiedCandidates = functionCandidatesByQualifiedName(qualifiedName)
          val candidates =
            if (ownerCandidates.nonEmpty) ownerCandidates
            else if (qualifiedCandidates.nonEmpty) qualifiedCandidates
            else functionCandidatesByName(call.name)
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

  private def functionCandidatesByQualifiedName(name: String): Seq[FunctionEntry] = {
    functionsByQualifiedName.getOrElse(name, Seq.empty)
  }

  private def selectFunctionEntry(
    candidates: Seq[FunctionEntry],
    arguments: Option[Seq[OxExpression]]
  ): Option[FunctionEntry] = {
    arguments match {
      case Some(arguments) =>
        val arityMatches  = candidates.filter(_.function.parameters.size == arguments.size)
        val pool          = if (arityMatches.nonEmpty) arityMatches else candidates
        val argumentTypes = arguments.map(argument => expressionTypeFullName(argument))
        pool.zipWithIndex
          .maxByOption { case (candidate, index) => (overloadScore(candidate, argumentTypes), index) }
          .map(_._1)
      case None =>
        candidates.lastOption
    }
  }

  private def overloadScore(candidate: FunctionEntry, argumentTypes: Seq[Option[String]]): Int = {
    val arityPenalty = math.abs(candidate.function.parameters.size - argumentTypes.size) * -100
    arityPenalty + candidate.function.parameters
      .zip(argumentTypes)
      .map { case (parameter, argumentType) =>
        argumentType.map(typeCompatibilityScore(parameter.typeName, _)).getOrElse(1)
      }
      .sum
  }

  private def typeCompatibilityScore(parameterTypeName: String, argumentTypeName: String): Int = {
    val parameterType = overloadComparableType(parameterTypeName)
    val argumentType  = overloadComparableType(argumentTypeName)
    if (parameterType == Defines.Any || argumentType == Defines.Any) 1
    else if (parameterType == argumentType) 4
    else if (parameterType.endsWith(s".$argumentType") || argumentType.endsWith(s".$parameterType")) 3
    else 0
  }

  private def overloadComparableType(typeName: String): String = {
    val dereferenced =
      if (typeName.endsWith("&&")) typeName.dropRight(2)
      else if (typeName.endsWith("&")) typeName.dropRight(1)
      else typeName
    resolveAliasType(dereferenced)
      .split("\\s+")
      .filterNot(part => Set("const", "volatile", "mutable").contains(part))
      .mkString(" ")
  }

  private def callName(call: OxCall): String = {
    call.callee match {
      case OxFieldAccess(field, _, _, _) => field
      case _                             => qualifiedNameParts(call.name).lastOption.getOrElse(call.name)
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
    functionOwnerFullName(function, ownerFullName)
      .map(owner => s"$owner.$simpleName:${function.signature}")
      .getOrElse(function.name)
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
      .replace("::", ".")
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
