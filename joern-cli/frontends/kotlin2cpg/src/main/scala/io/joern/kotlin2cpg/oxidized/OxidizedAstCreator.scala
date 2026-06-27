package io.joern.kotlin2cpg.oxidized

import io.joern.kotlin2cpg.{Config, Constants}
import io.joern.kotlin2cpg.parser.{KotlinAstDocument, KotlinAstNode}
import io.joern.kotlin2cpg.types.TypeConstants
import io.joern.x2cpg.AstNodeBuilder.{bindingNode, closureBindingNode}
import io.joern.x2cpg.{Ast, AstCreatorBase, Defines, ValidationMode}
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.codepropertygraph.generated.{
  ControlStructureTypes,
  DiffGraphBuilder,
  DispatchTypes,
  EvaluationStrategies,
  ModifierTypes,
  NodeTypes,
  Operators
}
import io.shiftleft.semanticcpg.language.types.structure.NamespaceTraversal

import scala.collection.mutable

final class OxidizedAstCreator(document: KotlinAstDocument, config: Config)
    extends AstCreatorBase[KotlinAstNode, OxidizedAstCreator](document.relativeName)(config.schemaValidation) {

  private implicit val validationMode: ValidationMode = config.schemaValidation

  private val usedTypeNames: mutable.Set[String] =
    mutable.Set(TypeConstants.Any, TypeConstants.Void, TypeConstants.JavaLangObject, TypeConstants.Kotlin)
  private val importAliases: mutable.Map[String, String]              = mutable.Map.empty
  private val typeAliases: mutable.Map[String, String]                = mutable.Map.empty
  private val typeParameterBounds: mutable.Map[String, String]        = mutable.Map.empty
  private val typeDeclarationInfos: mutable.Map[String, TypeInfo]     = mutable.Map.empty
  private val lambdaMethodAsts: mutable.ListBuffer[Ast]               = mutable.ListBuffer.empty
  private val lambdaTypeDeclAsts: mutable.ListBuffer[Ast]             = mutable.ListBuffer.empty
  private val callableReferenceTypeDeclAsts: mutable.ListBuffer[Ast]  = mutable.ListBuffer.empty
  private val callableReferenceTypeDeclFullNames: mutable.Set[String] = mutable.Set.empty
  private val companionObjects: mutable.Map[String, CompanionInfo]    = mutable.Map.empty
  private val methodsByOwnerNameAndArity: mutable.Map[(String, String, Int), MethodInfo] =
    mutable.Map.empty
  private val topLevelMethodsByNameAndArity: mutable.Map[(String, Int), MethodInfo] =
    mutable.Map.empty
  private val methodsByFullName: mutable.Map[String, NewMethod] =
    mutable.Map.empty
  private val constructorsByTypeAndArity: mutable.Map[(String, Int), MethodInfo] =
    mutable.Map.empty
  private val inheritedTypesByFullName: mutable.Map[String, Seq[String]] =
    mutable.Map.empty
  private val topLevelGlobals: mutable.Map[String, GlobalInfo] = mutable.Map.empty
  private var tmpLocalCounter: Int                             = 0
  private var iteratorLocalCounter: Int                        = 0
  private var objectLiteralCounter: Int                        = 0
  private var objectTempCounter: Int                           = 0
  private var sourcePackageName: Option[String]                = None

  private val KotlinModifierTypeByKeyword: Map[String, String] = Map(
    "public"    -> ModifierTypes.PUBLIC,
    "private"   -> ModifierTypes.PRIVATE,
    "protected" -> ModifierTypes.PROTECTED,
    "internal"  -> ModifierTypes.INTERNAL,
    "abstract"  -> ModifierTypes.ABSTRACT
  )
  private val VisibilityModifierTypes: Set[String] =
    Set(ModifierTypes.PUBLIC, ModifierTypes.PRIVATE, ModifierTypes.PROTECTED, ModifierTypes.INTERNAL)

  def usedTypes(): Set[String] = usedTypeNames.toSet

  override def createAst(): DiffGraphBuilder = {
    val fileNode = NewFile()
      .name(document.relativeName)
      .order(0)
    Option.when(!config.disableFileContent)(document.ast.code).foreach(fileNode.content)

    Ast.storeInDiffGraph(Ast(fileNode).withChild(astForSourceFile(document.ast)), diffGraph)
    callableReferenceTypeDeclAsts.foreach(Ast.storeInDiffGraph(_, diffGraph))
    diffGraph
  }

  protected def line(node: KotlinAstNode): Option[Int]      = Some(node.start.line)
  protected def column(node: KotlinAstNode): Option[Int]    = Some(node.start.column)
  protected def lineEnd(node: KotlinAstNode): Option[Int]   = Some(node.end.line)
  protected def columnEnd(node: KotlinAstNode): Option[Int] = Some(node.end.column)
  protected def code(node: KotlinAstNode): String           = node.code

  override protected def offset(node: KotlinAstNode): Option[(Int, Int)] =
    Some((node.startByte, node.endByte))

  private def astForSourceFile(root: KotlinAstNode): Ast = {
    val packageName = packageNameFor(root)
    sourcePackageName = packageName
    val namespaceBlock      = namespaceBlockFor(root, packageName)
    val importAsts          = root.children.filter(_.kind == "import_list").flatMap(astForImportList)
    val topLevelTypeAliases = root.children.filter(_.kind == "type_alias")
    val topLevelProperties  = root.children.filter(_.kind == "property_declaration")
    root.children.foreach {
      case declaration if declaration.kind == "class_declaration" || declaration.kind == "object_declaration" =>
        val name = typeDeclarationName(declaration)
        val fullName =
          fullNameForTypeDeclaration(declaration, name, packageName, ownerTypeFullName = None)
        registerType(fullName)
        importAliases.update(name, fullName)
      case _ =>
    }
    topLevelTypeAliases.foreach(registerTypeAlias(_, packageName))
    root.children.foreach {
      case declaration if declaration.kind == "function_declaration" =>
        registerTopLevelMethodInfo(declaration, packageName)
      case _ =>
    }
    root.children.foreach {
      case declaration if declaration.kind == "class_declaration" || declaration.kind == "object_declaration" =>
        registerTypeDeclarationInfo(declaration, packageName, ownerTypeFullName = None)
      case _ =>
    }
    val globalMethodAst = astForGlobalMethod(root, packageName, topLevelProperties)
    val declarationAsts = root.children.flatMap {
      case declaration if declaration.kind == "type_alias" =>
        Some(astForTypeAliasDeclaration(declaration, packageName))
      case declaration if declaration.kind == "class_declaration" || declaration.kind == "object_declaration" =>
        Some(astForTypeDeclaration(declaration, packageName))
      case declaration if declaration.kind == "function_declaration" =>
        Some(astForFunctionDeclaration(declaration, packageName))
      case _ => None
    }

    Ast(namespaceBlock)
      .withChildren(importAsts)
      .withChildren(globalMethodAst.toList ++ declarationAsts ++ lambdaMethodAsts.toList ++ lambdaTypeDeclAsts.toList)
  }

  private def namespaceBlockFor(root: KotlinAstNode, packageName: Option[String]): NewNamespaceBlock = {
    packageName match {
      case Some(name) if name.nonEmpty =>
        namespaceBlockNode(root, name, s"${document.relativeName}:$name", document.relativeName)
      case _ =>
        globalNamespaceBlock()
    }
  }

  private def astForImportList(importList: KotlinAstNode): List[Ast] = {
    importList.children.filter(_.kind == "import_header").map(astForImportHeader)
  }

  private def astForImportHeader(importHeader: KotlinAstNode): Ast = {
    val ImportParts(importedEntity, importedAs, isWildcard) = importParts(importHeader)
    if (!isWildcard) {
      registerType(importedEntity)
      importAliases.update(importedAs, importedEntity)
    }
    val importNode = NewImport()
      .code(importHeader.code)
      .importedEntity(importedEntity)
      .importedAs(importedAs)
      .isWildcard(isWildcard)
      .isModuleImport(false)
      .lineNumber(line(importHeader))
      .columnNumber(column(importHeader))
    Ast(importNode)
  }

  private def astForGlobalMethod(
    root: KotlinAstNode,
    packageName: Option[String],
    properties: List[KotlinAstNode]
  ): Option[Ast] = {
    if (properties.isEmpty) {
      None
    } else {
      val signature = methodSignature(TypeConstants.Any, Nil)
      val fullName  = methodFullName(packageName.map(pkg => s"$pkg.<global>").getOrElse("<global>"), signature)
      val method  = methodNode(root, NamespaceTraversal.globalNamespaceName, fullName, signature, document.relativeName)
      val context = BodyContext(mutable.Map.empty, mutable.Map.empty, fullName)
      val propertyAsts = properties.flatMap(property => astsForTopLevelProperty(property, packageName, context))
      Some(
        methodAst(
          method,
          Seq.empty,
          blockAst(blockNode(root, Constants.Empty, TypeConstants.Any), propertyAsts),
          methodReturnNode(root, TypeConstants.Any)
        )
      )
    }
  }

  private def astsForTopLevelProperty(
    propertyDeclaration: KotlinAstNode,
    packageName: Option[String],
    context: BodyContext
  ): List[Ast] = {
    val declaration =
      propertyDeclaration.children.find(_.kind == "variable_declaration").getOrElse(propertyDeclaration)
    val name = firstChildCode(declaration, "simple_identifier").getOrElse(nameFromDeclarationCode(declaration.code))
    val initializer = initializerNode(propertyDeclaration)
    initializer match {
      case Some(objectLiteral) if objectLiteral.kind == "object_literal" =>
        astsForTopLevelObjectLiteralProperty(propertyDeclaration, packageName, name, objectLiteral, context)
      case _ =>
        val typeName = typeFromDirectChildren(declaration)
          .orElse(initializer.flatMap(typeForExpression(_, context)))
          .getOrElse(TypeConstants.Any)
        val typeFullName = registerType(typeName)
        val local        = localNode(propertyDeclaration, name, name, typeFullName)
        val localAst     = Ast(local).withChildren(annotationNodesFor(propertyDeclaration).map(astForAnnotationEntry))
        context.types.update(name, typeFullName)
        context.refs.update(name, local)
        updateCollectionElementType(
          name,
          collectionElementTypeFromDirectChildren(declaration).orElse(
            collectionElementTypeFromDeclarationText(declaration.code)
          ),
          initializer,
          context,
          declaredPairTypes =
            pairTypesFromDirectChildren(declaration).orElse(pairTypesFromDeclarationText(declaration.code)),
          declaredTripleTypes =
            tripleTypesFromDirectChildren(declaration).orElse(tripleTypesFromDeclarationText(declaration.code))
        )
        topLevelGlobals.update(name, GlobalInfo(name, typeFullName, local))

        val assignmentAst = initializer.map { rhs =>
          val target    = identifierNode(propertyDeclaration, name, name, typeFullName)
          val targetAst = Ast(target).withRefEdge(target, local)
          callAst(
            operatorCallNode(propertyDeclaration, propertyDeclaration.code, Operators.assignment, Some(typeFullName)),
            Seq(targetAst, astForExpression(rhs, context, Some(typeFullName)))
          )
        }
        localAst :: assignmentAst.toList
    }
  }

  private def astsForTopLevelObjectLiteralProperty(
    propertyDeclaration: KotlinAstNode,
    packageName: Option[String],
    name: String,
    objectLiteral: KotlinAstNode,
    context: BodyContext
  ): List[Ast] = {
    val objectOwnerName  = packageName.map(pkg => s"$pkg.$name").getOrElse(name)
    val typeDeclAst      = astForObjectLiteralTypeDeclaration(objectLiteral, context, Some(objectOwnerName))
    val typeFullName     = typeDeclAst.root.collect { case typeDecl: NewTypeDecl => typeDecl.fullName }.get
    val local            = localNode(propertyDeclaration, name, name, typeFullName)
    val localAst         = Ast(local).withChildren(annotationNodesFor(propertyDeclaration).map(astForAnnotationEntry))
    val lhs              = identifierNode(propertyDeclaration, name, name, typeFullName)
    val assignmentTarget = Ast(lhs).withRefEdge(lhs, local)
    val alloc            = operatorCallNode(objectLiteral, Operators.alloc, Operators.alloc, Some(typeFullName))
    val assignment = callAst(
      operatorCallNode(propertyDeclaration, propertyDeclaration.code, Operators.assignment, Some(typeFullName)),
      Seq(assignmentTarget, Ast(alloc))
    )
    val initAst = astForObjectLiteralInitCall(objectLiteral, name, local, typeFullName)

    context.types.update(name, typeFullName)
    context.refs.update(name, local)
    topLevelGlobals.update(name, GlobalInfo(name, typeFullName, local))

    List(typeDeclAst, localAst, assignment, initAst)
  }

  private def astForTypeAliasDeclaration(typeAlias: KotlinAstNode, packageName: Option[String]): Ast = {
    val name              = typeAliasName(typeAlias)
    val fullName          = registerType(fullNameForTypeAlias(name, packageName))
    val aliasTypeFullName = registerType(typeAliasTargetFullName(typeAlias, name).getOrElse(TypeConstants.Any))
    val typeDecl = typeDeclNode(
      typeAlias,
      name,
      fullName,
      document.relativeName,
      name,
      NodeTypes.NAMESPACE_BLOCK,
      namespaceAstParentFullName(packageName),
      alias = Some(aliasTypeFullName)
    )
    Ast(typeDecl).withChildren(annotationNodesFor(typeAlias).map(astForAnnotationEntry))
  }

  private def registerTypeAlias(typeAlias: KotlinAstNode, packageName: Option[String]): Unit = {
    val name     = typeAliasName(typeAlias)
    val fullName = fullNameForTypeAlias(name, packageName)
    val target   = typeAliasTargetFullName(typeAlias, name).getOrElse(TypeConstants.Any)
    registerType(fullName)
    registerType(target)
    typeAliases.update(name, target)
    typeAliases.update(fullName, target)
  }

  private def typeAliasName(typeAlias: KotlinAstNode): String =
    firstChildCode(typeAlias, "type_identifier").getOrElse(nameFromDeclarationCode(typeAlias.code))

  private def typeAliasTargetFullName(typeAlias: KotlinAstNode, aliasName: String): Option[String] = {
    typeAliasTargetNode(typeAlias).map { target =>
      val targetBaseName = baseTypeName(target.code)
      if (targetBaseName == aliasName || target.code == aliasName) TypeConstants.Any else typeName(target)
    }
  }

  private def typeAliasTargetNode(typeAlias: KotlinAstNode): Option[KotlinAstNode] =
    typeAlias.children.dropWhile(_.kind != "=").drop(1).find(child => TypeNodeKinds.contains(child.kind))

  private def registerTypeDeclarationInfo(
    declaration: KotlinAstNode,
    packageName: Option[String],
    ownerTypeFullName: Option[String]
  ): Unit = {
    val name     = typeDeclarationName(declaration)
    val fullName = fullNameForTypeDeclaration(declaration, name, packageName, ownerTypeFullName)
    val bounds   = typeParameterBoundsFor(declaration, Map.empty)
    val members  = declaredMemberInfos(declaration, bounds)
    val methods = classBodyChildren(declaration).collect {
      case method if method.kind == "function_declaration" =>
        declaredMethodInfo(method, bounds)
    } ++ dataClassComponentMethodInfos(declaration, bounds)
    val typeInfo = TypeInfo(name, fullName, bounds.keys.toList, bounds, methods, members)
    typeDeclarationInfos.update(name, typeInfo)
    typeDeclarationInfos.update(fullName, typeInfo)
    classBodyChildren(declaration).foreach {
      case nested
          if nested.kind == "class_declaration" || nested.kind == "object_declaration" || nested.kind == "companion_object" =>
        registerTypeDeclarationInfo(nested, packageName, ownerTypeFullName = Some(fullName))
      case _ =>
    }
  }

  private def declaredMethodInfo(
    functionDeclaration: KotlinAstNode,
    inheritedBounds: Map[String, String]
  ): MethodDeclInfo = {
    val bounds         = inheritedBounds ++ typeParameterBoundsFor(functionDeclaration, inheritedBounds)
    val name           = firstChildCode(functionDeclaration, "simple_identifier").getOrElse("<anonymous>")
    val parameterTypes = functionParameterTypeFullNames(functionDeclaration, bounds)
    val returnType = explicitReturnTypeNode(functionDeclaration)
      .map(node => mapTypeName(node.code, bounds))
      .orElse(inferExpressionBodyType(functionDeclaration))
      .getOrElse(TypeConstants.Void)
    MethodDeclInfo(
      name,
      methodSignature(returnType, parameterTypes),
      parameterTypes.size,
      isPrivateMethod(functionDeclaration)
    )
  }

  private def declaredMemberInfos(declaration: KotlinAstNode, bounds: Map[String, String]): Map[String, String] = {
    val constructorMembers =
      primaryConstructorMemberInfos(declaration, bounds).map(member => member.name -> member.typeFullName)
    val propertyMembers = classBodyChildren(declaration).collect {
      case property if property.kind == "property_declaration" =>
        val variableDeclaration = property.children.find(_.kind == "variable_declaration").getOrElse(property)
        val name =
          firstChildCode(variableDeclaration, "simple_identifier").getOrElse(
            nameFromDeclarationCode(variableDeclaration.code)
          )
        val typeFullName = typeFromDirectChildren(variableDeclaration, bounds)
          .orElse(initializerNode(property).flatMap(typeForMemberInitializer(_, bounds)))
          .getOrElse(TypeConstants.Any)
        name -> registerType(typeFullName)
    }
    (constructorMembers ++ propertyMembers).toMap
  }

  private def primaryConstructorMemberInfos(
    declaration: KotlinAstNode,
    bounds: Map[String, String]
  ): List[MemberDeclInfo] =
    primaryConstructor(declaration).toList
      .flatMap(_.children.filter(child => child.kind == "class_parameter" && hasValOrVar(child)))
      .map { parameter =>
        val name = firstChildCode(parameter, "simple_identifier").getOrElse(nameFromDeclarationCode(parameter.code))
        val typeFullName = typeFromDirectChildren(parameter, bounds)
          .orElse(typeFromDeclarationText(parameter.code, bounds))
          .getOrElse(TypeConstants.Any)
        MemberDeclInfo(parameter, name, registerType(typeFullName))
      }

  private def dataClassComponentMethodInfos(
    declaration: KotlinAstNode,
    bounds: Map[String, String]
  ): List[MethodDeclInfo] =
    if (isDataClassDeclaration(declaration)) {
      primaryConstructorMemberInfos(declaration, bounds).zipWithIndex.map { case (member, index) =>
        MethodDeclInfo(
          s"${Constants.ComponentNPrefix}${index + 1}",
          methodSignature(member.typeFullName, Nil),
          parameterCount = 0,
          isPrivate = false
        )
      }
    } else {
      Nil
    }

  private def typeForMemberInitializer(expression: KotlinAstNode, bounds: Map[String, String]): Option[String] =
    expression.kind match {
      case "as_expression" =>
        expression.children
          .find(child => TypeNodeKinds.contains(child.kind))
          .map(node => mapTypeName(node.code, bounds))
      case _ => typeForExpression(expression, BodyContext(mutable.Map.empty, mutable.Map.empty, ""))
    }

  private def astForTypeDeclaration(
    classDeclaration: KotlinAstNode,
    packageName: Option[String],
    ownerTypeFullName: Option[String] = None
  ): Ast = {
    val name = typeDeclarationName(classDeclaration)
    val fullName =
      fullNameForTypeDeclaration(classDeclaration, name, packageName, ownerTypeFullName)
    registerType(fullName)
    importAliases.update(name, fullName)
    if (classDeclaration.kind == "companion_object") {
      ownerTypeFullName.foreach(owner => companionObjects.update(owner, CompanionInfo(name, fullName)))
    }

    withTypeParameterBoundsFor(classDeclaration) {
      val inherits = inheritsForTypeDeclaration(classDeclaration)
      inheritedTypesByFullName.update(fullName, inherits)
      val typeDecl = typeDeclNode(
        classDeclaration,
        name,
        fullName,
        document.relativeName,
        codeForTypeDeclaration(classDeclaration, name),
        ownerTypeFullName.map(_ => NodeTypes.TYPE_DECL).getOrElse(NodeTypes.NAMESPACE_BLOCK),
        ownerTypeFullName.getOrElse(namespaceAstParentFullName(packageName)),
        inherits
      )

      val constructorParams = primaryConstructor(classDeclaration).toList.flatMap(classParameterInfos)
      val constructorInfo   = constructorMethodInfo(fullName, constructorParams)
      constructorsByTypeAndArity.update((fullName, constructorParams.size), constructorInfo)
      val constructorAst = astForPrimaryConstructor(classDeclaration, fullName, constructorParams, constructorInfo)
      val constructorMembers = constructorParams
        .filter(_.declaresMember)
        .map(param =>
          Ast(memberNode(param.node, param.name, param.name, registerType(param.typeFullName)))
            .withChildren(param.annotations.map(astForAnnotationEntry))
        )
      val companionReceiverMember = Option.when(classDeclaration.kind == "companion_object") {
        val ownerType = ownerTypeFullName.getOrElse(TypeConstants.Any)
        Ast(
          memberNode(
            classDeclaration,
            Constants.CompanionObjectMemberName,
            Constants.CompanionObjectMemberName,
            registerType(ownerType)
          )
        )
      }

      val bodyChildren = classBodyChildren(classDeclaration)
      val memberAsts = bodyChildren.flatMap {
        case declaration if declaration.kind == "property_declaration" =>
          astsForPropertyDeclaration(declaration, fullName)
        case _ => Nil
      }
      val enumEntryMembers = enumEntries(classDeclaration).map { entry =>
        val name = firstChildCode(entry, "simple_identifier").getOrElse(entry.code.takeWhile(_ != '(').trim)
        Ast(memberNode(entry, name, name, fullName))
      }
      val ownerIsInterface = isInterfaceDeclaration(classDeclaration)
      val methodAsts = bodyChildren.collect {
        case declaration if declaration.kind == "function_declaration" =>
          astForFunctionDeclaration(
            declaration,
            packageName,
            ownerTypeFullName = Some(fullName),
            ownerIsInterface = ownerIsInterface
          )
      }
      val componentMethodAsts = dataClassComponentMethodAsts(classDeclaration, fullName, constructorParams)
      val secondaryConstructorAsts = bodyChildren.collect {
        case declaration if declaration.kind == "secondary_constructor" =>
          astForSecondaryConstructor(declaration, fullName, constructorInfo)
      }
      val innerTypeAsts = bodyChildren.collect {
        case declaration
            if declaration.kind == "class_declaration" || declaration.kind == "object_declaration" || declaration.kind == "companion_object" =>
          astForTypeDeclaration(declaration, packageName, ownerTypeFullName = Some(fullName))
      }

      val annotationAsts = annotationNodesFor(classDeclaration).map(astForAnnotationEntry)
      val modifierAsts   = typeDeclarationModifierNodes(classDeclaration).map(modifier => Ast(modifier))
      val children =
        constructorAst :: constructorMembers ++ memberAsts ++ companionReceiverMember.toList ++ enumEntryMembers ++ methodAsts ++ componentMethodAsts ++ secondaryConstructorAsts ++ annotationAsts ++ modifierAsts ++ innerTypeAsts
      val typeAst         = Ast(typeDecl).withChildren(children)
      val boundMethodAsts = methodAsts ++ componentMethodAsts
      bindErasedGenericMethodsToType(
        classDeclaration,
        bindMethodsToType(typeAst, typeDecl, constructorAst :: secondaryConstructorAsts ++ boundMethodAsts),
        typeDecl,
        boundMethodAsts
      )
    }
  }

  private def dataClassComponentMethodAsts(
    classDeclaration: KotlinAstNode,
    classFullName: String,
    constructorParams: List[ParameterInfo]
  ): List[Ast] =
    if (isDataClassDeclaration(classDeclaration)) {
      constructorParams.filter(_.declaresMember).zipWithIndex.map { case (param, index) =>
        val typeFullName = registerType(param.typeFullName)
        val thisParam = parameterInNode(
          classDeclaration,
          Constants.ThisName,
          Constants.ThisName,
          index = 0,
          isVariadic = false,
          evaluationStrategy = EvaluationStrategies.BY_SHARING,
          typeFullName = classFullName
        ).dynamicTypeHintFullName(Seq(classFullName))
        val thisIdentifier =
          identifierNode(param.node, Constants.ThisName, Constants.ThisName, classFullName, Seq(classFullName))
        val thisAst         = Ast(thisIdentifier).withRefEdge(thisIdentifier, thisParam)
        val fieldIdentifier = fieldIdentifierNode(param.node, param.name, param.name)
        val fieldAccessCall =
          operatorCallNode(
            param.node,
            s"${Constants.ThisName}.${param.name}",
            Operators.fieldAccess,
            Some(typeFullName)
          )
        val fieldAccessAst = callAst(fieldAccessCall, List(thisAst, Ast(fieldIdentifier)))
        val body = blockAst(
          blockNode(param.node, fieldAccessCall.code, typeFullName),
          List(returnAst(returnNode(param.node, Constants.RetCode), List(fieldAccessAst)))
        )
        val componentName = s"${Constants.ComponentNPrefix}${index + 1}"
        val signature     = methodSignature(typeFullName, Nil)
        val fullName      = methodFullName(s"$classFullName.$componentName", signature)
        methodsByOwnerNameAndArity
          .update((classFullName, componentName, 0), MethodInfo(fullName, signature, typeFullName))
        methodAst(
          methodNode(param.node, componentName, fullName, signature, document.relativeName),
          Seq(Ast(thisParam)),
          body,
          methodReturnNode(param.node, typeFullName)
        )
      }
    } else {
      Nil
    }

  private def astForPrimaryConstructor(
    classDeclaration: KotlinAstNode,
    classFullName: String,
    params: List[ParameterInfo],
    methodInfo: MethodInfo
  ): Ast = {
    val origin = primaryConstructor(classDeclaration).getOrElse(classDeclaration)
    val method = methodNode(
      origin,
      Defines.ConstructorMethodName,
      methodInfo.fullName,
      methodInfo.signature,
      document.relativeName
    )
    val thisParam = parameterInNode(
      origin,
      "this",
      "this",
      index = 0,
      isVariadic = false,
      evaluationStrategy = EvaluationStrategies.BY_SHARING,
      typeFullName = classFullName
    ).dynamicTypeHintFullName(Seq(classFullName))
    val paramNodes = params.zipWithIndex.map { case (param, index) =>
      parameterInNode(
        param.node,
        param.name,
        param.name,
        index + 1,
        isVariadic = false,
        evaluationStrategy = EvaluationStrategies.BY_VALUE,
        typeFullName = param.typeFullName
      )
    }
    val paramAsts = Ast(thisParam) +: params.zip(paramNodes).map { case (param, paramNode) =>
      Ast(paramNode).withChildren(param.annotations.map(astForAnnotationEntry))
    }
    val bodyContext = BodyContext(
      mutable.Map.from(params.map(param => param.name -> param.typeFullName) :+ ("this" -> classFullName)),
      mutable.Map.from(paramNodes.map(param => param.name -> param) :+ ("this"          -> thisParam)),
      methodInfo.fullName,
      collectionElementTypes = collectionElementTypesForParams(params),
      mapKeyTypes = mapKeyTypesForParams(params),
      mapValueTypes = mapValueTypesForParams(params),
      mapEntryKeyTypes = mapEntryKeyTypesForParams(params),
      mapEntryValueTypes = mapEntryValueTypesForParams(params),
      pairFirstTypes = pairFirstTypesForParams(params),
      pairSecondTypes = pairSecondTypesForParams(params),
      tripleFirstTypes = tripleFirstTypesForParams(params),
      tripleSecondTypes = tripleSecondTypesForParams(params),
      tripleThirdTypes = tripleThirdTypesForParams(params)
    )
    val superConstructorCall = primarySuperConstructorInvocation(classDeclaration).map { invocation =>
      astForPrimarySuperConstructorCall(invocation, classFullName, bodyContext)
    }
    val memberSetCalls = params.filter(_.declaresMember).map(param => memberSetCallAst(param, bodyContext))
    val memberInitializerSetCalls =
      classBodyChildren(classDeclaration).flatMap(propertyInitializerSetCallAst(_, bodyContext))
    val initAsts = classBodyChildren(classDeclaration)
      .filter(_.kind == "anonymous_initializer")
      .flatMap(astsForAnonymousInitializer(_, bodyContext))
    val body =
      blockAst(
        blockNode(origin, "", TypeConstants.Void),
        superConstructorCall.toList ++ memberSetCalls ++ memberInitializerSetCalls ++ initAsts
      )
    val constructorAnnotations =
      if (origin.kind == "primary_constructor") annotationNodesFor(origin).map(astForAnnotationEntry) else Nil
    methodAstWithAnnotations(
      method,
      paramAsts,
      body,
      methodReturnNode(origin, TypeConstants.Void),
      Seq(modifierNode(origin, ModifierTypes.CONSTRUCTOR)),
      constructorAnnotations
    )
  }

  private def astForPrimarySuperConstructorCall(
    constructorInvocation: KotlinAstNode,
    classFullName: String,
    context: BodyContext
  ): Ast = {
    val superTypeFullName = constructorInvocation.children
      .find(_.kind == "user_type")
      .map(typeName)
      .getOrElse(inheritedTypesByFullName.getOrElse(classFullName, Nil).headOption.getOrElse(TypeConstants.Any))
    val argumentNodes         = valueArgumentNodes(constructorInvocation)
    val argumentTypeFullNames = argumentNodes.map(typeForExpression(_, context).getOrElse(TypeConstants.Any))
    val methodInfo = constructorsByTypeAndArity.get(superTypeFullName -> argumentNodes.size).getOrElse {
      val signature = methodSignature(TypeConstants.Void, argumentTypeFullNames)
      MethodInfo(
        methodFullName(s"$superTypeFullName.${Defines.ConstructorMethodName}", signature),
        signature,
        TypeConstants.Void
      )
    }
    val receiver =
      identifierNode(constructorInvocation, Constants.ThisName, Constants.ThisName, registerType(superTypeFullName))
    val receiverAst = context.refs
      .get(Constants.ThisName)
      .map(target => Ast(receiver).withRefEdge(receiver, target))
      .getOrElse(Ast(receiver))
    val call = callNode(
      constructorInvocation,
      constructorInvocation.code,
      Defines.ConstructorMethodName,
      methodInfo.fullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(methodInfo.signature),
      Some(TypeConstants.Void)
    )
    callAst(call, argumentNodes.map(astForExpression(_, context)), Some(receiverAst))
  }

  private def astsForPropertyDeclaration(propertyDeclaration: KotlinAstNode, ownerTypeFullName: String): List[Ast] = {
    val declaration = propertyDeclaration.children.find(_.kind == "variable_declaration").getOrElse(propertyDeclaration)
    val name = firstChildCode(declaration, "simple_identifier").getOrElse(nameFromDeclarationCode(declaration.code))
    initializerNode(propertyDeclaration) match {
      case Some(objectLiteral) if objectLiteral.kind == "object_literal" =>
        val objectOwnerName = s"$ownerTypeFullName.$name"
        val typeDeclAst = astForObjectLiteralTypeDeclaration(
          objectLiteral,
          BodyContext(mutable.Map.empty, mutable.Map.empty, ""),
          ownerNameOverride = Some(objectOwnerName),
          parentTypeOverride = Some(NodeTypes.TYPE_DECL),
          parentNameOverride = Some(ownerTypeFullName)
        )
        val typeFullName = typeDeclAst.root.collect { case typeDecl: NewTypeDecl => typeDecl.fullName }.get
        List(typeDeclAst, astForPropertyMember(propertyDeclaration, name, typeFullName))
      case initializer =>
        val typeName = typeFromDirectChildren(declaration)
          .orElse(initializer.flatMap(typeForExpression(_, BodyContext(mutable.Map.empty, mutable.Map.empty, ""))))
          .getOrElse(TypeConstants.Any)
        List(astForPropertyMember(propertyDeclaration, name, typeName))
    }
  }

  private def astForPropertyMember(propertyDeclaration: KotlinAstNode, name: String, typeFullName: String): Ast =
    Ast(memberNode(propertyDeclaration, name, name, registerType(typeFullName)))
      .withChildren(annotationNodesFor(propertyDeclaration).map(astForAnnotationEntry))

  private def memberSetCallAst(param: ParameterInfo, context: BodyContext): Ast =
    memberAssignmentAst(
      param.node,
      param.name,
      param.typeFullName,
      identifierAstForName(param.node, param.name, param.typeFullName, context),
      param.name,
      context
    )

  private def propertyInitializerSetCallAst(propertyDeclaration: KotlinAstNode, context: BodyContext): List[Ast] =
    if (propertyDeclaration.kind != "property_declaration") {
      Nil
    } else {
      val declaration =
        propertyDeclaration.children.find(_.kind == "variable_declaration").getOrElse(propertyDeclaration)
      val name = firstChildCode(declaration, "simple_identifier").getOrElse(nameFromDeclarationCode(declaration.code))
      initializerNode(propertyDeclaration)
        .filterNot(_.kind == "object_literal")
        .map { initializer =>
          val typeFullName = typeFromDirectChildren(declaration)
            .orElse(typeForExpression(initializer, context))
            .getOrElse(TypeConstants.Any)
          memberAssignmentAst(
            propertyDeclaration,
            name,
            typeFullName,
            astForExpression(initializer, context, Some(typeFullName)),
            initializer.code,
            context
          )
        }
        .toList
    }

  private def memberAssignmentAst(
    origin: KotlinAstNode,
    memberName: String,
    typeFullName: String,
    rhsAst: Ast,
    rhsCode: String,
    context: BodyContext
  ): Ast = {
    val thisAst         = thisIdentifierAst(origin, Constants.ThisName, context)
    val fieldIdentifier = fieldIdentifierNode(origin, memberName, memberName)
    val fieldAccessAst = callAst(
      operatorCallNode(
        origin,
        s"${Constants.ThisName}.$memberName",
        Operators.fieldAccess,
        Some(registerType(typeFullName))
      ),
      List(thisAst, Ast(fieldIdentifier))
    )
    callAst(
      operatorCallNode(
        origin,
        s"${Constants.ThisName}.$memberName = $rhsCode",
        Operators.assignment,
        Some(TypeConstants.Any)
      ),
      List(fieldAccessAst, rhsAst)
    )
  }

  private def registerTopLevelMethodInfo(functionDeclaration: KotlinAstNode, packageName: Option[String]): Unit = {
    val name      = firstChildCode(functionDeclaration, "simple_identifier").getOrElse("<anonymous>")
    val params    = functionParameters(functionDeclaration)
    val returnTyp = registerType(returnTypeForFunction(functionDeclaration))
    val signature = methodSignature(returnTyp, params.map(_.typeFullName))
    val descFullName = packageName match {
      case Some(pkg) if pkg.nonEmpty => s"$pkg.$name"
      case _                         => name
    }
    topLevelMethodsByNameAndArity.update(
      (name, params.size),
      MethodInfo(methodFullName(descFullName, signature), signature, returnTyp)
    )
  }

  private def astForFunctionDeclaration(
    functionDeclaration: KotlinAstNode,
    packageName: Option[String],
    ownerTypeFullName: Option[String] = None,
    ownerIsInterface: Boolean = false
  ): Ast = withTypeParameterBoundsFor(functionDeclaration) {
    val name       = firstChildCode(functionDeclaration, "simple_identifier").getOrElse("<anonymous>")
    val params     = functionParameters(functionDeclaration)
    val returnType = registerType(returnTypeForFunction(functionDeclaration))
    val signature  = methodSignature(returnType, params.map(_.typeFullName))
    val descFullName =
      ownerTypeFullName.map(owner => s"$owner.$name").getOrElse(packageName.map(pkg => s"$pkg.$name").getOrElse(name))
    val fullName = methodFullName(descFullName, signature)
    val method   = methodNode(functionDeclaration, name, fullName, signature, document.relativeName)
    methodsByFullName.update(fullName, method)
    ownerTypeFullName.foreach { owner =>
      methodsByOwnerNameAndArity.update(
        (owner, name, params.size),
        MethodInfo(fullName, signature, returnType, isPrivateMethod(functionDeclaration))
      )
    }
    if (ownerTypeFullName.isEmpty) {
      topLevelMethodsByNameAndArity.update((name, params.size), MethodInfo(fullName, signature, returnType))
    }

    val thisParamNodes = ownerTypeFullName.toList.map { owner =>
      parameterInNode(
        functionDeclaration,
        "this",
        "this",
        index = 0,
        isVariadic = false,
        evaluationStrategy = EvaluationStrategies.BY_SHARING,
        typeFullName = owner
      ).dynamicTypeHintFullName(Seq(owner))
    }
    val paramNodes = params.zipWithIndex.map { case (param, index) =>
      parameterInNode(
        param.node,
        param.name,
        param.name,
        index + 1,
        isVariadic = false,
        evaluationStrategy = EvaluationStrategies.BY_VALUE,
        typeFullName = param.typeFullName
      )
    }
    val thisParamAsts = thisParamNodes.map(Ast(_))
    val paramAsts = params.zip(paramNodes).map { case (param, paramNode) =>
      Ast(paramNode).withChildren(param.annotations.map(astForAnnotationEntry))
    }
    val capturedGlobals = referencedTopLevelGlobals(functionDeclaration, params.map(_.name).toSet, ownerTypeFullName)
    val capturedLocals = capturedGlobals.map { global =>
      localNode(functionDeclaration, global.name, global.name, global.typeFullName, Some(s"$fullName:${global.name}"))
    }
    val bodyContext = BodyContext(
      mutable.Map.from(
        params.map(param => param.name -> param.typeFullName) ++
          ownerTypeFullName.map("this" -> _) ++
          capturedGlobals.map(global => global.name -> global.typeFullName)
      ),
      mutable.Map.from(
        paramNodes.map(param => param.name -> param) ++
          thisParamNodes.map(thisParam => thisParam.name -> thisParam) ++
          capturedGlobals.zip(capturedLocals).map { case (global, local) => global.name -> local }
      ),
      fullName,
      collectionElementTypes = collectionElementTypesForParams(params),
      mapKeyTypes = mapKeyTypesForParams(params),
      mapValueTypes = mapValueTypesForParams(params),
      mapEntryKeyTypes = mapEntryKeyTypesForParams(params),
      mapEntryValueTypes = mapEntryValueTypesForParams(params),
      pairFirstTypes = pairFirstTypesForParams(params),
      pairSecondTypes = pairSecondTypesForParams(params),
      tripleFirstTypes = tripleFirstTypesForParams(params),
      tripleSecondTypes = tripleSecondTypesForParams(params),
      tripleThirdTypes = tripleThirdTypesForParams(params)
    )
    val body = astForFunctionBody(functionDeclaration, bodyContext, capturedLocals.map(Ast(_)))
    val modifiers = methodModifierNodes(
      functionDeclaration,
      withVirtualModifier = ownerTypeFullName.nonEmpty,
      isAbstract = ownerIsInterface || hasKotlinModifier(functionDeclaration, "abstract")
    )
    val captureRefAst = astForCapturedGlobalsMethodRef(functionDeclaration, name, fullName, capturedGlobals)
    methodAstWithAnnotations(
      method,
      thisParamAsts ++ paramAsts,
      body,
      methodReturnNode(functionDeclaration, returnType),
      modifiers,
      annotationNodesFor(functionDeclaration).map(astForAnnotationEntry)
    ).withChildren(captureRefAst.toSeq)
  }

  private def astForSecondaryConstructor(
    constructor: KotlinAstNode,
    classFullName: String,
    primaryConstructorInfo: MethodInfo
  ): Ast = {
    val params     = functionParameters(constructor)
    val methodInfo = constructorMethodInfo(classFullName, params)
    constructorsByTypeAndArity.update((classFullName, params.size), methodInfo)
    val method = methodNode(
      constructor,
      Defines.ConstructorMethodName,
      methodInfo.fullName,
      methodInfo.signature,
      document.relativeName
    )
    val thisParam = parameterInNode(
      constructor,
      "this",
      "this",
      index = 0,
      isVariadic = false,
      evaluationStrategy = EvaluationStrategies.BY_SHARING,
      typeFullName = classFullName
    ).dynamicTypeHintFullName(Seq(classFullName))
    val paramNodes = params.zipWithIndex.map { case (param, index) =>
      parameterInNode(
        param.node,
        param.name,
        param.name,
        index + 1,
        isVariadic = false,
        evaluationStrategy = EvaluationStrategies.BY_VALUE,
        typeFullName = param.typeFullName
      )
    }
    val paramAsts = Ast(thisParam) +: params.zip(paramNodes).map { case (param, paramNode) =>
      Ast(paramNode).withChildren(param.annotations.map(astForAnnotationEntry))
    }
    val bodyContext = BodyContext(
      mutable.Map.from(params.map(param => param.name -> param.typeFullName) :+ ("this" -> classFullName)),
      mutable.Map.from(paramNodes.map(param => param.name -> param) :+ ("this"          -> thisParam)),
      methodInfo.fullName,
      collectionElementTypes = collectionElementTypesForParams(params),
      mapKeyTypes = mapKeyTypesForParams(params),
      mapValueTypes = mapValueTypesForParams(params),
      mapEntryKeyTypes = mapEntryKeyTypesForParams(params),
      mapEntryValueTypes = mapEntryValueTypesForParams(params),
      pairFirstTypes = pairFirstTypesForParams(params),
      pairSecondTypes = pairSecondTypesForParams(params),
      tripleFirstTypes = tripleFirstTypesForParams(params),
      tripleSecondTypes = tripleSecondTypesForParams(params),
      tripleThirdTypes = tripleThirdTypesForParams(params)
    )
    val delegationAst =
      constructor.children.find(_.kind == "constructor_delegation_call").map { delegation =>
        astForConstructorDelegationCall(delegation, primaryConstructorInfo, bodyContext)
      }
    val bodyAsts = statementsChild(constructor)
      .map(_.children.filter(_.named).flatMap(astForStatement(_, bodyContext)))
      .getOrElse(Nil)
    val body = blockAst(blockNode(constructor, constructor.code, TypeConstants.Void), delegationAst.toList ++ bodyAsts)

    methodAstWithAnnotations(
      method,
      paramAsts,
      body,
      methodReturnNode(constructor, TypeConstants.Void),
      Seq(modifierNode(constructor, ModifierTypes.CONSTRUCTOR)),
      annotationNodesFor(constructor).map(astForAnnotationEntry)
    )
  }

  private def astForConstructorDelegationCall(
    delegationCall: KotlinAstNode,
    primaryConstructorInfo: MethodInfo,
    context: BodyContext
  ): Ast = {
    val args = valueArgumentNodes(delegationCall).map(astForExpression(_, context))
    val call = callNode(
      delegationCall,
      delegationCall.code,
      Defines.ConstructorMethodName,
      primaryConstructorInfo.fullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(primaryConstructorInfo.signature),
      Some(TypeConstants.Void)
    )
    callAst(call, args)
  }

  private def referencedTopLevelGlobals(
    functionDeclaration: KotlinAstNode,
    declaredParams: Set[String],
    ownerTypeFullName: Option[String]
  ): List[GlobalInfo] = {
    if (ownerTypeFullName.nonEmpty || topLevelGlobals.isEmpty) {
      Nil
    } else {
      val functionBody = functionDeclaration.children.find(_.kind == "function_body").toList
      val localDeclarations = functionBody
        .flatMap(body => body :: body.descendants)
        .filter(_.kind == "property_declaration")
        .flatMap(_.children.find(_.kind == "variable_declaration"))
        .flatMap(declaration => firstChildCode(declaration, "simple_identifier"))
        .toSet
      val referencedNames = functionBody
        .flatMap(body => body :: body.descendants)
        .collect { case node if node.kind == "simple_identifier" => node.code }
        .toSet
      (referencedNames -- declaredParams -- localDeclarations).toList.sorted.flatMap(topLevelGlobals.get)
    }
  }

  private def astForCapturedGlobalsMethodRef(
    functionDeclaration: KotlinAstNode,
    name: String,
    fullName: String,
    capturedGlobals: List[GlobalInfo]
  ): Option[Ast] = {
    if (capturedGlobals.isEmpty) {
      None
    } else {
      val methodRef = methodRefNode(functionDeclaration, name, fullName, fullName.takeWhile(_ != ':'))
      Some(capturedGlobals.foldLeft(Ast(methodRef)) { case (ast, global) =>
        val closureBinding = closureBindingNode(s"$fullName:${global.name}", EvaluationStrategies.BY_REFERENCE)
        ast
          .merge(Ast(closureBinding))
          .withCaptureEdge(methodRef, closureBinding)
          .withRefEdge(closureBinding, global.local)
      })
    }
  }

  private def astForFunctionBody(
    functionDeclaration: KotlinAstNode,
    context: BodyContext,
    prefixAsts: List[Ast] = Nil
  ): Ast = {
    val body = functionDeclaration.children.find(_.kind == "function_body")
    body match {
      case Some(functionBody) =>
        val statements = statementChildren(functionBody).flatMap(astForStatement(_, context))
        if (statements.nonEmpty) {
          blockAst(blockNode(functionBody, functionBody.code, TypeConstants.Any), prefixAsts ++ statements)
        } else {
          val expressionBody = expressionBodyNode(functionBody)
          val expressionStatements = expressionBody.toList.map { expression =>
            returnAst(returnNode(expression, s"return ${expression.code}"), Seq(astForExpression(expression, context)))
          }
          blockAst(blockNode(functionBody, functionBody.code, TypeConstants.Any), prefixAsts ++ expressionStatements)
        }
      case None =>
        blockAst(blockNode(functionDeclaration, "<empty>", TypeConstants.Any), prefixAsts)
    }
  }

  private def astForStatement(statement: KotlinAstNode, context: BodyContext): List[Ast] = {
    statement.kind match {
      case "function_declaration" =>
        astsForLocalFunctionDeclaration(statement, context)
      case "class_declaration" | "object_declaration" =>
        List(astForLocalTypeDeclaration(statement, context))
      case "property_declaration" =>
        astsForLocalPropertyDeclaration(statement, context)
      case "if_expression" =>
        List(astForIfExpression(statement, context))
      case "while_statement" =>
        List(astForWhileStatement(statement, context))
      case "for_statement" =>
        List(astForForStatement(statement, context))
      case "when_expression" =>
        List(astForWhenExpression(statement, context))
      case "try_expression" =>
        List(astForTryAsStatement(statement, context))
      case "label" =>
        List(astForJumpTarget(statement))
      case "jump_expression" if hasReturnKeyword(statement) =>
        List(astForReturn(statement, context))
      case "jump_expression" if hasThrowKeyword(statement) =>
        List(astForThrow(statement, context))
      case "jump_expression" if hasBreakKeyword(statement) =>
        List(astForBreakOrContinue(statement, ControlStructureTypes.BREAK))
      case "jump_expression" if hasContinueKeyword(statement) =>
        List(astForBreakOrContinue(statement, ControlStructureTypes.CONTINUE))
      case _ if statement.named =>
        List(astForExpression(statement, context))
      case _ =>
        Nil
    }
  }

  private def astsForLocalFunctionDeclaration(functionDeclaration: KotlinAstNode, context: BodyContext): List[Ast] =
    withTypeParameterBoundsFor(functionDeclaration) {
      val name       = firstChildCode(functionDeclaration, "simple_identifier").getOrElse("<anonymous>")
      val params     = functionParameters(functionDeclaration)
      val returnType = registerType(returnTypeForFunction(functionDeclaration))
      val signature  = methodSignature(returnType, params.map(_.typeFullName))
      val descName   = s"${methodBaseFullName(context.ownerMethodFullName)}.$name"
      val fullName   = methodFullName(descName, signature)
      val methodInfo = MethodInfo(fullName, signature, returnType)
      context.methods.update((name, params.size), methodInfo)

      val method = methodNode(functionDeclaration, name, fullName, signature, document.relativeName)
      methodsByFullName.update(fullName, method)
      val bodyContext = BodyContext(
        mutable.Map.from(context.types),
        mutable.Map.from(context.refs),
        fullName,
        mutable.Map.from(context.methods),
        mutable.Map.from(context.collectionElementTypes),
        mutable.Map.from(context.iteratorElementTypes),
        mutable.Map.from(context.mapKeyTypes),
        mutable.Map.from(context.mapValueTypes),
        mutable.Map.from(context.mapEntryKeyTypes),
        mutable.Map.from(context.mapEntryValueTypes),
        pairFirstTypes = mutable.Map.from(context.pairFirstTypes),
        pairSecondTypes = mutable.Map.from(context.pairSecondTypes),
        tripleFirstTypes = mutable.Map.from(context.tripleFirstTypes),
        tripleSecondTypes = mutable.Map.from(context.tripleSecondTypes),
        tripleThirdTypes = mutable.Map.from(context.tripleThirdTypes)
      )
      val paramNodes = params.zipWithIndex.map { case (param, index) =>
        val paramNode = parameterInNode(
          param.node,
          param.name,
          param.name,
          index + 1,
          isVariadic = false,
          evaluationStrategy = EvaluationStrategies.BY_VALUE,
          typeFullName = param.typeFullName
        )
        bodyContext.types.update(param.name, param.typeFullName)
        bodyContext.refs.update(param.name, paramNode)
        updateParameterTypeMetadata(param, bodyContext)
        paramNode
      }
      val paramAsts = params.zip(paramNodes).map { case (param, paramNode) =>
        Ast(paramNode).withChildren(param.annotations.map(astForAnnotationEntry))
      }
      val methodAst = methodAstWithAnnotations(
        method,
        paramAsts,
        astForFunctionBody(functionDeclaration, bodyContext),
        methodReturnNode(functionDeclaration, returnType),
        Seq.empty,
        annotationNodesFor(functionDeclaration).map(astForAnnotationEntry)
      )
      List(astForFunctionTypeDecl(functionDeclaration, name, fullName, signature, method), methodAst)
    }

  private def astForFunctionTypeDecl(
    functionDeclaration: KotlinAstNode,
    name: String,
    fullName: String,
    signature: String,
    method: NewMethod
  ): Ast = {
    val typeDecl =
      typeDeclNode(functionDeclaration, name, fullName, document.relativeName, name, NodeTypes.METHOD, fullName)
    val binding = bindingNode(name, signature, method.fullName)
    Ast(typeDecl)
      .merge(Ast(binding))
      .withBindsEdge(typeDecl, binding)
      .withRefEdge(binding, method)
  }

  private def astForLocalTypeDeclaration(typeDeclaration: KotlinAstNode, context: BodyContext): Ast =
    astForTypeDeclaration(
      typeDeclaration,
      sourcePackageName,
      ownerTypeFullName = Some(methodBaseFullName(context.ownerMethodFullName))
    )

  private def astsForLocalPropertyDeclaration(propertyDeclaration: KotlinAstNode, context: BodyContext): List[Ast] = {
    propertyDeclaration.children.find(_.kind == "multi_variable_declaration") match {
      case Some(destructuring) =>
        astsForDestructuringDeclaration(propertyDeclaration, destructuring, context)
      case None =>
        val declaration =
          propertyDeclaration.children.find(_.kind == "variable_declaration").getOrElse(propertyDeclaration)
        val name = firstChildCode(declaration, "simple_identifier").getOrElse(nameFromDeclarationCode(declaration.code))
        val initializer = initializerNode(propertyDeclaration)
        val specializedAsts = initializer match {
          case Some(objectLiteral) if objectLiteral.kind == "object_literal" =>
            Some(astsForLocalObjectLiteralProperty(propertyDeclaration, name, objectLiteral, context))
          case Some(callExpression) if callExpression.kind == "call_expression" =>
            constructorInfoForCallExpression(callExpression, context)
              .map { constructorInfo =>
                astsForLocalConstructorProperty(propertyDeclaration, name, callExpression, constructorInfo, context)
              }
          case _ =>
            None
        }

        specializedAsts.getOrElse {
          val typeName = typeFromDirectChildren(declaration)
            .orElse(initializer.flatMap(typeForExpression(_, context)))
            .getOrElse(TypeConstants.Any)
          val typeFullName = registerType(typeName)
          val local        = localNode(propertyDeclaration, name, name, typeFullName)
          val rhsAst       = initializer.map(astForExpression(_, context, Some(typeFullName)))

          context.types.update(name, typeFullName)
          context.refs.update(name, local)
          updateCollectionElementType(
            name,
            collectionElementTypeFromDirectChildren(declaration).orElse(
              collectionElementTypeFromDeclarationText(declaration.code)
            ),
            initializer,
            context,
            declaredPairTypes =
              pairTypesFromDirectChildren(declaration).orElse(pairTypesFromDeclarationText(declaration.code)),
            declaredTripleTypes =
              tripleTypesFromDirectChildren(declaration).orElse(tripleTypesFromDeclarationText(declaration.code))
          )

          val localAst = Ast(local).withChildren(annotationNodesFor(propertyDeclaration).map(astForAnnotationEntry))
          val assignmentAst = rhsAst.map { rhs =>
            val target    = identifierNode(propertyDeclaration, name, name, typeFullName)
            val targetAst = Ast(target).withRefEdge(target, local)
            callAst(
              operatorCallNode(propertyDeclaration, propertyDeclaration.code, Operators.assignment, Some(typeFullName)),
              Seq(targetAst, rhs)
            )
          }
          localAst :: assignmentAst.toList
        }
    }
  }

  private def astsForLocalConstructorProperty(
    propertyDeclaration: KotlinAstNode,
    name: String,
    callExpression: KotlinAstNode,
    constructorInfo: MethodInfo,
    context: BodyContext
  ): List[Ast] = {
    val typeFullName = registerType(constructorInfo.returnTypeFullName)
    val local        = localNode(propertyDeclaration, name, name, typeFullName)
    val localAst     = Ast(local).withChildren(annotationNodesFor(propertyDeclaration).map(astForAnnotationEntry))
    context.types.update(name, typeFullName)
    context.refs.update(name, local)
    val declaration = propertyDeclaration.children.find(_.kind == "variable_declaration").getOrElse(propertyDeclaration)
    updateCollectionElementType(
      name,
      collectionElementTypeFromDirectChildren(declaration).orElse(
        collectionElementTypeFromDeclarationText(declaration.code)
      ),
      Some(callExpression),
      context,
      declaredPairTypes =
        pairTypesFromDirectChildren(declaration).orElse(pairTypesFromDeclarationText(declaration.code)),
      declaredTripleTypes =
        tripleTypesFromDirectChildren(declaration).orElse(tripleTypesFromDeclarationText(declaration.code))
    )

    val lhs              = identifierNode(propertyDeclaration, name, name, typeFullName)
    val assignmentTarget = Ast(lhs).withRefEdge(lhs, local)
    val alloc            = operatorCallNode(callExpression, Operators.alloc, Operators.alloc, Some(typeFullName))
    val assignment = callAst(
      operatorCallNode(propertyDeclaration, s"$name = <alloc>", Operators.assignment, Some(typeFullName)),
      Seq(assignmentTarget, Ast(alloc))
    )
    val receiver    = identifierNode(callExpression, name, name, typeFullName)
    val receiverAst = Ast(receiver).withRefEdge(receiver, local)
    val initCall = callNode(
      callExpression,
      callExpression.code,
      Defines.ConstructorMethodName,
      constructorInfo.fullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(constructorInfo.signature),
      Some(TypeConstants.Void)
    )
    val argumentContext = constructorArgumentContext(constructorInfo, context)
    val initAst =
      callAst(initCall, callArgumentNodes(callExpression).map(astForExpression(_, argumentContext)), Some(receiverAst))
    List(localAst, assignment, initAst)
  }

  private def astsForLocalObjectLiteralProperty(
    propertyDeclaration: KotlinAstNode,
    name: String,
    objectLiteral: KotlinAstNode,
    context: BodyContext
  ): List[Ast] = {
    val typeDeclAst      = astForObjectLiteralTypeDeclaration(objectLiteral, context)
    val typeFullName     = typeDeclAst.root.collect { case typeDecl: NewTypeDecl => typeDecl.fullName }.get
    val local            = localNode(propertyDeclaration, name, name, typeFullName)
    val localAst         = Ast(local).withChildren(annotationNodesFor(propertyDeclaration).map(astForAnnotationEntry))
    val lhs              = identifierNode(propertyDeclaration, name, name, typeFullName)
    val assignmentTarget = Ast(lhs).withRefEdge(lhs, local)
    val alloc            = operatorCallNode(objectLiteral, Operators.alloc, Operators.alloc, Some(typeFullName))
    val assignment = callAst(
      operatorCallNode(propertyDeclaration, propertyDeclaration.code, Operators.assignment, Some(typeFullName)),
      Seq(assignmentTarget, Ast(alloc))
    )
    val initAst = astForObjectLiteralInitCall(objectLiteral, name, local, typeFullName)

    context.types.update(name, typeFullName)
    context.refs.update(name, local)

    List(typeDeclAst, localAst, assignment, initAst)
  }

  private def astsForDestructuringDeclaration(
    declaration: KotlinAstNode,
    destructuring: KotlinAstNode,
    context: BodyContext
  ): List[Ast] = {
    val entries = destructuringEntries(destructuring)
    initializerNode(declaration) match {
      case None =>
        entries.flatMap { case DestructuringEntry(entryNode, name, _, typeFullName) =>
          if (name == Constants.UnusedDestructuringEntryText) {
            Nil
          } else {
            val local = localNode(entryNode, name, name, typeFullName)
            context.types.update(name, typeFullName)
            context.refs.update(name, local)
            List(Ast(local))
          }
        }
      case Some(initializer) =>
        val base =
          if (initializer.kind == "simple_identifier") {
            DestructuringBase(
              initializer.code,
              typeForExpression(initializer, context).getOrElse(TypeConstants.Any),
              Nil
            )
          } else {
            val tmpName     = nextTmpLocalName()
            val tmpTypeName = typeForExpression(initializer, context).getOrElse(TypeConstants.Any)
            val tmpLocal    = localNode(declaration, tmpName, tmpName, registerType(tmpTypeName))
            context.types.update(tmpName, tmpLocal.typeFullName)
            context.refs.update(tmpName, tmpLocal)
            updateMapEntryTypesFromExpression(tmpName, initializer, context)
            updatePairTypesFromExpression(tmpName, initializer, context)
            updateTripleTypesFromExpression(tmpName, initializer, context)
            val tmpLhs = identifierAstForName(declaration, tmpName, tmpLocal.typeFullName, context)
            val tmpAssignment = callAst(
              operatorCallNode(
                declaration,
                s"$tmpName = ${initializer.code}",
                Operators.assignment,
                Some(TypeConstants.Any)
              ),
              Seq(tmpLhs, astForExpression(initializer, context))
            )
            DestructuringBase(tmpName, tmpLocal.typeFullName, List(Ast(tmpLocal), tmpAssignment))
          }

        val typedEntries = destructuringEntriesWithComponentTypes(entries, base, context)
        val entryLocals  = localAstsForDestructuringEntries(typedEntries, context)
        entryLocals ++ base.prologueAsts ++ assignmentAstsForDestructuringEntries(typedEntries, base, context)
    }
  }

  private def astForReturn(jumpExpression: KotlinAstNode, context: BodyContext): Ast = {
    val arguments =
      jumpExpression.children
        .filterNot(child => child.kind == "return" || child.kind == "label")
        .filter(_.named)
        .take(1)
        .map(astForExpression(_, context))
    val returnNode_ = returnNode(jumpExpression, jumpExpression.code)
    val labelAst    = jumpLabelAst(jumpExpression)
    val ast         = returnAst(returnNode_, arguments).withChildren(labelAst.toList)
    labelAst.flatMap(_.root) match {
      case Some(labelRoot) => ast.withJumpArgumentEdge(returnNode_, labelRoot)
      case None            => ast
    }
  }

  private def astForExpression(
    expression: KotlinAstNode,
    context: BodyContext,
    expectedTypeFullName: Option[String] = None
  ): Ast = {
    expression.kind match {
      case "additive_expression" =>
        val operatorName = expression.children.find(child => child.kind == "+" || child.kind == "-").map(_.kind) match {
          case Some("-") => Operators.subtraction
          case _         => Operators.addition
        }
        callAst(
          operatorCallNode(
            expression,
            expression.code,
            operatorName,
            Some(
              registerType(
                additiveExpressionTypeFullName(expression, context, operatorName).getOrElse(TypeConstants.Any)
              )
            )
          ),
          expression.children.filter(_.named).map(astForExpression(_, context))
        )
      case "multiplicative_expression" =>
        astForMultiplicativeExpression(expression, context)
      case "comparison_expression" =>
        astForComparisonExpression(expression, context)
      case "equality_expression" =>
        astForComparisonExpression(expression, context)
      case "check_expression" =>
        astForCheckExpression(expression, context)
      case "as_expression" =>
        astForAsExpression(expression, context)
      case "conjunction_expression" =>
        astForBinaryOperatorExpression(expression, context, Operators.logicalAnd, "boolean")
      case "disjunction_expression" =>
        astForBinaryOperatorExpression(expression, context, Operators.logicalOr, "boolean")
      case "elvis_expression" =>
        astForBinaryOperatorExpression(
          expression,
          context,
          Operators.elvis,
          typeForElvisExpression(expression, context)
        )
      case "range_expression" =>
        val typeFullName = rangeExpressionTypeFullName(expression, context).getOrElse(TypeConstants.Any)
        callAst(
          operatorCallNode(expression, expression.code, Operators.range, Some(registerType(typeFullName))),
          expression.children.filter(_.named).map(astForExpression(_, context))
        )
      case "infix_expression" =>
        astForInfixExpression(expression, context)
      case "prefix_expression" if directAnnotationNodesFor(expression).nonEmpty =>
        astForAnnotatedExpression(expression, context)
      case "prefix_expression" =>
        astForPrefixExpression(expression, context)
      case "postfix_expression" =>
        astForPostfixExpression(expression, context)
      case "parenthesized_expression" =>
        expression.children
          .find(_.named)
          .map(astForExpression(_, context))
          .getOrElse(Ast(unknownNode(expression, expression.code)))
      case "if_expression" =>
        astForIfAsExpression(expression, context)
      case "when_expression" =>
        astForWhenAsExpression(expression, context)
      case "try_expression" =>
        astForTryAsExpression(expression, context)
      case "annotated_lambda" =>
        expression.children
          .find(_.kind == "lambda_literal")
          .map(astForExpression(_, context))
          .getOrElse(Ast(unknownNode(expression, expression.code)))
      case "lambda_literal" =>
        astForLambdaLiteral(expression, context)
      case "anonymous_function" =>
        astForAnonymousFunction(expression, context)
      case "object_literal" =>
        astForObjectLiteralExpression(expression, context)
      case "when_subject" | "when_condition" =>
        expression.children
          .find(isExpressionArgument)
          .map(astForExpression(_, context))
          .getOrElse(Ast(unknownNode(expression, expression.code)))
      case "assignment" =>
        astForAssignmentExpression(expression, context)
      case "jump_expression" if hasReturnKeyword(expression) =>
        astForReturn(expression, context)
      case "jump_expression" if hasThrowKeyword(expression) =>
        astForThrow(expression, context)
      case "jump_expression" if hasBreakKeyword(expression) =>
        astForBreakOrContinue(expression, ControlStructureTypes.BREAK)
      case "jump_expression" if hasContinueKeyword(expression) =>
        astForBreakOrContinue(expression, ControlStructureTypes.CONTINUE)
      case "call_expression" =>
        astForCallExpression(expression, context)
      case "navigation_expression" if isCallableReferenceNavigationExpression(expression) =>
        astForCallableReference(expression, context, expectedTypeFullName)
      case "navigation_expression" =>
        astForNavigationExpression(expression, context)
      case "this_expression" =>
        astForThisExpression(expression, context)
      case "super_expression" =>
        astForSuperExpression(expression, context)
      case "indexing_expression" =>
        astForIndexingExpression(expression, context)
      case "callable_reference" if isClassLiteralReference(expression) =>
        astForClassLiteralReference(expression)
      case "callable_reference" =>
        astForCallableReference(expression, context, expectedTypeFullName)
      case "directly_assignable_expression" =>
        astForDirectlyAssignableExpression(expression, context)
      case "value_argument" =>
        astForValueArgument(expression, context)
      case "simple_identifier" =>
        val typeFullName = registerType(context.types.getOrElse(expression.code, TypeConstants.Any))
        val identifier   = identifierNode(expression, expression.code, expression.code, typeFullName)
        context.refs
          .get(expression.code)
          .map(target => Ast(identifier).withRefEdge(identifier, target))
          .getOrElse(Ast(identifier))
      case "integer_literal" =>
        Ast(literalNode(expression, expression.code, registerType(integerLiteralType(expectedTypeFullName))))
      case "long_literal" =>
        Ast(literalNode(expression, expression.code, registerType("long")))
      case "real_literal" =>
        Ast(literalNode(expression, expression.code, registerType("double")))
      case "boolean_literal" =>
        Ast(literalNode(expression, expression.code, registerType("boolean")))
      case "character_literal" =>
        Ast(literalNode(expression, expression.code, registerType("char")))
      case "string_literal" =>
        astForStringLiteral(expression, context)
      case "hex_literal" | "bin_literal" =>
        Ast(literalNode(expression, expression.code, registerType(integerLiteralType(expectedTypeFullName))))
      case "null" =>
        Ast(literalNode(expression, expression.code, registerType("null")))
      case _ =>
        Ast(unknownNode(expression, expression.code))
    }
  }

  private def astForIfExpression(ifExpression: KotlinAstNode, context: BodyContext): Ast = {
    val conditionAst    = conditionExpression(ifExpression).map(astForExpression(_, context))
    val bodies          = ifExpression.children.filter(_.kind == "control_structure_body")
    val thenAst         = bodies.headOption.map(body => astForControlStructureBody(body, childContext(context)))
    val elseAst         = bodies.drop(1).headOption.map(body => astForControlStructureBody(body, childContext(context)))
    val ifNode          = controlStructureNode(ifExpression, ControlStructureTypes.IF, ifExpression.code)
    val childAsts       = thenAst.toList ++ elseAst.toList
    val astWithChildren = controlStructureAst(ifNode, conditionAst, childAsts)
    val astWithTrueBody = thenAst.flatMap(_.root) match {
      case Some(thenRoot) => astWithChildren.withTrueBodyEdge(ifNode, thenRoot)
      case None           => astWithChildren
    }
    elseAst.flatMap(_.root) match {
      case Some(elseRoot) => astWithTrueBody.withFalseBodyEdge(ifNode, elseRoot)
      case None           => astWithTrueBody
    }
  }

  private def astForIfAsExpression(ifExpression: KotlinAstNode, context: BodyContext): Ast = {
    val conditionAsts = conditionExpression(ifExpression).map(astForExpression(_, context)).toList
    val bodies        = ifExpression.children.filter(_.kind == "control_structure_body")
    val thenAsts      = bodies.headOption.toList.flatMap(body => astsForIfExpressionBody(body, context))
    val elseAsts      = bodies.drop(1).headOption.toList.flatMap(body => astsForIfExpressionBody(body, context))
    val args          = conditionAsts ++ thenAsts ++ elseAsts
    if (args.nonEmpty) {
      callAst(
        operatorCallNode(
          ifExpression,
          ifExpression.code,
          Operators.conditional,
          Some(typeForIfExpression(ifExpression, context))
        ),
        args
      )
    } else {
      Ast(unknownNode(ifExpression, ifExpression.code))
    }
  }

  private def astsForIfExpressionBody(body: KotlinAstNode, context: BodyContext): List[Ast] = {
    val statements     = controlBodyStatements(body)
    val branchContext  = childContext(context)
    val statementAsts  = statements.flatMap(astForStatement(_, branchContext))
    val expressionAsts = statements.map(astForExpression(_, branchContext))
    statements match {
      case Nil      => Nil
      case _ :: Nil => expressionAsts
      case _        => List(blockAst(blockNode(body, body.code, TypeConstants.Any), statementAsts))
    }
  }

  private def astForWhileStatement(whileStatement: KotlinAstNode, context: BodyContext): Ast = {
    val conditionAst = conditionExpression(whileStatement).map(astForExpression(_, context))
    val bodyAst = whileStatement.children
      .find(_.kind == "control_structure_body")
      .map(body => astForControlStructureBody(body, childContext(context)))
    whileAst(conditionAst, bodyAst.toList, Some(whileStatement.code), line(whileStatement), column(whileStatement))
  }

  private def astForForStatement(forStatement: KotlinAstNode, context: BodyContext): Ast = {
    val loopContext   = childContext(context)
    val iteratorName  = nextIteratorLocalName()
    val iteratorLocal = localNode(forStatement, iteratorName, iteratorName, TypeConstants.Any)
    loopContext.types.update(iteratorName, TypeConstants.Any)
    loopContext.refs.update(iteratorName, iteratorLocal)

    val iterableNode     = forStatement.children.find(isForIterableNode)
    val iteratorLocalAst = Ast(iteratorLocal)
    val iteratorAssignmentAst = iterableNode
      .map(iterable => astForIteratorAssignment(forStatement, iterable, iteratorName, context, loopContext))
      .getOrElse(Ast(unknownNode(forStatement, s"$iteratorName = <unknown>.iterator()")))
    iterableNode.flatMap(iterable => iterableElementTypeForExpression(iterable, context)).foreach { elementType =>
      loopContext.iteratorElementTypes.update(iteratorName, registerType(elementType))
    }
    iterableNode.foreach(iterable => updateForIteratorElementTypes(iterable, iteratorName, context, loopContext))
    val conditionAst = iteratorHasNextCallAst(forStatement, iteratorName, loopContext)
    val loopPrologue = forStatement.children
      .find(_.kind == "multi_variable_declaration")
      .map(destructuring => astsForForDestructuringPrologue(destructuring, iteratorName, loopContext))
      .getOrElse(astsForForVariablePrologue(forStatement, iteratorName, loopContext))
    val bodyAst = forStatement.children
      .find(_.kind == "control_structure_body")
      .map(body => astForControlStructureBody(body, loopContext, loopPrologue))
      .getOrElse(blockAst(blockNode(forStatement, "", TypeConstants.Any), loopPrologue))
    val whileAst = controlStructureAst(
      controlStructureNode(forStatement, ControlStructureTypes.WHILE, forStatement.code),
      Some(conditionAst),
      List(bodyAst)
    )
    blockAst(
      blockNode(forStatement, Constants.CodeForLoweredForBlock, TypeConstants.Any),
      List(iteratorLocalAst, iteratorAssignmentAst, whileAst)
    )
  }

  private def astsForForVariablePrologue(
    forStatement: KotlinAstNode,
    iteratorName: String,
    context: BodyContext
  ): List[Ast] =
    forStatement.children
      .find(_.kind == "variable_declaration")
      .flatMap { declaration =>
        firstChildCode(declaration, "simple_identifier").map { name =>
          val typeFullName = registerType(
            typeFromDirectChildren(declaration)
              .orElse(context.iteratorElementTypes.get(iteratorName))
              .getOrElse(TypeConstants.Any)
          )
          val local = localNode(declaration, name, name, typeFullName)
          context.types.update(name, typeFullName)
          context.refs.update(name, local)
          val nextAssignment = callAst(
            operatorCallNode(
              declaration,
              s"$name = $iteratorName.next()",
              Operators.assignment,
              Some(TypeConstants.Any)
            ),
            Seq(
              identifierAstForName(declaration, name, typeFullName, context),
              iteratorNextCallAst(declaration, iteratorName, context)
            )
          )
          List(Ast(local), nextAssignment)
        }
      }
      .getOrElse(Nil)

  private def updateForIteratorElementTypes(
    iterable: KotlinAstNode,
    iteratorName: String,
    outerContext: BodyContext,
    loopContext: BodyContext
  ): Unit = {
    val isMapIteration = receiverExpressionTypeFullName(iterable, outerContext).exists(MapTypeFullNames.contains)
    if (isMapIteration) {
      loopContext.iteratorElementTypes.update(iteratorName, MapEntryTypeFullName)
      mapKeyTypeForExpression(iterable, outerContext).foreach(keyType =>
        loopContext.mapEntryKeyTypes.update(iteratorName, registerType(keyType))
      )
      mapValueTypeForExpression(iterable, outerContext).foreach(valueType =>
        loopContext.mapEntryValueTypes.update(iteratorName, registerType(valueType))
      )
    } else {
      iterableElementTypeForExpression(iterable, outerContext).foreach(elementType =>
        loopContext.iteratorElementTypes.update(iteratorName, registerType(elementType))
      )
      mapEntryKeyTypeForExpression(iterable, outerContext).foreach(keyType =>
        loopContext.mapEntryKeyTypes.update(iteratorName, registerType(keyType))
      )
      mapEntryValueTypeForExpression(iterable, outerContext).foreach(valueType =>
        loopContext.mapEntryValueTypes.update(iteratorName, registerType(valueType))
      )
    }
  }

  private def propagateMapEntryTypes(sourceName: String, targetName: String, context: BodyContext): Unit = {
    context.mapEntryKeyTypes.get(sourceName) match {
      case Some(keyType) => context.mapEntryKeyTypes.update(targetName, keyType)
      case None          => context.mapEntryKeyTypes.remove(targetName)
    }
    context.mapEntryValueTypes.get(sourceName) match {
      case Some(valueType) => context.mapEntryValueTypes.update(targetName, valueType)
      case None            => context.mapEntryValueTypes.remove(targetName)
    }
  }

  private def propagatePairTypes(sourceName: String, targetName: String, context: BodyContext): Unit = {
    context.pairFirstTypes.get(sourceName) match {
      case Some(firstType) => context.pairFirstTypes.update(targetName, firstType)
      case None            => context.pairFirstTypes.remove(targetName)
    }
    context.pairSecondTypes.get(sourceName) match {
      case Some(secondType) => context.pairSecondTypes.update(targetName, secondType)
      case None             => context.pairSecondTypes.remove(targetName)
    }
  }

  private def propagateTripleTypes(sourceName: String, targetName: String, context: BodyContext): Unit = {
    context.tripleFirstTypes.get(sourceName) match {
      case Some(firstType) => context.tripleFirstTypes.update(targetName, firstType)
      case None            => context.tripleFirstTypes.remove(targetName)
    }
    context.tripleSecondTypes.get(sourceName) match {
      case Some(secondType) => context.tripleSecondTypes.update(targetName, secondType)
      case None             => context.tripleSecondTypes.remove(targetName)
    }
    context.tripleThirdTypes.get(sourceName) match {
      case Some(thirdType) => context.tripleThirdTypes.update(targetName, thirdType)
      case None            => context.tripleThirdTypes.remove(targetName)
    }
  }

  private def updateMapEntryTypesFromExpression(name: String, expression: KotlinAstNode, context: BodyContext): Unit = {
    mapEntryKeyTypeForExpression(expression, context) match {
      case Some(keyType) => context.mapEntryKeyTypes.update(name, registerType(keyType))
      case None          => context.mapEntryKeyTypes.remove(name)
    }
    mapEntryValueTypeForExpression(expression, context) match {
      case Some(valueType) => context.mapEntryValueTypes.update(name, registerType(valueType))
      case None            => context.mapEntryValueTypes.remove(name)
    }
  }

  private def updatePairTypesFromExpression(name: String, expression: KotlinAstNode, context: BodyContext): Unit = {
    pairTypesForExpression(expression, context) match {
      case Some((firstType, secondType)) =>
        context.pairFirstTypes.update(name, registerType(firstType))
        context.pairSecondTypes.update(name, registerType(secondType))
      case None =>
        context.pairFirstTypes.remove(name)
        context.pairSecondTypes.remove(name)
    }
  }

  private def updateTripleTypesFromExpression(name: String, expression: KotlinAstNode, context: BodyContext): Unit = {
    tripleTypesForExpression(expression, context) match {
      case Some((firstType, secondType, thirdType)) =>
        context.tripleFirstTypes.update(name, registerType(firstType))
        context.tripleSecondTypes.update(name, registerType(secondType))
        context.tripleThirdTypes.update(name, registerType(thirdType))
      case None =>
        context.tripleFirstTypes.remove(name)
        context.tripleSecondTypes.remove(name)
        context.tripleThirdTypes.remove(name)
    }
  }

  private def astsForForDestructuringPrologue(
    destructuring: KotlinAstNode,
    iteratorName: String,
    context: BodyContext
  ): List[Ast] = {
    val entries     = destructuringEntries(destructuring)
    val tmpName     = nextTmpLocalName()
    val tmpTypeName = context.iteratorElementTypes.getOrElse(iteratorName, TypeConstants.Any)
    val tmpLocal    = localNode(destructuring, tmpName, tmpName, registerType(tmpTypeName))
    context.types.update(tmpName, tmpLocal.typeFullName)
    context.refs.update(tmpName, tmpLocal)
    val nextAssignment = callAst(
      operatorCallNode(
        destructuring,
        s"$tmpName = $iteratorName.next()",
        Operators.assignment,
        Some(TypeConstants.Any)
      ),
      Seq(
        identifierAstForName(destructuring, tmpName, tmpLocal.typeFullName, context),
        iteratorNextCallAst(destructuring, iteratorName, context)
      )
    )
    val base = DestructuringBase(tmpName, tmpLocal.typeFullName, Nil)
    propagateMapEntryTypes(iteratorName, tmpName, context)
    propagatePairTypes(iteratorName, tmpName, context)
    propagateTripleTypes(iteratorName, tmpName, context)
    val typedEntries = destructuringEntriesWithComponentTypes(entries, base, context)
    val entryLocals  = localAstsForDestructuringEntries(typedEntries, context)
    val assignments  = assignmentAstsForDestructuringEntries(typedEntries, base, context)
    entryLocals ++ List(Ast(tmpLocal), nextAssignment) ++ assignments
  }

  private def astForIteratorAssignment(
    origin: KotlinAstNode,
    iterable: KotlinAstNode,
    iteratorName: String,
    iterableContext: BodyContext,
    iteratorContext: BodyContext
  ): Ast = {
    val iteratorCallCode = s"${iterable.code}.${Constants.GetIteratorMethodName}()"
    val assignment =
      operatorCallNode(origin, s"$iteratorName = $iteratorCallCode", Operators.assignment, Some(TypeConstants.Any))
    callAst(
      assignment,
      Seq(
        identifierAstForName(origin, iteratorName, TypeConstants.Any, iteratorContext),
        iteratorCallAst(iterable, iteratorCallCode, iterableContext)
      )
    )
  }

  private def iteratorCallAst(iterable: KotlinAstNode, code: String, context: BodyContext): Ast = {
    val signature        = methodSignature("java.util.Iterator", Nil)
    val iterableTypeName = receiverExpressionTypeFullName(iterable, context).getOrElse(TypeConstants.Any)
    val fullName         = methodFullName(s"$iterableTypeName.${Constants.GetIteratorMethodName}", signature)
    val call = callNode(
      iterable,
      code,
      Constants.GetIteratorMethodName,
      fullName,
      DispatchTypes.DYNAMIC_DISPATCH,
      Some(signature),
      Some("java.util.Iterator")
    )
    callAst(call, base = Some(astForExpression(iterable, context)))
  }

  private def iteratorHasNextCallAst(origin: KotlinAstNode, iteratorName: String, context: BodyContext): Ast = {
    val signature = methodSignature("boolean", Nil)
    val call = callNode(
      origin,
      s"$iteratorName.${Constants.HasNextIteratorMethodName}()",
      Constants.HasNextIteratorMethodName,
      methodFullName("kotlin.collections.Iterator.hasNext", signature),
      DispatchTypes.DYNAMIC_DISPATCH,
      Some(signature),
      Some("boolean")
    )
    callAst(call, base = Some(identifierAstForName(origin, iteratorName, TypeConstants.Any, context)))
  }

  private def iteratorNextCallAst(origin: KotlinAstNode, iteratorName: String, context: BodyContext): Ast = {
    val signature = methodSignature(TypeConstants.JavaLangObject, Nil)
    val call = callNode(
      origin,
      s"$iteratorName.${Constants.NextIteratorMethodName}()",
      Constants.NextIteratorMethodName,
      methodFullName("kotlin.collections.Iterator.next", signature),
      DispatchTypes.DYNAMIC_DISPATCH,
      Some(signature),
      Some(TypeConstants.JavaLangObject)
    )
    callAst(call, base = Some(identifierAstForName(origin, iteratorName, TypeConstants.Any, context)))
  }

  private def astForWhenExpression(whenExpression: KotlinAstNode, context: BodyContext): Ast = {
    val subjectAst = whenSubjectExpression(whenExpression).map(astForExpression(_, context))
    val entryAsts = whenExpression.children
      .filter(_.kind == "when_entry")
      .map(entry => astForWhenEntry(entry, context))
    val entryCode   = whenExpression.children.filter(_.kind == "when_entry").map(_.code).mkString("\n")
    val switchBlock = blockAst(blockNode(whenExpression, entryCode, TypeConstants.Any), entryAsts)
    val switchCode  = whenSubjectExpression(whenExpression).map(subject => s"when(${subject.code})").getOrElse("when")
    val switchNode  = controlStructureNode(whenExpression, ControlStructureTypes.SWITCH, switchCode)
    val switchAst   = Ast(switchNode).withChildren(subjectAst.toList :+ switchBlock)
    subjectAst.flatMap(_.root) match {
      case Some(subjectRoot) => switchAst.withConditionEdge(switchNode, subjectRoot)
      case None              => switchAst
    }
  }

  private def astForWhenAsExpression(whenExpression: KotlinAstNode, context: BodyContext): Ast =
    whenSubjectExpression(whenExpression) match {
      case Some(subject) =>
        val subjectBlock = blockAst(blockNode(subject, "", TypeConstants.Any), List(astForExpression(subject, context)))
        val entryAsts = whenExpression.children
          .filter(_.kind == "when_entry")
          .map(entry => astForWhenExpressionEntry(entry, context))
        callAst(
          operatorCallNode(
            whenExpression,
            "<operator>.when",
            "<operator>.when",
            Some(typeForWhenExpression(whenExpression, context))
          ),
          subjectBlock +: entryAsts
        )
      case None =>
        astForSubjectlessWhenAsExpression(whenExpression, context)
    }

  private def astForWhenExpressionEntry(whenEntry: KotlinAstNode, context: BodyContext): Ast = {
    val entryContext = childContext(context)
    val conditionAsts = whenEntry.children
      .filter(_.kind == "when_condition")
      .flatMap(condition => astsForWhenCondition(condition, context))
    val bodyAsts = whenEntry.children
      .find(_.kind == "control_structure_body")
      .toList
      .flatMap(body => astsForWhenExpressionBody(body, entryContext))
    blockAst(blockNode(whenEntry, "", TypeConstants.Any), conditionAsts ++ bodyAsts)
  }

  private def astForSubjectlessWhenAsExpression(whenExpression: KotlinAstNode, context: BodyContext): Ast = {
    val typeFullName = typeForWhenExpression(whenExpression, context)
    whenExpression.children
      .filter(_.kind == "when_entry")
      .reverse
      .foldLeft(Ast()) { case (elseAst, entry) =>
        val conditions = entry.children.filter(_.kind == "when_condition")
        val bodyAst = entry.children
          .find(_.kind == "control_structure_body")
          .toList
          .flatMap(body => astsForWhenExpressionBody(body, childContext(context)))
          .headOption
        if (conditions.isEmpty) {
          bodyAst.getOrElse(elseAst)
        } else {
          val conditionAst = conditions.headOption.flatMap(astsForWhenCondition(_, context).headOption)
          (conditionAst, bodyAst) match {
            case (Some(condition), Some(body)) =>
              callAst(
                operatorCallNode(entry, Operators.conditional, Operators.conditional, Some(typeFullName)),
                List(condition, body, elseAst)
              )
            case _ => elseAst
          }
        }
      }
  }

  private def astForWhenEntry(whenEntry: KotlinAstNode, context: BodyContext): Ast = {
    val entryContext = childContext(context)
    val conditionAsts = whenEntry.children
      .filter(_.kind == "when_condition")
      .flatMap(condition => astsForWhenCondition(condition, context))
    val bodyAsts = whenEntry.children
      .find(_.kind == "control_structure_body")
      .map(controlBodyStatements)
      .getOrElse(Nil)
      .flatMap(astForStatement(_, entryContext))
    blockAst(blockNode(whenEntry, whenEntry.code, TypeConstants.Any), conditionAsts ++ bodyAsts)
  }

  private def astsForWhenExpressionBody(body: KotlinAstNode, context: BodyContext): List[Ast] = {
    val statements = controlBodyStatements(body)
    if (body.code.trim.startsWith("{") || statements.sizeCompare(1) > 0) {
      List(blockAst(blockNode(body, body.code, TypeConstants.Any), statements.flatMap(astForStatement(_, context))))
    } else {
      statements.map(astForExpression(_, context))
    }
  }

  private def astsForWhenCondition(whenCondition: KotlinAstNode, context: BodyContext): List[Ast] = {
    val conditionExpressions = whenCondition.children.filter(isExpressionArgument)
    if (conditionExpressions.nonEmpty) {
      conditionExpressions.map(astForExpression(_, context))
    } else {
      Nil
    }
  }

  private def astForTryAsStatement(tryExpression: KotlinAstNode, context: BodyContext): Ast = {
    val tryBodyAst = statementsChild(tryExpression)
      .map(statements => astForStatementsBlock(statements, childContext(context)))
      .getOrElse(blockAst(blockNode(tryExpression, "", TypeConstants.Any)))
    val catchAsts = tryExpression.children.filter(_.kind == "catch_block").map(astForCatchBlock(_, context))
    val finallyAst = tryExpression.children
      .find(_.kind == "finally_block")
      .map(astForFinallyBlock(_, context))
    val tryNode = controlStructureNode(tryExpression, ControlStructureTypes.TRY, tryExpression.code)
    tryCatchAst(tryNode, tryBodyAst, catchAsts, finallyAst)
  }

  private def astForTryAsExpression(tryExpression: KotlinAstNode, context: BodyContext): Ast = {
    val tryBodyAst = statementsChild(tryExpression)
      .map(statements => astForStatementsBlock(statements, childContext(context)))
      .getOrElse(blockAst(blockNode(tryExpression, "", TypeConstants.Any)))
    val catchBodyAsts = tryExpression.children
      .filter(_.kind == "catch_block")
      .flatMap(statementsChild)
      .map(statements => astForStatementsBlock(statements, childContext(context)))
    val typeFullName = registerType(typeForTryExpression(tryExpression, context).getOrElse(TypeConstants.Any))
    callAst(
      operatorCallNode(tryExpression, tryExpression.code, Operators.tryCatch, Some(typeFullName)),
      tryBodyAst +: catchBodyAsts
    )
  }

  private def astForCatchBlock(catchBlock: KotlinAstNode, context: BodyContext): Ast = {
    val catchNode    = controlStructureNode(catchBlock, ControlStructureTypes.CATCH, catchBlock.code)
    val catchContext = childContext(context)
    val bodyAst = statementsChild(catchBlock)
      .map(statements => astForStatementsBlock(statements, catchContext))
      .getOrElse(blockAst(blockNode(catchBlock, "", TypeConstants.Any)))
    Ast(catchNode).withChild(bodyAst)
  }

  private def astForFinallyBlock(finallyBlock: KotlinAstNode, context: BodyContext): Ast = {
    val finallyNode = controlStructureNode(finallyBlock, ControlStructureTypes.FINALLY, finallyBlock.code)
    val bodyAst = statementsChild(finallyBlock)
      .map(statements => astForStatementsBlock(statements, childContext(context)))
      .getOrElse(blockAst(blockNode(finallyBlock, "", TypeConstants.Any)))
    Ast(finallyNode).withChild(bodyAst)
  }

  private def astForThrow(jumpExpression: KotlinAstNode, context: BodyContext): Ast = {
    val thrownAsts = jumpExpression.children
      .filter(child => child.named && child.kind != "label")
      .take(1)
      .map(astForThrownExpression(_, context))
    thrownAsts.headOption.flatMap(_.root).collect { case node: AstNodeNew =>
      node.order(1)
    }
    val throwNode = controlStructureNode(jumpExpression, ControlStructureTypes.THROW, jumpExpression.code)
    val throwAst  = Ast(throwNode).withChildren(thrownAsts)
    thrownAsts.headOption.flatMap(_.root) match {
      case Some(thrownRoot) => throwAst.withArgEdge(throwNode, thrownRoot)
      case None             => throwAst
    }
  }

  private def astForThrownExpression(expression: KotlinAstNode, context: BodyContext): Ast =
    expression.kind match {
      case "call_expression" =>
        constructorInfoForThrownCall(expression, context)
          .map(constructorCallBlockAst(expression, context, _))
          .getOrElse(astForExpression(expression, context))
      case _ =>
        astForExpression(expression, context)
    }

  private def constructorCallBlockAst(
    callExpression: KotlinAstNode,
    context: BodyContext,
    constructorInfo: MethodInfo,
    includeResultIdentifier: Boolean = false
  ): Ast = {
    val tmpName      = nextTmpLocalName()
    val typeFullName = registerType(constructorInfo.returnTypeFullName)
    val local        = localNode(callExpression, tmpName, tmpName, typeFullName)
    val lhs          = identifierNode(callExpression, tmpName, tmpName, typeFullName)
    val lhsAst       = Ast(lhs).withRefEdge(lhs, local)
    val alloc        = operatorCallNode(callExpression, Operators.alloc, Operators.alloc, Some(typeFullName))
    val assignment = callAst(
      operatorCallNode(callExpression, s"$tmpName = <alloc>", Operators.assignment, Some(typeFullName)),
      List(lhsAst, Ast(alloc))
    )
    val receiver    = identifierNode(callExpression, tmpName, tmpName, typeFullName)
    val receiverAst = Ast(receiver).withRefEdge(receiver, local)
    val initCall = callNode(
      callExpression,
      callExpression.code,
      Defines.ConstructorMethodName,
      constructorInfo.fullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(constructorInfo.signature),
      Some(TypeConstants.Void)
    )
    val argumentContext = constructorArgumentContext(constructorInfo, context)
    val args            = callArgumentNodes(callExpression).map(astForExpression(_, argumentContext))
    val initAst         = callAst(initCall, args, Some(receiverAst))
    val resultIdentifierAst = Option.when(includeResultIdentifier) {
      val resultIdentifier = identifierNode(callExpression, tmpName, tmpName, typeFullName)
      Ast(resultIdentifier).withRefEdge(resultIdentifier, local)
    }
    blockAst(
      blockNode(callExpression, callExpression.code, typeFullName),
      List(Ast(local), assignment, initAst) ++ resultIdentifierAst
    )
  }

  private def constructorInfoForThrownCall(callExpression: KotlinAstNode, context: BodyContext): Option[MethodInfo] = {
    constructorInfoForCallExpression(callExpression, context)
  }

  private def constructedTypeFullName(constructorInfo: MethodInfo): String =
    methodBaseFullName(constructorInfo.fullName).stripSuffix(s".${Defines.ConstructorMethodName}")

  private def constructorArgumentContext(constructorInfo: MethodInfo, context: BodyContext): BodyContext =
    arrayConstructorLambdaReturnType(constructorInfo)
      .map(returnType =>
        context.copy(expectedLambdaElementType = Some("int"), expectedLambdaReturnType = Some(returnType))
      )
      .getOrElse(context)

  private def arrayConstructorLambdaReturnType(constructorInfo: MethodInfo): Option[String] =
    Option
      .when(methodBaseFullName(constructorInfo.fullName).startsWith("kotlin."))(
        indexElementTypeFullName(constructorInfo.returnTypeFullName)
      )
      .flatten

  private def javaLangThrowableConstructorInfo(
    callName: String,
    argumentNodes: List[KotlinAstNode],
    context: BodyContext
  ): Option[MethodInfo] =
    JavaLangThrowableTypes.get(callName).map { typeFullName =>
      val parameterTypes = argumentNodes.map(argument =>
        valueArgumentExpressionNode(argument).flatMap(typeForExpression(_, context)).getOrElse(TypeConstants.Any)
      )
      val signature = methodSignature(TypeConstants.Void, parameterTypes)
      MethodInfo(methodFullName(s"$typeFullName.${Defines.ConstructorMethodName}", signature), signature, typeFullName)
    }

  private def astForBreakOrContinue(jumpExpression: KotlinAstNode, controlStructureType: String): Ast = {
    val node     = controlStructureNode(jumpExpression, controlStructureType, jumpExpression.code)
    val labelAst = jumpLabelAst(jumpExpression)
    val jumpAst  = Ast(node).withChildren(labelAst.toList)
    labelAst.flatMap(_.root) match {
      case Some(labelRoot) => jumpAst.withJumpArgumentEdge(node, labelRoot)
      case None            => jumpAst
    }
  }

  private def astForJumpTarget(label: KotlinAstNode): Ast = {
    val labelName = label.code.stripSuffix("@")
    Ast(jumpTargetNode(label, labelName, label.code))
  }

  private def astForStatementsBlock(statements: KotlinAstNode, context: BodyContext): Ast =
    blockAst(
      blockNode(statements, statements.code, TypeConstants.Any),
      statements.children.filter(_.named).flatMap(astForStatement(_, context))
    )

  private def astsForAnonymousInitializer(initializer: KotlinAstNode, context: BodyContext): List[Ast] =
    statementsChild(initializer)
      .map(_.children.filter(_.named).flatMap(astForStatement(_, context)))
      .getOrElse(initializer.children.filter(_.named).flatMap(astForStatement(_, context)))

  private def astForLambdaLiteral(lambdaLiteral: KotlinAstNode, context: BodyContext): Ast = {
    val explicitParams = lambdaParameterInfos(lambdaLiteral)
    val params =
      if (explicitParams.nonEmpty) {
        explicitParams.map { param =>
          context.expectedLambdaElementType
            .filter(_ => param.typeFullName == TypeConstants.Any && explicitParams.sizeCompare(1) == 0)
            .map(elementType => lambdaParamWithExpectedElementType(param, elementType, context))
            .getOrElse(param)
        }
      } else if (
        lambdaLiteral.children.exists(child =>
          child.kind == "lambda_parameters" && child.children.exists(_.kind == "multi_variable_declaration")
        )
      ) {
        List(
          ParameterInfo(
            lambdaLiteral,
            s"${Constants.DestructedParamNamePrefix}1",
            registerType(context.expectedLambdaElementType.getOrElse(TypeConstants.Any)),
            s"${Constants.DestructedParamNamePrefix}1",
            declaresMember = false,
            mapEntryKeyTypeFullName = context.expectedLambdaMapEntryKeyType,
            mapEntryValueTypeFullName = context.expectedLambdaMapEntryValueType
          )
        )
      } else if (lambdaUsesImplicitIt(lambdaLiteral)) {
        List(
          ParameterInfo(
            lambdaLiteral,
            "it",
            registerType(context.expectedLambdaElementType.getOrElse(TypeConstants.Any)),
            "it",
            declaresMember = false,
            mapEntryKeyTypeFullName = context.expectedLambdaMapEntryKeyType,
            mapEntryValueTypeFullName = context.expectedLambdaMapEntryValueType
          )
        )
      } else {
        Nil
      }
    astForGeneratedFunction(
      lambdaLiteral,
      context,
      params,
      context.expectedLambdaReturnType.getOrElse(TypeConstants.JavaLangObject),
      lambdaStatements(lambdaLiteral)
    )
  }

  private def lambdaParamWithExpectedElementType(
    param: ParameterInfo,
    elementType: String,
    context: BodyContext
  ): ParameterInfo =
    param.copy(
      typeFullName = registerType(elementType),
      mapEntryKeyTypeFullName = context.expectedLambdaMapEntryKeyType.orElse(param.mapEntryKeyTypeFullName),
      mapEntryValueTypeFullName = context.expectedLambdaMapEntryValueType.orElse(param.mapEntryValueTypeFullName)
    )

  private def astForAnonymousFunction(anonymousFunction: KotlinAstNode, context: BodyContext): Ast = {
    val params     = functionParameters(anonymousFunction)
    val returnType = registerType(returnTypeForFunction(anonymousFunction))
    val statements = anonymousFunction.children
      .find(_.kind == "function_body")
      .flatMap(_.children.find(_.kind == "statements"))
      .map(_.children.filter(_.named))
      .getOrElse(Nil)
    astForGeneratedFunction(anonymousFunction, context, params, returnType, statements)
  }

  private def astForObjectLiteralExpression(objectLiteral: KotlinAstNode, context: BodyContext): Ast = {
    val typeDeclAst  = astForObjectLiteralTypeDeclaration(objectLiteral, context)
    val typeFullName = typeDeclAst.root.collect { case typeDecl: NewTypeDecl => typeDecl.fullName }.get
    val tmpName      = nextObjectTempName()
    val local        = localNode(objectLiteral, tmpName, tmpName, typeFullName)
    val lhs          = identifierNode(objectLiteral, tmpName, tmpName, typeFullName)
    val lhsAst       = Ast(lhs).withRefEdge(lhs, local)
    val alloc        = operatorCallNode(objectLiteral, Operators.alloc, Operators.alloc, Some(typeFullName))
    val assignment = callAst(
      operatorCallNode(objectLiteral, s"$tmpName = <alloc>", Operators.assignment, Some(typeFullName)),
      Seq(lhsAst, Ast(alloc))
    )
    val initAst = astForObjectLiteralInitCall(objectLiteral, tmpName, local, typeFullName)
    val ref     = identifierNode(objectLiteral, tmpName, tmpName, typeFullName)
    val refAst  = Ast(ref).withRefEdge(ref, local)

    blockAst(
      blockNode(objectLiteral, objectLiteral.code, typeFullName),
      List(typeDeclAst, Ast(local), assignment, initAst, refAst)
    )
  }

  private def astForObjectLiteralTypeDeclaration(
    objectLiteral: KotlinAstNode,
    context: BodyContext,
    ownerNameOverride: Option[String] = None,
    parentTypeOverride: Option[String] = None,
    parentNameOverride: Option[String] = None
  ): Ast = {
    val index     = nextObjectLiteralIndex()
    val ownerName = ownerNameOverride.getOrElse(context.ownerMethodFullName.takeWhile(_ != ':'))
    val parentType = parentTypeOverride.getOrElse {
      if (context.ownerMethodFullName.nonEmpty) NodeTypes.METHOD else NodeTypes.NAMESPACE_BLOCK
    }
    val parentName = parentNameOverride.getOrElse {
      if (context.ownerMethodFullName.nonEmpty) context.ownerMethodFullName
      else namespaceAstParentFullName(sourcePackageName)
    }
    val fullName = registerType(s"$ownerName.object$$$index")
    val inherits = objectLiteralInherits(objectLiteral)
    val objectType = typeDeclNode(
      objectLiteral,
      "anonymous_obj",
      fullName,
      document.relativeName,
      objectLiteral.code,
      parentType,
      parentName,
      inherits
    )
    val constructor  = astForPrimaryConstructor(objectLiteral, fullName, Nil, constructorMethodInfo(fullName, Nil))
    val bodyChildren = classBodyChildren(objectLiteral)
    val memberAsts = bodyChildren.flatMap {
      case declaration if declaration.kind == "property_declaration" =>
        astsForPropertyDeclaration(declaration, fullName)
      case _ => Nil
    }
    val methodAsts = bodyChildren.collect {
      case declaration if declaration.kind == "function_declaration" =>
        astForFunctionDeclaration(declaration, sourcePackageName, ownerTypeFullName = Some(fullName))
    }
    val innerTypeAsts = bodyChildren.collect {
      case declaration
          if declaration.kind == "class_declaration" || declaration.kind == "object_declaration" || declaration.kind == "companion_object" =>
        astForTypeDeclaration(declaration, sourcePackageName, ownerTypeFullName = Some(fullName))
    }
    val typeAst = Ast(objectType).withChildren(constructor :: memberAsts ++ methodAsts ++ innerTypeAsts)
    bindMethodsToType(typeAst, objectType, constructor :: methodAsts)
  }

  private def astForObjectLiteralInitCall(
    objectLiteral: KotlinAstNode,
    receiverName: String,
    local: NewLocal,
    typeFullName: String
  ): Ast = {
    val initSignature = methodSignature(TypeConstants.Void, Nil)
    val initFullName  = methodFullName(s"$typeFullName.${Defines.ConstructorMethodName}", initSignature)
    val initCall = callNode(
      objectLiteral,
      Defines.ConstructorMethodName,
      Defines.ConstructorMethodName,
      initFullName,
      DispatchTypes.STATIC_DISPATCH,
      Some(initSignature),
      Some(TypeConstants.Void)
    )
    val receiver    = identifierNode(objectLiteral, receiverName, receiverName, typeFullName)
    val receiverAst = Ast(receiver).withRefEdge(receiver, local)
    callAst(initCall, Seq.empty, Some(receiverAst))
  }

  private def objectLiteralInherits(objectLiteral: KotlinAstNode): Seq[String] = {
    val inheritedTypes = objectLiteral.children
      .filter(_.kind == "delegation_specifier")
      .flatMap(
        _.descendants
          .find(_.kind == "type_identifier")
          .map(typeIdentifier => registerType(mapTypeName(typeIdentifier.code)))
      )
    if (inheritedTypes.nonEmpty) inheritedTypes else Seq(registerType(TypeConstants.JavaLangObject))
  }

  private def astForGeneratedFunction(
    origin: KotlinAstNode,
    context: BodyContext,
    params: List[ParameterInfo],
    returnTypeFullName: String,
    statements: List[KotlinAstNode]
  ): Ast = {
    val name      = nextClosureName()
    val descName  = s"${context.ownerMethodFullName.takeWhile(_ != ':')}.$name"
    val signature = methodSignature(returnTypeFullName, params.map(_.typeFullName))
    val fullName  = methodFullName(descName, signature)
    val method    = methodNode(origin, name, fullName, signature, document.relativeName)

    val lambdaContext = childContext(context).copy(ownerMethodFullName = fullName)
    val paramNodes = params.zipWithIndex.map { case (param, index) =>
      val paramNode = parameterInNode(
        param.node,
        param.name,
        param.code,
        index + 1,
        isVariadic = false,
        evaluationStrategy = EvaluationStrategies.BY_VALUE,
        typeFullName = param.typeFullName
      )
      lambdaContext.types.update(param.name, param.typeFullName)
      lambdaContext.refs.update(param.name, paramNode)
      updateParameterTypeMetadata(param, lambdaContext)
      paramNode
    }
    val prologueAsts = origin.kind match {
      case "lambda_literal" => astsForLambdaDestructuringPrologue(origin, params.headOption, lambdaContext)
      case _                => Nil
    }
    val bodyAsts = astsForFunctionLiteralStatements(statements, lambdaContext)
    val body     = blockAst(blockNode(origin, origin.code, TypeConstants.Any), prologueAsts ++ bodyAsts)
    val lambdaMethodAst = methodAst(
      method,
      params.zip(paramNodes).map { case (param, paramNode) =>
        Ast(paramNode).withChildren(param.annotations.map(astForAnnotationEntry))
      },
      body,
      methodReturnNode(origin, returnTypeFullName),
      Seq(modifierNode(origin, ModifierTypes.VIRTUAL), modifierNode(origin, ModifierTypes.LAMBDA))
    )
    lambdaMethodAsts.append(lambdaMethodAst)

    val lambdaTypeDeclFullName = fullName.takeWhile(_ != ':')
    val lambdaTypeDecl = typeDeclNode(
      origin,
      Constants.LambdaTypeDeclName,
      lambdaTypeDeclFullName,
      document.relativeName,
      Seq(registerType(Constants.UnknownLambdaBaseClass)),
      None
    )
    val lambdaBinding = bindingNode(Constants.UnknownLambdaBindingName, signature, fullName)
    lambdaTypeDeclAsts.append(
      Ast(lambdaTypeDecl)
        .merge(Ast(lambdaBinding))
        .withBindsEdge(lambdaTypeDecl, lambdaBinding)
        .withRefEdge(lambdaBinding, method)
    )

    val methodRef = methodRefNode(origin, origin.code, fullName, lambdaTypeDeclFullName)
    capturedRefsForFunctionLiteral(statements, params.map(_.name).toSet, context)
      .foldLeft(Ast(methodRef).withRefEdge(methodRef, method)) { case (ast, (capturedName, capturedNode)) =>
        val closureBinding = closureBindingNode(s"$descName.$capturedName", EvaluationStrategies.BY_REFERENCE)
        ast
          .merge(Ast(closureBinding))
          .withCaptureEdge(methodRef, closureBinding)
          .withRefEdge(closureBinding, capturedNode)
      }
  }

  private def astsForFunctionLiteralStatements(statements: List[KotlinAstNode], context: BodyContext): List[Ast] = {
    statements.zipWithIndex.flatMap { case (statement, index) =>
      val isLastStatement = index == statements.size - 1
      if (isLastStatement && shouldWrapAsImplicitReturn(statement)) {
        List(returnAst(returnNode(statement, Constants.RetCode), Seq(astForExpression(statement, context))))
      } else {
        astForStatement(statement, context)
      }
    }
  }

  private def shouldWrapAsImplicitReturn(statement: KotlinAstNode): Boolean =
    statement.kind != "property_declaration" && !(statement.kind == "jump_expression" && hasReturnKeyword(statement))

  private def astsForLambdaDestructuringPrologue(
    lambdaLiteral: KotlinAstNode,
    syntheticParam: Option[ParameterInfo],
    context: BodyContext
  ): List[Ast] = {
    val destructuring = lambdaLiteral.children
      .find(_.kind == "lambda_parameters")
      .flatMap(_.children.find(_.kind == "multi_variable_declaration"))
    (destructuring, syntheticParam) match {
      case (Some(destructuringNode), Some(param)) =>
        val itLocal  = localNode(lambdaLiteral, "it", "it", param.typeFullName)
        val tmpName  = nextTmpLocalName()
        val tmpLocal = localNode(lambdaLiteral, tmpName, tmpName, param.typeFullName)
        context.types.update(tmpName, tmpLocal.typeFullName)
        context.refs.update(tmpName, tmpLocal)

        val tmpLhs       = identifierAstForName(lambdaLiteral, tmpName, tmpLocal.typeFullName, context)
        val itIdentifier = identifierNode(lambdaLiteral, "it", "it", param.typeFullName)
        val itAst = context.refs
          .get(param.name)
          .map(target => Ast(itIdentifier).withRefEdge(itIdentifier, target))
          .getOrElse(Ast(itIdentifier))
        val tmpAssignment = callAst(
          operatorCallNode(lambdaLiteral, s"$tmpName = it", Operators.assignment, Some(TypeConstants.Any)),
          Seq(tmpLhs, itAst)
        )
        val entries = destructuringEntries(destructuringNode)
        val base    = DestructuringBase(tmpName, tmpLocal.typeFullName, Nil)
        propagateMapEntryTypes(param.name, tmpName, context)
        val typedEntries = destructuringEntriesWithComponentTypes(entries, base, context)
        val entryLocals  = localAstsForDestructuringEntries(typedEntries, context)
        val assignments  = assignmentAstsForDestructuringEntries(typedEntries, base, context)
        List(Ast(itLocal), Ast(tmpLocal)) ++ entryLocals ++ List(tmpAssignment) ++ assignments
      case _ =>
        Nil
    }
  }

  private def capturedRefsForFunctionLiteral(
    statements: List[KotlinAstNode],
    declaredParams: Set[String],
    outerContext: BodyContext
  ): List[(String, NewNode)] = {
    val declaredLocals = statements
      .filter(_.kind == "property_declaration")
      .flatMap(_.children.find(_.kind == "variable_declaration"))
      .flatMap(declaration => firstChildCode(declaration, "simple_identifier"))
      .toSet
    val referencedNames = statements
      .flatMap(statement => statement :: statement.descendants)
      .collect { case node if node.kind == "simple_identifier" => node.code }
      .toSet
    (referencedNames -- declaredParams -- declaredLocals).toList.sorted.flatMap { name =>
      outerContext.refs.get(name).map(name -> _)
    }
  }

  private def destructuringEntries(destructuring: KotlinAstNode): List[DestructuringEntry] = {
    destructuring.children.filter(_.kind == "variable_declaration").zipWithIndex.map { case (entryNode, index) =>
      val name = firstChildCode(entryNode, "simple_identifier").getOrElse(nameFromDeclarationCode(entryNode.code))
      val typeFullName = registerType(typeFromDirectChildren(entryNode).getOrElse(TypeConstants.Any))
      DestructuringEntry(entryNode, name, index + 1, typeFullName)
    }
  }

  private def localAstsForDestructuringEntries(entries: List[DestructuringEntry], context: BodyContext): List[Ast] = {
    entries.filterNot(_.name == Constants.UnusedDestructuringEntryText).map { entry =>
      val local = localNode(entry.node, entry.name, entry.name, entry.typeFullName)
      context.types.update(entry.name, entry.typeFullName)
      context.refs.update(entry.name, local)
      Ast(local)
    }
  }

  private def destructuringEntriesWithComponentTypes(
    entries: List[DestructuringEntry],
    base: DestructuringBase,
    context: BodyContext
  ): List[DestructuringEntry] =
    entries.map { entry =>
      if (entry.typeFullName == TypeConstants.Any) {
        val componentName = s"${Constants.ComponentNPrefix}${entry.originalIndex}"
        componentMethodInfo(base, componentName, context)
          .map(info => entry.copy(typeFullName = registerType(info.returnTypeFullName)))
          .getOrElse(entry)
      } else {
        entry
      }
    }

  private def assignmentAstsForDestructuringEntries(
    entries: List[DestructuringEntry],
    base: DestructuringBase,
    context: BodyContext
  ): List[Ast] = {
    entries.filterNot(_.name == Constants.UnusedDestructuringEntryText).map { entry =>
      val componentIdx  = entry.originalIndex
      val componentName = s"${Constants.ComponentNPrefix}$componentIdx"
      val componentCode = s"${base.name}.$componentName()"
      val lhsAst        = identifierAstForName(entry.node, entry.name, entry.typeFullName, context)
      val rhsAst        = componentCallAst(entry.node, componentName, componentCode, base, context)
      callAst(
        operatorCallNode(entry.node, s"${entry.name} = $componentCode", Operators.assignment, Some(TypeConstants.Any)),
        Seq(lhsAst, rhsAst)
      )
    }
  }

  private def componentCallAst(
    origin: KotlinAstNode,
    componentName: String,
    componentCode: String,
    base: DestructuringBase,
    context: BodyContext
  ): Ast = {
    val resolvedInfo = componentMethodInfo(base, componentName, context)
    val signature    = resolvedInfo.map(_.signature).getOrElse(s"${Defines.UnresolvedSignature}(0)")
    val fullName = resolvedInfo
      .map(_.fullName)
      .getOrElse(methodFullName(s"${Defines.UnresolvedNamespace}.$componentName", signature))
    val call = callNode(
      origin,
      componentCode,
      componentName,
      fullName,
      DispatchTypes.DYNAMIC_DISPATCH,
      Some(signature),
      Some(resolvedInfo.map(_.returnTypeFullName).getOrElse(TypeConstants.Any))
    )
    callAst(call, base = Some(identifierAstForName(origin, base.name, base.typeFullName, context)))
  }

  private def componentMethodInfo(
    base: DestructuringBase,
    componentName: String,
    context: BodyContext
  ): Option[MethodInfo] =
    mapEntryComponentMethodInfo(base, componentName, context)
      .orElse(pairComponentMethodInfo(base, componentName, context))
      .orElse(tripleComponentMethodInfo(base, componentName, context))
      .orElse(methodInfoByOwnerNameAndArity(base.typeFullName, componentName, arity = 0))

  private def mapEntryComponentMethodInfo(
    base: DestructuringBase,
    componentName: String,
    context: BodyContext
  ): Option[MethodInfo] =
    Option.when(base.typeFullName == MapEntryTypeFullName && MapEntryComponentNames(componentName)) {
      val returnType =
        if (componentName == "component1") {
          context.mapEntryKeyTypes.getOrElse(base.name, TypeConstants.JavaLangObject)
        } else {
          context.mapEntryValueTypes.getOrElse(base.name, TypeConstants.JavaLangObject)
        }
      val signature = methodSignature(TypeConstants.JavaLangObject, Seq(MapEntryTypeFullName))
      MethodInfo(
        methodFullName(s"kotlin.collections.$componentName", signature),
        signature,
        returnType,
        isExtension = true
      )
    }

  private def pairComponentMethodInfo(
    base: DestructuringBase,
    componentName: String,
    context: BodyContext
  ): Option[MethodInfo] =
    Option.when(base.typeFullName == PairTypeFullName && PairComponentNames(componentName)) {
      val returnType =
        if (componentName == "component1") {
          context.pairFirstTypes.getOrElse(base.name, TypeConstants.JavaLangObject)
        } else {
          context.pairSecondTypes.getOrElse(base.name, TypeConstants.JavaLangObject)
        }
      val signature = methodSignature(TypeConstants.JavaLangObject, Nil)
      MethodInfo(methodFullName(s"$PairTypeFullName.$componentName", signature), signature, returnType)
    }

  private def tripleComponentMethodInfo(
    base: DestructuringBase,
    componentName: String,
    context: BodyContext
  ): Option[MethodInfo] =
    Option.when(base.typeFullName == TripleTypeFullName && TripleComponentNames(componentName)) {
      val returnType =
        componentName match {
          case "component1" => context.tripleFirstTypes.getOrElse(base.name, TypeConstants.JavaLangObject)
          case "component2" => context.tripleSecondTypes.getOrElse(base.name, TypeConstants.JavaLangObject)
          case _            => context.tripleThirdTypes.getOrElse(base.name, TypeConstants.JavaLangObject)
        }
      val signature = methodSignature(TypeConstants.JavaLangObject, Nil)
      MethodInfo(methodFullName(s"$TripleTypeFullName.$componentName", signature), signature, returnType)
    }

  private def identifierAstForName(
    origin: KotlinAstNode,
    name: String,
    typeFullName: String,
    context: BodyContext
  ): Ast = {
    val identifier = identifierNode(origin, name, name, registerType(typeFullName))
    context.refs
      .get(name)
      .map(target => Ast(identifier).withRefEdge(identifier, target))
      .getOrElse(Ast(identifier))
  }

  private def nextTmpLocalName(): String = {
    tmpLocalCounter += 1
    s"${Constants.TmpLocalPrefix}$tmpLocalCounter"
  }

  private def nextIteratorLocalName(): String = {
    iteratorLocalCounter += 1
    s"${Constants.IteratorPrefix}$iteratorLocalCounter"
  }

  private def nextObjectLiteralIndex(): Int = {
    val index = objectLiteralCounter
    objectLiteralCounter += 1
    index
  }

  private def nextObjectTempName(): String = {
    objectTempCounter += 1
    s"tmp_obj_$objectTempCounter"
  }

  private def astForControlStructureBody(
    body: KotlinAstNode,
    context: BodyContext,
    prefixAsts: List[Ast] = Nil
  ): Ast = {
    val statements = controlBodyStatements(body).flatMap(astForStatement(_, context))
    blockAst(blockNode(body, body.code, TypeConstants.Any), prefixAsts ++ statements)
  }

  private def astForBinaryOperatorExpression(
    expression: KotlinAstNode,
    context: BodyContext,
    operatorName: String,
    typeFullName: String
  ): Ast = {
    callAst(
      operatorCallNode(expression, expression.code, operatorName, Some(typeFullName)),
      expression.children.filter(isExpressionArgument).map(astForExpression(_, context))
    )
  }

  private def astForMultiplicativeExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val operatorName = expression.children.find(child => Set("*", "/", "%").contains(child.kind)).map(_.kind) match {
      case Some("/") => Operators.division
      case Some("%") => Operators.modulo
      case _         => Operators.multiplication
    }
    callAst(
      operatorCallNode(
        expression,
        expression.code,
        operatorName,
        Some(registerType(arithmeticExpressionTypeFullName(expression, context).getOrElse(TypeConstants.Any)))
      ),
      expression.children.filter(_.named).map(astForExpression(_, context))
    )
  }

  private def astForStringLiteral(stringLiteral: KotlinAstNode, context: BodyContext): Ast = {
    val interpolatedChildren = stringLiteral.children.filter(child =>
      child.kind == "interpolated_identifier" || child.kind == "interpolated_expression"
    )
    if (interpolatedChildren.isEmpty) {
      Ast(literalNode(stringLiteral, stringLiteral.code, registerType("java.lang.String")))
    } else {
      val formattedValues = interpolatedChildren.map(astForFormattedValue(_, context))
      callAst(
        operatorCallNode(stringLiteral, stringLiteral.code, Operators.formatString, Some("java.lang.String")),
        formattedValues
      )
    }
  }

  private def astForFormattedValue(interpolated: KotlinAstNode, context: BodyContext): Ast = {
    val (valueCode, valueAst, typeFullName) =
      if (interpolated.kind == "interpolated_identifier") {
        val name     = interpolated.code
        val typeName = context.types.getOrElse(name, TypeConstants.Any)
        (name, identifierAstForName(interpolated, name, typeName, context), typeName)
      } else {
        val expression = interpolated.children.find(isExpressionArgument).getOrElse(interpolated)
        val typeName   = typeForExpression(expression, context).getOrElse(TypeConstants.Any)
        (expression.code, astForExpression(expression, context), typeName)
      }
    callAst(
      operatorCallNode(interpolated, valueCode, Operators.formattedValue, Some(registerType(typeFullName))),
      Seq(valueAst)
    )
  }

  private def astForComparisonExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val operatorName =
      expression.children.find(child => ComparisonOperatorNames.contains(child.kind)).map(_.kind) match {
        case Some(">")  => Operators.greaterThan
        case Some(">=") => Operators.greaterEqualsThan
        case Some("<")  => Operators.lessThan
        case Some("<=") => Operators.lessEqualsThan
        case Some("!=") => Operators.notEquals
        case _          => Operators.equals
      }
    callAst(
      operatorCallNode(expression, expression.code, operatorName, Some("boolean")),
      expression.children.filter(isExpressionArgument).map(astForExpression(_, context))
    )
  }

  private def astForCheckExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val operatorKind = expression.children.find(child => !child.named).map(_.kind)
    val operatorName = operatorKind match {
      case Some("!in") => Operators.notIn
      case Some("in")  => Operators.in
      case _           => Operators.is
    }
    val args = operatorKind match {
      case Some("is") | Some("!is") =>
        val lhsAst = expression.children
          .find(child => child.named && !TypeNodeKinds.contains(child.kind))
          .map(astForExpression(_, context))
        val typeAst =
          expression.children.find(child => child.named && TypeNodeKinds.contains(child.kind)).map(astForTypeNode)
        lhsAst.toSeq ++ typeAst.toSeq
      case _ =>
        expression.children.filter(isExpressionArgument).map(astForExpression(_, context))
    }
    callAst(operatorCallNode(expression, expression.code, operatorName, Some("boolean")), args)
  }

  private def astForAsExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val lhsAst = expression.children
      .find(child => child.named && !TypeNodeKinds.contains(child.kind))
      .map(astForExpression(_, context))
    val typeNodeMaybe = expression.children.find(child => child.named && TypeNodeKinds.contains(child.kind))
    val typeAst       = typeNodeMaybe.map(astForTypeNode)
    val typeFullName  = typeNodeMaybe.map(typeName).getOrElse(TypeConstants.Any)
    callAst(
      operatorCallNode(expression, expression.code, Operators.cast, Some(registerType(typeFullName))),
      lhsAst.toSeq ++ typeAst.toSeq
    )
  }

  private def astForAnnotatedExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val annotations = directAnnotationNodesFor(expression).map(astForAnnotationEntry)
    val expressionAst = expression.children
      .find(child => child.named && child.kind != "annotation")
      .map(astForExpression(_, context))
      .getOrElse(Ast(unknownNode(expression, expression.code)))
    expressionAst.withChildren(annotations)
  }

  private def astForPrefixExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val operatorName = expression.children.find(child => !child.named).map(_.kind) match {
      case Some("!")  => Some(Operators.logicalNot)
      case Some("-")  => Some(Operators.minus)
      case Some("+")  => Some(Operators.plus)
      case Some("++") => Some(Operators.preIncrement)
      case Some("--") => Some(Operators.preDecrement)
      case _          => None
    }
    operatorName match {
      case Some(name) =>
        val operandType = expression.children
          .find(isExpressionArgument)
          .flatMap(typeForExpression(_, context))
          .getOrElse(TypeConstants.Any)
        val typeFullName = if (name == Operators.logicalNot) "boolean" else operandType
        callAst(
          operatorCallNode(expression, expression.code, name, Some(registerType(typeFullName))),
          expression.children.filter(isExpressionArgument).map(astForExpression(_, context))
        )
      case None =>
        expression.children
          .find(isExpressionArgument)
          .map(astForExpression(_, context))
          .getOrElse(Ast(unknownNode(expression, expression.code)))
    }
  }

  private def astForPostfixExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val operatorName = expression.children.find(child => !child.named).map(_.kind) match {
      case Some("!!") => Some(Operators.notNullAssert)
      case Some("++") => Some(Operators.postIncrement)
      case Some("--") => Some(Operators.postDecrement)
      case _          => None
    }
    operatorName match {
      case Some(name) =>
        val operandType = expression.children
          .find(isExpressionArgument)
          .flatMap(typeForExpression(_, context))
          .getOrElse(TypeConstants.Any)
        callAst(
          operatorCallNode(expression, expression.code, name, Some(registerType(operandType))),
          expression.children.filter(isExpressionArgument).map(astForExpression(_, context))
        )
      case None =>
        expression.children
          .find(isExpressionArgument)
          .map(astForExpression(_, context))
          .getOrElse(Ast(unknownNode(expression, expression.code)))
    }
  }

  private def astForAssignmentExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val namedChildren = expression.children.filter(_.named)
    val lhsAst        = namedChildren.headOption.map(astForExpression(_, context))
    val rhsAst        = namedChildren.drop(1).headOption.map(astForExpression(_, context))
    val operatorName = expression.children.find(child => !child.named).map(_.kind) match {
      case Some("+=") => Operators.assignmentPlus
      case Some("-=") => Operators.assignmentMinus
      case Some("*=") => Operators.assignmentMultiplication
      case Some("/=") => Operators.assignmentDivision
      case Some("%=") => Operators.assignmentModulo
      case _          => Operators.assignment
    }
    val typeFullName = if (operatorName == Operators.assignment) TypeConstants.Any else TypeConstants.Void
    callAst(
      operatorCallNode(expression, expression.code, operatorName, Some(typeFullName)),
      lhsAst.toSeq ++ rhsAst.toSeq
    )
  }

  private def astForInfixExpression(expression: KotlinAstNode, context: BodyContext): Ast = {
    val namedChildren = expression.children.filter(_.named)
    namedChildren match {
      case lhsNode :: operatorNode :: rhsNode :: Nil =>
        val lhsTypeFullName = typeForExpression(lhsNode, context)
        val methodInfo =
          lhsTypeFullName.flatMap(owner => methodInfoByOwnerNameAndArity(owner, operatorNode.code, 1))
        val localMethodInfo = context.methods
          .get((operatorNode.code, 2))
          .orElse(topLevelMethodsByNameAndArity.get((operatorNode.code, 2)))
        val resolvedInfo = localMethodInfo.orElse(methodInfo)
        val signature    = resolvedInfo.map(_.signature).getOrElse(s"${Defines.UnresolvedSignature}(2)")
        val targetFullName = resolvedInfo
          .map(_.fullName)
          .getOrElse(methodFullName(s"${Defines.UnresolvedNamespace}.${operatorNode.code}", signature))
        val isExtensionCall = resolvedInfo.exists(_.isExtension)
        val dispatchType =
          if (isExtensionCall || localMethodInfo.nonEmpty) DispatchTypes.STATIC_DISPATCH
          else DispatchTypes.DYNAMIC_DISPATCH
        val call = callNode(
          expression,
          expression.code,
          operatorNode.code,
          targetFullName,
          dispatchType,
          Some(signature),
          Some(resolvedInfo.map(_.returnTypeFullName).getOrElse(TypeConstants.Any))
        )
        val lhsAst = astForExpression(lhsNode, context)
        val rhsAst = astForExpression(rhsNode, context)
        if (isExtensionCall || localMethodInfo.nonEmpty) {
          callAst(call, Seq(lhsAst, rhsAst))
        } else {
          callAst(call, Seq(rhsAst), base = Some(lhsAst))
        }
      case _ =>
        Ast(unknownNode(expression, expression.code))
    }
  }

  private def astForDirectlyAssignableExpression(expression: KotlinAstNode, context: BodyContext): Ast =
    if (expression.children.exists(child => child.kind == "navigation_suffix" || child.kind == "indexing_suffix")) {
      astForAssignableSuffixChain(expression, context)
    } else {
      expression.children.find(_.named).map(astForExpression(_, context)).getOrElse {
        val typeFullName = registerType(context.types.getOrElse(expression.code, TypeConstants.Any))
        val identifier   = identifierNode(expression, expression.code, expression.code, typeFullName)
        context.refs
          .get(expression.code)
          .map(target => Ast(identifier).withRefEdge(identifier, target))
          .getOrElse(Ast(identifier))
      }
    }

  private def astForAssignableSuffixChain(expression: KotlinAstNode, context: BodyContext): Ast = {
    val baseMaybe = expression.children.find(child =>
      child.named && child.kind != "navigation_suffix" && child.kind != "indexing_suffix"
    )
    baseMaybe
      .map { base =>
        val suffixes =
          expression.children.filter(child => child.kind == "navigation_suffix" || child.kind == "indexing_suffix")
        val initial =
          (astForNavigationReceiver(base, context), base.code, receiverExpressionTypeFullName(base, context))
        suffixes
          .foldLeft(initial) { case ((receiverAst, receiverCode, receiverType), suffix) =>
            val code = s"$receiverCode${suffix.code}"
            suffix.kind match {
              case "navigation_suffix" =>
                val fieldName    = navigationSuffixFieldName(suffix)
                val typeFullName = receiverType.flatMap(memberTypeFullName(_, fieldName)).getOrElse(TypeConstants.Any)
                val field        = fieldIdentifierNode(suffix, fieldName, fieldName)
                val ast = callAst(
                  operatorCallNode(expression, code, Operators.fieldAccess, Some(registerType(typeFullName))),
                  Seq(receiverAst, Ast(field))
                )
                (ast, code, Some(typeFullName))
              case "indexing_suffix" =>
                val typeFullName = context.collectionElementTypes
                  .get(receiverCode)
                  .orElse(receiverType.flatMap(indexElementTypeFullName))
                  .getOrElse(TypeConstants.Any)
                val indexAsts = suffix.children.filter(isExpressionArgument).map(astForExpression(_, context))
                val ast = callAst(
                  operatorCallNode(expression, code, Operators.indexAccess, Some(registerType(typeFullName))),
                  receiverAst +: indexAsts
                )
                (ast, code, Some(typeFullName))
              case _ =>
                (receiverAst, code, receiverType)
            }
          }
          ._1
      }
      .getOrElse(Ast(unknownNode(expression, expression.code)))
  }

  private def astForClassLiteralReference(expression: KotlinAstNode): Ast = {
    val typeFullName = registerType(KotlinReflectKClass)
    val call = callNode(
      expression,
      expression.code,
      ClassLiteralOperatorName,
      ClassLiteralOperatorName,
      DispatchTypes.STATIC_DISPATCH,
      Some(methodSignature(typeFullName, Nil)),
      Some(typeFullName)
    )
    Ast(call)
  }

  private def astForCallableReference(
    expression: KotlinAstNode,
    context: BodyContext,
    expectedTypeFullName: Option[String]
  ): Ast = {
    val isUnboundReference =
      expression.kind == "callable_reference" && expression.children.headOption.exists(_.kind == "::")
    val referenceName = callableReferenceName(expression)
    val expectedArity = expectedTypeFullName.flatMap(functionTypeParameterCount)
    val receiverNode  = callableReferenceReceiverNode(expression)

    (isUnboundReference, referenceName, receiverNode) match {
      case (true, Some(name), None) =>
        methodInfoForCallableReference(name, expectedArity, context)
          .map { info =>
            val typeFullName = registerType(callableReferenceTypeFullName(info))
            val methodRef    = methodRefNode(expression, expression.code, info.fullName, typeFullName)
            methodsByFullName
              .get(info.fullName)
              .map(method => Ast(methodRef).withRefEdge(methodRef, method))
              .getOrElse(Ast(methodRef))
          }
          .getOrElse(Ast(unknownNode(expression, expression.code)))
      case (false, Some(name), Some(receiver)) =>
        boundCallableReferenceInfo(expression, receiver, name, expectedArity, context)
          .map(info => astForBoundCallableReference(expression, info))
          .getOrElse(Ast(unknownNode(expression, expression.code)))
      case _ =>
        Ast(unknownNode(expression, expression.code))
    }
  }

  private def callableReferenceReceiverNode(expression: KotlinAstNode): Option[KotlinAstNode] =
    if (expression.kind == "navigation_expression") {
      navigationReceiverNode(expression)
    } else {
      expression.children.takeWhile(_.kind != "::").find(_.named)
    }

  private def callableReferenceName(expression: KotlinAstNode): Option[String] =
    expression.children
      .collectFirst { case child if child.kind == "simple_identifier" => child.code }
      .orElse(navigationSuffixNode(expression).flatMap(_.children.collectFirst {
        case child if child.kind == "simple_identifier" => child.code
      }))

  private def isCallableReferenceNavigationExpression(expression: KotlinAstNode): Boolean =
    expression.kind == "navigation_expression" &&
      navigationSuffixNode(expression).exists(_.children.exists(_.kind == "::"))

  private def boundCallableReferenceInfo(
    expression: KotlinAstNode,
    receiver: KotlinAstNode,
    name: String,
    arity: Option[Int],
    context: BodyContext
  ): Option[BoundCallableReferenceInfo] = {
    val receiverCode = receiver.code
    def targetFor(ownerTypeFullName: String, receiverAst: Ast, isStatic: Boolean): Option[BoundCallableReferenceInfo] =
      arity
        .flatMap(methodInfoByOwnerNameAndArity(ownerTypeFullName, name, _))
        .orElse(uniqueMethodInfoForOwnerCallableReference(ownerTypeFullName, name))
        .map { methodInfo =>
          BoundCallableReferenceInfo(
            methodInfo,
            receiverAst,
            receiverCode,
            ownerTypeFullName,
            isStatic,
            expectedFunctionTypeFullName(methodInfo)
          )
        }

    context.types
      .get(receiverCode)
      .flatMap(receiverType =>
        targetFor(receiverType, identifierAstForName(receiver, receiverCode, receiverType, context), isStatic = false)
      )
      .orElse {
        Option
          .when(receiver.kind == "this_expression") {
            currentReceiverTypeFullName(context).flatMap { receiverType =>
              targetFor(receiverType, thisIdentifierAst(receiver, Constants.ThisName, context), isStatic = false)
            }
          }
          .flatten
      }
      .orElse {
        constructorTargetTypeFullName(receiverCode).flatMap { ownerType =>
          companionObjects
            .get(ownerType)
            .flatMap { companion =>
              targetFor(
                companion.fullName,
                companionCallableReferenceReceiverAst(receiver, ownerType, companion.fullName),
                isStatic = true
              )
            }
            .orElse(
              targetFor(
                ownerType,
                Ast(identifierNode(receiver, receiverCode, receiverCode, registerType(ownerType))),
                isStatic = true
              )
            )
        }
      }
  }

  private def astForBoundCallableReference(expression: KotlinAstNode, info: BoundCallableReferenceInfo): Ast = {
    val samImplClass = registerType(callableReferenceTypeFullName(info.methodInfo))
    ensureBoundCallableReferenceTypeDecl(expression, info, samImplClass)

    val tmpName           = nextTmpLocalName()
    val tmpLocalNode      = localNode(expression, tmpName, tmpName, samImplClass)
    val tmpLocalAst       = Ast(tmpLocalNode)
    val assignmentLhsNode = identifierNode(expression, tmpName, tmpName, samImplClass)
    val assignmentLhsAst  = Ast(assignmentLhsNode).withRefEdge(assignmentLhsNode, tmpLocalNode)
    val assignmentAst = callAst(
      operatorCallNode(expression, s"$tmpName = <alloc>", Operators.assignment, Some(samImplClass)),
      List(assignmentLhsAst, Ast(operatorCallNode(expression, Operators.alloc, Operators.alloc, Some(samImplClass))))
    )

    val initReceiverNode = identifierNode(expression, tmpName, tmpName, samImplClass).argumentIndex(0)
    val initReceiverAst  = Ast(initReceiverNode).withRefEdge(initReceiverNode, tmpLocalNode)
    val ctorSignature    = methodSignature(TypeConstants.Void, Seq(info.receiverTypeFullName))
    val ctorCall = callNode(
      expression,
      s"$samImplClass(${info.receiverCode})",
      Defines.ConstructorMethodName,
      methodFullName(s"$samImplClass.${Defines.ConstructorMethodName}", ctorSignature),
      DispatchTypes.STATIC_DISPATCH,
      Some(ctorSignature),
      Some(TypeConstants.Void)
    )
    val ctorCallAst = callAst(ctorCall, List(info.receiverAst), Some(initReceiverAst))

    val resultNode = identifierNode(expression, tmpName, tmpName, samImplClass)
    val resultAst  = Ast(resultNode).withRefEdge(resultNode, tmpLocalNode)
    blockAst(
      blockNode(expression, expression.code, samImplClass),
      List(tmpLocalAst, assignmentAst, ctorCallAst, resultAst)
    )
  }

  private def ensureBoundCallableReferenceTypeDecl(
    expression: KotlinAstNode,
    info: BoundCallableReferenceInfo,
    samImplClass: String
  ): Unit = {
    if (callableReferenceTypeDeclFullNames.add(samImplClass)) {
      val typeDecl = typeDeclNode(
        expression,
        samImplClass.split('.').last,
        samImplClass,
        document.relativeName,
        Seq(registerType(info.functionTypeFullName), registerType("kotlin.jvm.internal.CallableReference")),
        None
      )
      val invokeAst       = boundCallableReferenceInvokeAst(expression, info, samImplClass)
      val ctorAst         = boundCallableReferenceConstructorAst(expression, info, samImplClass)
      val invokeMethod    = invokeAst.root.collect { case method: NewMethod => method }.get
      val concreteBinding = bindingNode(InvokeMethodName, info.methodInfo.signature, invokeMethod.fullName)
      val erasedBinding = bindingNode(
        InvokeMethodName,
        erasedFunctionSignature(parameterTypesFromSignature(info.methodInfo.signature).size),
        invokeMethod.fullName
      )
      val typeAst = Ast(typeDecl)
        .withChild(invokeAst)
        .withChild(ctorAst)
        .merge(Ast(concreteBinding))
        .merge(Ast(erasedBinding))
        .withBindsEdge(typeDecl, concreteBinding)
        .withBindsEdge(typeDecl, erasedBinding)
        .withRefEdge(concreteBinding, invokeMethod)
        .withRefEdge(erasedBinding, invokeMethod)
      callableReferenceTypeDeclAsts.append(typeAst)
    }
  }

  private def boundCallableReferenceInvokeAst(
    expression: KotlinAstNode,
    info: BoundCallableReferenceInfo,
    samImplClass: String
  ): Ast = {
    val params = parameterTypesFromSignature(info.methodInfo.signature).zipWithIndex.map { case (typeFullName, idx) =>
      (s"p${idx + 1}", typeFullName)
    }
    val invokeFullName = methodFullName(s"$samImplClass.$InvokeMethodName", info.methodInfo.signature)
    val method =
      methodNode(expression, InvokeMethodName, invokeFullName, info.methodInfo.signature, document.relativeName)
    val thisParam =
      parameterInNode(
        expression,
        Constants.ThisName,
        Constants.ThisName,
        0,
        false,
        EvaluationStrategies.BY_SHARING,
        samImplClass
      )
        .dynamicTypeHintFullName(Seq(samImplClass))
    val paramNodes = params.zipWithIndex.map { case ((paramName, paramType), idx) =>
      parameterInNode(
        expression,
        paramName,
        paramName,
        idx + 1,
        false,
        EvaluationStrategies.BY_VALUE,
        registerType(paramType)
      )
    }
    val receiverAccessAst = syntheticReceiverFieldAccessAst(expression, info.receiverTypeFullName, samImplClass)
    val callArgs = params.zip(paramNodes).map { case ((paramName, paramType), paramNode) =>
      val identifier = identifierNode(expression, paramName, paramName, registerType(paramType))
      Ast(identifier).withRefEdge(identifier, paramNode)
    }
    val calledMethodName = methodBaseFullName(info.methodInfo.fullName).split('.').last
    val callCode         = s"${Constants.ReceiverName}.$calledMethodName(${params.map(_._1).mkString(", ")})"
    val call = callNode(
      expression,
      callCode,
      calledMethodName,
      info.methodInfo.fullName,
      if (info.isStatic) DispatchTypes.STATIC_DISPATCH else DispatchTypes.DYNAMIC_DISPATCH,
      Some(info.methodInfo.signature),
      Some(registerType(info.methodInfo.returnTypeFullName))
    )
    val callAst_   = callAst(call, callArgs, Some(receiverAccessAst))
    val returnAst_ = returnAst(returnNode(expression, s"return $callCode"), List(callAst_))
    methodAst(
      method,
      Ast(thisParam) +: paramNodes.map(Ast(_)),
      blockAst(blockNode(expression, s"return $callCode", TypeConstants.JavaLangVoid), List(returnAst_)),
      methodReturnNode(expression, registerType(info.methodInfo.returnTypeFullName)),
      Seq(modifierNode(expression, ModifierTypes.PUBLIC), modifierNode(expression, ModifierTypes.VIRTUAL))
    )
  }

  private def boundCallableReferenceConstructorAst(
    expression: KotlinAstNode,
    info: BoundCallableReferenceInfo,
    samImplClass: String
  ): Ast = {
    val ctorSignature = methodSignature(TypeConstants.Void, Seq(info.receiverTypeFullName))
    val ctorFullName  = methodFullName(s"$samImplClass.${Defines.ConstructorMethodName}", ctorSignature)
    val method =
      methodNode(expression, Defines.ConstructorMethodName, ctorFullName, ctorSignature, document.relativeName)
    val thisParam =
      parameterInNode(
        expression,
        Constants.ThisName,
        Constants.ThisName,
        0,
        false,
        EvaluationStrategies.BY_SHARING,
        samImplClass
      )
        .dynamicTypeHintFullName(Seq(samImplClass))
    val receiverParam = parameterInNode(
      expression,
      Constants.ReceiverName,
      Constants.ReceiverName,
      1,
      false,
      EvaluationStrategies.BY_VALUE,
      registerType(info.receiverTypeFullName)
    )
    val receiverAccessAst = syntheticReceiverFieldAccessAst(expression, info.receiverTypeFullName, samImplClass)
    val receiverIdentifier = identifierNode(
      expression,
      Constants.ReceiverName,
      Constants.ReceiverName,
      registerType(info.receiverTypeFullName)
    ).argumentIndex(2)
    val assignmentAst = callAst(
      operatorCallNode(
        expression,
        s"this.${Constants.ReceiverName} = ${Constants.ReceiverName}",
        Operators.assignment,
        None
      ),
      List(receiverAccessAst, Ast(receiverIdentifier).withRefEdge(receiverIdentifier, receiverParam))
    )
    methodAst(
      method,
      Seq(Ast(thisParam), Ast(receiverParam)),
      blockAst(
        blockNode(expression, s"this.${Constants.ReceiverName} = ${Constants.ReceiverName}", TypeConstants.Void),
        List(assignmentAst)
      ),
      methodReturnNode(expression, TypeConstants.Void),
      Seq(modifierNode(expression, ModifierTypes.CONSTRUCTOR))
    )
  }

  private def syntheticReceiverFieldAccessAst(
    expression: KotlinAstNode,
    receiverTypeFullName: String,
    samImplClass: String
  ): Ast = {
    val thisIdentifier =
      identifierNode(expression, Constants.ThisName, Constants.ThisName, samImplClass, Seq(samImplClass))
    callAst(
      operatorCallNode(
        expression,
        s"this.${Constants.ReceiverName}",
        Operators.fieldAccess,
        Some(registerType(receiverTypeFullName))
      ),
      List(Ast(thisIdentifier), Ast(fieldIdentifierNode(expression, Constants.ReceiverName, Constants.ReceiverName)))
    )
  }

  private def companionCallableReferenceReceiverAst(
    receiver: KotlinAstNode,
    ownerTypeFullName: String,
    companionTypeFullName: String
  ): Ast = {
    val ownerIdentifier = identifierNode(receiver, receiver.code, receiver.code, registerType(ownerTypeFullName))
    val fieldIdentifier =
      fieldIdentifierNode(receiver, Constants.CompanionObjectMemberName, Constants.CompanionObjectMemberName)
    callAst(
      operatorCallNode(
        receiver,
        s"${receiver.code}.${Constants.CompanionObjectMemberName}",
        Operators.fieldAccess,
        Some(registerType(companionTypeFullName))
      ),
      List(Ast(ownerIdentifier), Ast(fieldIdentifier))
    )
  }

  private def uniqueMethodInfoForOwnerCallableReference(owner: String, name: String): Option[MethodInfo] = {
    val candidates = methodsByOwnerNameAndArity.collect {
      case ((candidateOwner, candidateName, _), info) if candidateOwner == owner && candidateName == name => info
    }.toList
    candidates.distinct match {
      case single :: Nil => Some(single)
      case _             => None
    }
  }

  private def expectedFunctionTypeFullName(methodInfo: MethodInfo): String =
    s"kotlin.jvm.functions.Function${parameterTypesFromSignature(methodInfo.signature).size}"

  private def erasedFunctionSignature(arity: Int): String =
    methodSignature(TypeConstants.JavaLangObject, List.fill(arity)(TypeConstants.JavaLangObject))

  private val InvokeMethodName: String = "invoke"

  private def methodInfoForCallableReference(
    name: String,
    arity: Option[Int],
    context: BodyContext
  ): Option[MethodInfo] =
    arity
      .flatMap { value =>
        context.methods.get((name, value)).orElse(topLevelMethodsByNameAndArity.get((name, value)))
      }
      .orElse(uniqueMethodInfoForCallableReference(name, context))

  private def uniqueMethodInfoForCallableReference(name: String, context: BodyContext): Option[MethodInfo] = {
    val candidates =
      context.methods.collect { case ((candidateName, _), info) if candidateName == name => info }.toList ++
        topLevelMethodsByNameAndArity.collect {
          case ((candidateName, _), info) if candidateName == name => info
        }.toList
    candidates.distinct match {
      case single :: Nil => Some(single)
      case _             => None
    }
  }

  private def callableReferenceTypeFullName(methodInfo: MethodInfo): String = {
    val functionFullName = methodInfo.fullName.takeWhile(_ != ':')
    val arity            = parameterCountFromSignature(methodInfo.signature)
    s"$functionFullName$$kotlin.jvm.functions.Function${arity}Impl.invoke:${methodInfo.signature}"
  }

  private def functionTypeParameterCount(typeFullName: String): Option[Int] =
    typeFullName.split("->", 2).headOption.map(_.trim).map { params =>
      val strippedParams = params.stripPrefix("(").stripSuffix(")").trim
      if (strippedParams.isEmpty) {
        0
      } else {
        strippedParams.split(",").length
      }
    }

  private def astForCallExpression(callExpression: KotlinAstNode, context: BodyContext): Ast = {
    val argumentNodes = callArgumentNodes(callExpression)
    val callName      = callNameFor(callExpression)
    constructorInfoForCallExpression(callExpression, context)
      .map(info => constructorCallBlockAst(callExpression, context, info, includeResultIdentifier = true))
      .getOrElse {
        val callTarget         = callTargetExpression(callExpression)
        val receiverNode       = callTarget.children.find(child => child.named && child.kind != "call_suffix")
        val hasPlainCallee     = receiverNode.exists(_.kind != "navigation_expression")
        val navigationReceiver = receiverNode.filter(_.kind == "navigation_expression").flatMap(navigationReceiverNode)
        val receiverAst        = navigationReceiver.map(astForNavigationReceiver(_, context))
        val argumentContext    = callArgumentContext(argumentNodes, navigationReceiver, context)
        val args               = argumentNodes.map(astForExpression(_, argumentContext))
        val argumentTypeFullNames = argumentNodes.map(typeForCallArgument(_, context).getOrElse(TypeConstants.Any))
        val methodInfo = navigationReceiver
          .flatMap(receiverCallTargetTypeFullName(_, context))
          .flatMap(owner => methodInfoByOwnerNameAndArguments(owner, callName, argumentTypeFullNames))
        val implicitReceiverMethodInfo =
          if (hasPlainCallee)
            currentReceiverTypeFullName(context)
              .flatMap(owner => methodInfoByOwnerNameAndArguments(owner, callName, argumentTypeFullNames))
          else None
        val topLevelMethodInfo =
          if (hasPlainCallee)
            topLevelMethodsByNameAndArity.get((callName, args.size))
          else None
        val builtinTopLevelInfo =
          if (hasPlainCallee) builtinTopLevelMethodInfo(callName, argumentNodes, context, Some(callExpression))
          else None
        val localMethodInfo =
          if (hasPlainCallee) context.methods.get((callName, args.size)) else None
        val resolvedViaImplicitReceiver =
          methodInfo.isEmpty && localMethodInfo.isEmpty && implicitReceiverMethodInfo.nonEmpty
        val resolvedInfo =
          methodInfo
            .orElse(localMethodInfo)
            .orElse(implicitReceiverMethodInfo)
            .orElse(topLevelMethodInfo)
            .orElse(builtinTopLevelInfo)
            .map(info => normalizeCallMethodInfo(callName, info, navigationReceiver, argumentNodes, context))
        val signature = resolvedInfo.map(_.signature).getOrElse(s"${Defines.UnresolvedSignature}(${args.size})")
        val targetFullName = resolvedInfo
          .map(_.fullName)
          .getOrElse(
            methodFullName(importAliases.getOrElse(callName, s"${Defines.UnresolvedNamespace}.$callName"), signature)
          )
        val isExtensionCall = resolvedInfo.exists(_.isExtension) && receiverAst.nonEmpty
        val isStaticReceiverCall =
          resolvedInfo.exists(_.isStatic) &&
            navigationReceiver.exists(receiverTargetTypeFullName(_).nonEmpty)
        val dispatchType =
          if (isExtensionCall || isStaticReceiverCall || receiverNode.isEmpty) {
            DispatchTypes.STATIC_DISPATCH
          } else if (resolvedViaImplicitReceiver) {
            if (implicitReceiverMethodInfo.exists(_.isPrivate)) DispatchTypes.STATIC_DISPATCH
            else DispatchTypes.DYNAMIC_DISPATCH
          } else if (hasPlainCallee) {
            DispatchTypes.STATIC_DISPATCH
          } else if (receiverNode.flatMap(navigationReceiverNode).exists(_.kind == "super_expression")) {
            DispatchTypes.STATIC_DISPATCH
          } else {
            DispatchTypes.DYNAMIC_DISPATCH
          }
        val typeFullName =
          resolvedInfo
            .map(info => callReturnTypeFullName(callName, info, navigationReceiver, argumentNodes, context))
            .getOrElse(TypeConstants.Any)
        val call = callNode(
          callExpression,
          callExpression.code,
          callName,
          targetFullName,
          dispatchType,
          Some(signature),
          Some(typeFullName)
        )
        val implicitReceiverAst =
          Option.when(resolvedViaImplicitReceiver)(thisIdentifierAst(callExpression, Constants.ThisName, context))
        val extensionReceiverArgs = if (isExtensionCall) receiverAst.toSeq else Nil
        val baseAst               = Option.when(!isExtensionCall)(receiverAst).flatten.orElse(implicitReceiverAst)
        val receiverOverride =
          Option.when(resolvedViaImplicitReceiver && dispatchType == DispatchTypes.STATIC_DISPATCH)(Ast())
        callAst(call, extensionReceiverArgs ++ args, base = baseAst, receiver = receiverOverride)
      }
  }

  private def callArgumentContext(
    argumentNodes: List[KotlinAstNode],
    navigationReceiver: Option[KotlinAstNode],
    context: BodyContext
  ): BodyContext =
    if (argumentNodes.exists(lambdaLiteralForArgument(_).nonEmpty)) {
      context.copy(
        expectedLambdaElementType = navigationReceiver.flatMap(collectionElementTypeForExpression(_, context)),
        expectedLambdaMapEntryKeyType = navigationReceiver.flatMap(mapEntryKeyTypeForExpression(_, context)),
        expectedLambdaMapEntryValueType = navigationReceiver.flatMap(mapEntryValueTypeForExpression(_, context))
      )
    } else {
      context
    }

  private def astForValueArgument(valueArgument: KotlinAstNode, context: BodyContext): Ast =
    valueArgumentExpressionNode(valueArgument)
      .map(argument => astWithArgumentName(astForExpression(argument, context), valueArgumentName(valueArgument)))
      .getOrElse(Ast(unknownNode(valueArgument, valueArgument.code)))

  private def astForThisExpression(thisExpression: KotlinAstNode, context: BodyContext): Ast = {
    thisIdentifierAst(thisExpression, thisExpression.code, context)
  }

  private def thisIdentifierAst(origin: KotlinAstNode, code: String, context: BodyContext): Ast = {
    val typeFullName = registerType(currentReceiverTypeFullName(context).getOrElse(TypeConstants.Any))
    val identifier   = identifierNode(origin, Constants.ThisName, code, typeFullName, Seq(typeFullName))
    context.refs.get("this").map(ref => Ast(identifier).withRefEdge(identifier, ref)).getOrElse(Ast(identifier))
  }

  private def astForSuperExpression(superExpression: KotlinAstNode, context: BodyContext): Ast = {
    val typeFullName = registerType(superReceiverTypeFullName(context).getOrElse(TypeConstants.Any))
    Ast(identifierNode(superExpression, "super", superExpression.code, typeFullName, Seq(typeFullName)))
  }

  private def valueArgumentExpressionNode(valueArgument: KotlinAstNode): Option[KotlinAstNode] =
    if (valueArgument.children.exists(_.kind == "=")) {
      valueArgument.children.dropWhile(_.kind != "=").drop(1).find(_.named)
    } else {
      valueArgument.children.find(_.named)
    }

  private def valueArgumentName(valueArgument: KotlinAstNode): Option[String] =
    Option
      .when(valueArgument.children.exists(_.kind == "=")) {
        valueArgument.children.takeWhile(_.kind != "=").find(_.kind == "simple_identifier").map(_.code)
      }
      .flatten

  private def astWithArgumentName(ast: Ast, argumentName: Option[String]): Ast = {
    argumentName.foreach { name =>
      ast.root.collect { case root: ExpressionNew =>
        root.argumentName = Some(name)
      }
    }
    ast
  }

  private def constructorInfoForDirectCall(callName: String, argumentCount: Int): Option[MethodInfo] =
    constructorTargetTypeFullName(callName).flatMap(typeFullName =>
      constructorsByTypeAndArity.get((typeFullName, argumentCount))
    )

  private def constructorInfoForCallExpression(
    callExpression: KotlinAstNode,
    context: BodyContext
  ): Option[MethodInfo] = {
    val callName      = callNameFor(callExpression)
    val argumentNodes = callArgumentNodes(callExpression)
    constructorInfoForDirectCall(callName, argumentNodes.size)
      .map(info => info.copy(returnTypeFullName = constructedTypeFullName(info)))
      .orElse(arrayConstructorInfoForCall(callExpression, callName, argumentNodes, context))
      .orElse(externalConstructorInfoForCall(callExpression, callName, argumentNodes, context))
      .orElse(javaLangThrowableConstructorInfo(callName, argumentNodes, context))
  }

  private def arrayConstructorInfoForCall(
    callExpression: KotlinAstNode,
    callName: String,
    argumentNodes: List[KotlinAstNode],
    context: BodyContext
  ): Option[MethodInfo] =
    arrayConstructorReturnTypeFullName(callExpression, callName).map { returnType =>
      val ownerType       = arrayConstructorOwnerTypeFullName(callName)
      val parameterTypes  = arrayConstructorParameterTypes(argumentNodes, context)
      val signature       = methodSignature(TypeConstants.Void, parameterTypes)
      val methodFullName_ = methodFullName(s"$ownerType.${Defines.ConstructorMethodName}", signature)
      MethodInfo(methodFullName_, signature, returnType)
    }

  private def arrayConstructorReturnTypeFullName(callExpression: KotlinAstNode, callName: String): Option[String] =
    if (callName == "Array") {
      Some(s"${typeArgumentTypeFullNames(callExpression).headOption.getOrElse(TypeConstants.JavaLangObject)}[]")
    } else {
      PrimitiveArrayTypeNames.get(callName)
    }

  private def arrayConstructorOwnerTypeFullName(callName: String): String =
    s"kotlin.$callName"

  private def arrayConstructorParameterTypes(argumentNodes: List[KotlinAstNode], context: BodyContext): List[String] =
    argumentNodes.map {
      case argument if lambdaLiteralForArgument(argument).nonEmpty => "kotlin.jvm.functions.Function1"
      case argument => typeForCallArgument(argument, context).getOrElse(TypeConstants.JavaLangObject)
    }

  private def externalConstructorInfoForCall(
    callExpression: KotlinAstNode,
    callName: String,
    argumentNodes: List[KotlinAstNode],
    context: BodyContext
  ): Option[MethodInfo] =
    externalConstructorTypeFullName(callExpression, callName).map { typeFullName =>
      val parameterTypes = argumentNodes.map(argument =>
        valueArgumentExpressionNode(argument).flatMap(typeForExpression(_, context)).getOrElse(TypeConstants.Any)
      )
      val signature = methodSignature(TypeConstants.Void, parameterTypes)
      MethodInfo(methodFullName(s"$typeFullName.${Defines.ConstructorMethodName}", signature), signature, typeFullName)
    }

  private def externalConstructorTypeFullName(callExpression: KotlinAstNode, callName: String): Option[String] =
    constructorTargetTypeFullName(callName)
      .filter(isExternalConstructorTypeFullName)
      .orElse(fullyQualifiedConstructorTypeFullName(callExpression))

  private def fullyQualifiedConstructorTypeFullName(callExpression: KotlinAstNode): Option[String] =
    callExpression.children
      .find(child => child.named && child.kind != "call_suffix")
      .filter(_.kind == "navigation_expression")
      .map(_.code)
      .filter(typeFullName => typeFullName.contains(".") && isTypeLikeName(typeFullName))
      .filter(isStdlibExternalConstructorTypeFullName)
      .filterNot(typeDeclarationInfos.contains)

  private def isExternalConstructorTypeFullName(typeFullName: String): Boolean =
    isTypeLikeName(typeFullName) &&
      isStdlibExternalConstructorTypeFullName(typeFullName) &&
      !typeDeclarationInfos.contains(typeFullName)

  private def isStdlibExternalConstructorTypeFullName(typeFullName: String): Boolean =
    StdlibExternalConstructorPackagePrefixes.exists(typeFullName.startsWith)

  private def isTypeLikeName(typeFullName: String): Boolean =
    typeFullName.split('.').lastOption.flatMap(_.headOption).exists(_.isUpper)

  private def constructorTargetTypeFullName(callName: String): Option[String] =
    typeAliases
      .get(callName)
      .orElse(importAliases.get(callName))
      .orElse(Option.when(callName == "Pair")(PairTypeFullName))
      .orElse(Option.when(callName == "Triple")(TripleTypeFullName))
      .orElse(DefaultTypeFullNames.get(callName))

  private def astForNavigationExpression(navigationExpression: KotlinAstNode, context: BodyContext): Ast = {
    val receiverAst =
      navigationReceiverNode(navigationExpression)
        .map(astForNavigationReceiver(_, context))
        .getOrElse(Ast(unknownNode(navigationExpression, "")))
    val suffix       = navigationSuffixNode(navigationExpression)
    val fieldName    = navigationFieldName(navigationExpression)
    val field        = fieldIdentifierNode(suffix.getOrElse(navigationExpression), fieldName, fieldName)
    val typeFullName = typeForNavigationExpression(navigationExpression, context).getOrElse(TypeConstants.Any)
    callAst(
      operatorCallNode(
        navigationExpression,
        navigationExpression.code,
        Operators.fieldAccess,
        Some(registerType(typeFullName))
      ),
      Seq(receiverAst, Ast(field))
    )
  }

  private def astForNavigationReceiver(receiver: KotlinAstNode, context: BodyContext): Ast =
    if (isClassLiteralReference(receiver)) {
      astForClassLiteralReference(receiver)
    } else {
      companionReceiverAst(receiver).orElse(typeReceiverAst(receiver)).getOrElse(astForExpression(receiver, context))
    }

  private def companionReceiverAst(receiver: KotlinAstNode): Option[Ast] = {
    receiverTargetTypeFullName(receiver).flatMap(companionObjects.get).map { companion =>
      val identifier = identifierNode(receiver, receiver.code, receiver.code, companion.fullName)
      val field =
        fieldIdentifierNode(receiver, Constants.CompanionObjectMemberName, Constants.CompanionObjectMemberName)
      callAst(
        operatorCallNode(receiver, receiver.code, Operators.fieldAccess, Some(companion.fullName)),
        Seq(Ast(identifier), Ast(field))
      )
    }
  }

  private def typeReceiverAst(receiver: KotlinAstNode): Option[Ast] =
    receiverTargetTypeFullName(receiver).map { typeFullName =>
      Ast(identifierNode(receiver, receiver.code, receiver.code, registerType(typeFullName)))
    }

  private def astForIndexingExpression(indexingExpression: KotlinAstNode, context: BodyContext): Ast = {
    val baseAsts = indexingExpression.children
      .find(child => child.named && child.kind != "indexing_suffix")
      .map(astForExpression(_, context))
      .toSeq
    val indexAsts = indexingExpression.children
      .filter(_.kind == "indexing_suffix")
      .flatMap(_.children.filter(isExpressionArgument))
      .map(astForExpression(_, context))
    val typeFullName = typeForIndexingExpression(indexingExpression, context).getOrElse(TypeConstants.Any)
    callAst(
      operatorCallNode(
        indexingExpression,
        indexingExpression.code,
        Operators.indexAccess,
        Some(registerType(typeFullName))
      ),
      baseAsts ++ indexAsts
    )
  }

  private def statementChildren(functionBody: KotlinAstNode): List[KotlinAstNode] = {
    functionBody.children
      .find(_.kind == "statements")
      .map(_.children.filter(_.named))
      .getOrElse(Nil)
  }

  private def statementsChild(node: KotlinAstNode): Option[KotlinAstNode] =
    node.children.find(_.kind == "statements")

  private def controlBodyStatements(body: KotlinAstNode): List[KotlinAstNode] = {
    body.children
      .find(_.kind == "statements")
      .map(_.children.filter(_.named))
      .getOrElse(body.children.filter(_.named))
  }

  private def childContext(context: BodyContext): BodyContext =
    BodyContext(
      mutable.Map.from(context.types),
      mutable.Map.from(context.refs),
      context.ownerMethodFullName,
      mutable.Map.from(context.methods),
      mutable.Map.from(context.collectionElementTypes),
      mutable.Map.from(context.iteratorElementTypes),
      mutable.Map.from(context.mapKeyTypes),
      mutable.Map.from(context.mapValueTypes),
      mutable.Map.from(context.mapEntryKeyTypes),
      mutable.Map.from(context.mapEntryValueTypes),
      context.expectedLambdaElementType,
      context.expectedLambdaReturnType,
      context.expectedLambdaMapEntryKeyType,
      context.expectedLambdaMapEntryValueType,
      mutable.Map.from(context.pairFirstTypes),
      mutable.Map.from(context.pairSecondTypes),
      mutable.Map.from(context.tripleFirstTypes),
      mutable.Map.from(context.tripleSecondTypes),
      mutable.Map.from(context.tripleThirdTypes)
    )

  private def conditionExpression(controlStructure: KotlinAstNode): Option[KotlinAstNode] =
    controlStructure.children.find(child => child.named && child.kind != "control_structure_body")

  private def whenSubjectExpression(whenExpression: KotlinAstNode): Option[KotlinAstNode] =
    whenExpression.children.find(_.kind == "when_subject").flatMap(_.children.find(isExpressionArgument))

  private def isExpressionArgument(node: KotlinAstNode): Boolean =
    node.named || node.kind == "null"

  private def isForIterableNode(node: KotlinAstNode): Boolean =
    node.named && node.kind != "variable_declaration" && node.kind != "multi_variable_declaration" && node.kind != "control_structure_body"

  private def typeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] = {
    expression.kind match {
      case "simple_identifier"                                              => context.types.get(expression.code)
      case "integer_literal"                                                => Some("int")
      case "long_literal"                                                   => Some("long")
      case "hex_literal"                                                    => Some("int")
      case "bin_literal"                                                    => Some("int")
      case "real_literal"                                                   => Some("double")
      case "boolean_literal"                                                => Some("boolean")
      case "character_literal"                                              => Some("char")
      case "string_literal"                                                 => Some("java.lang.String")
      case "null"                                                           => Some("null")
      case "this_expression"                                                => currentReceiverTypeFullName(context)
      case "super_expression"                                               => superReceiverTypeFullName(context)
      case "prefix_expression" if expression.children.exists(_.kind == "!") => Some("boolean")
      case "prefix_expression" =>
        expression.children.find(isExpressionArgument).flatMap(typeForExpression(_, context))
      case "postfix_expression" =>
        expression.children.find(isExpressionArgument).flatMap(typeForExpression(_, context))
      case "parenthesized_expression" =>
        expression.children.find(isExpressionArgument).flatMap(typeForExpression(_, context))
      case "additive_expression" =>
        val operatorName = expression.children.find(child => child.kind == "+" || child.kind == "-").map(_.kind) match {
          case Some("-") => Operators.subtraction
          case _         => Operators.addition
        }
        additiveExpressionTypeFullName(expression, context, operatorName)
      case "multiplicative_expression" =>
        arithmeticExpressionTypeFullName(expression, context)
      case "as_expression"    => expression.children.find(child => TypeNodeKinds.contains(child.kind)).map(typeName)
      case "check_expression" => Some("boolean")
      case "if_expression"    => Some(typeForIfExpression(expression, context))
      case "when_expression"  => Some(typeForWhenExpression(expression, context))
      case "elvis_expression" => Some(typeForElvisExpression(expression, context))
      case "range_expression" => rangeExpressionTypeFullName(expression, context)
      case "navigation_expression" => typeForNavigationExpression(expression, context)
      case "indexing_expression"   => typeForIndexingExpression(expression, context)
      case "callable_reference" if isClassLiteralReference(expression) => Some(KotlinReflectKClass)
      case "call_expression"                                           => typeForCallExpression(expression, context)
      case "infix_expression"                                          => typeForInfixExpression(expression, context)
      case "try_expression"                                            => typeForTryExpression(expression, context)
      case _                                                           => None
    }
  }

  private def integerLiteralType(expectedTypeFullName: Option[String]): String =
    expectedTypeFullName.filter(IntegerLiteralTypeFullNames.contains).getOrElse("int")

  private def arithmeticExpressionTypeFullName(expression: KotlinAstNode, context: BodyContext): Option[String] = {
    val operandTypes = expression.children.filter(isExpressionArgument).flatMap(typeForExpression(_, context)).distinct
    operandTypes match {
      case Nil           => None
      case single :: Nil => Some(promotedArithmeticTypeFullName(single))
      case _ if operandTypes.forall(NumericPrimitiveTypeFullNames.contains) =>
        val promotedOperandTypes = operandTypes.map(promotedArithmeticTypeFullName)
        NumericPrimitiveTypeFullNames.find(promotedOperandTypes.contains)
      case _ =>
        None
    }
  }

  private def additiveExpressionTypeFullName(
    expression: KotlinAstNode,
    context: BodyContext,
    operatorName: String
  ): Option[String] = {
    val operandTypes = expression.children.filter(isExpressionArgument).flatMap(typeForExpression(_, context)).distinct
    collectionOperatorReturnType(expression, context, operatorName)
      .orElse {
        Option.when(operatorName == Operators.addition && operandTypes.contains("java.lang.String"))("java.lang.String")
      }
      .orElse(arithmeticExpressionTypeFullName(expression, context))
  }

  private def collectionOperatorReturnType(
    expression: KotlinAstNode,
    context: BodyContext,
    operatorName: String
  ): Option[String] =
    Option
      .when(operatorName == Operators.addition || operatorName == Operators.subtraction) {
        expression.children.filter(isExpressionArgument).headOption.flatMap(typeForExpression(_, context))
      }
      .flatten
      .flatMap {
        case typeFullName if MapTypeFullNames.contains(typeFullName)          => Some("java.util.Map")
        case typeFullName if SetOperatorTypeFullNames.contains(typeFullName)  => Some("java.util.Set")
        case typeFullName if ListOperatorTypeFullNames.contains(typeFullName) => Some("java.util.List")
        case _                                                                => None
      }

  private def rangeExpressionTypeFullName(expression: KotlinAstNode, context: BodyContext): Option[String] = {
    val operandTypes = expression.children.filter(isExpressionArgument).flatMap(typeForExpression(_, context)).distinct
    if (operandTypes.contains("char")) {
      Some(CharRangeTypeFullName)
    } else if (operandTypes.contains("long")) {
      Some(LongRangeTypeFullName)
    } else if (operandTypes.nonEmpty && operandTypes.forall(IntegerLiteralTypeFullNames.contains)) {
      Some(IntRangeTypeFullName)
    } else {
      None
    }
  }

  private def rangeElementTypeFullName(typeFullName: String): Option[String] =
    typeFullName match {
      case IntRangeTypeFullName | IntProgressionTypeFullName   => Some("int")
      case LongRangeTypeFullName | LongProgressionTypeFullName => Some("long")
      case CharRangeTypeFullName | CharProgressionTypeFullName => Some("char")
      case _                                                   => None
    }

  private def rangeProgressionTypeFullName(typeFullName: String): Option[String] =
    typeFullName match {
      case IntRangeTypeFullName | IntProgressionTypeFullName   => Some(IntProgressionTypeFullName)
      case LongRangeTypeFullName | LongProgressionTypeFullName => Some(LongProgressionTypeFullName)
      case CharRangeTypeFullName | CharProgressionTypeFullName => Some(CharProgressionTypeFullName)
      case _                                                   => None
    }

  private def promotedArithmeticTypeFullName(typeFullName: String): String =
    typeFullName match {
      case "byte" | "short" => "int"
      case _                => typeFullName
    }

  private def typeForIfExpression(ifExpression: KotlinAstNode, context: BodyContext): String = {
    val branchTypes = ifExpression.children
      .filter(_.kind == "control_structure_body")
      .flatMap(body => controlBodyStatements(body).lastOption.flatMap(typeForExpression(_, context)))
      .distinct
    branchTypes match {
      case Seq(singleType) => singleType
      case _               => TypeConstants.Any
    }
  }

  private def typeForWhenExpression(whenExpression: KotlinAstNode, context: BodyContext): String = {
    val branchTypes = whenExpression.children
      .filter(_.kind == "when_entry")
      .flatMap(_.children.find(_.kind == "control_structure_body"))
      .flatMap(body => controlBodyStatements(body).lastOption.flatMap(typeForExpression(_, context)))
      .distinct
    branchTypes match {
      case Seq(singleType) => singleType
      case _               => TypeConstants.Any
    }
  }

  private def typeForElvisExpression(elvisExpression: KotlinAstNode, context: BodyContext): String = {
    val branchTypes = elvisExpression.children.filter(isExpressionArgument).flatMap(typeForExpression(_, context))
    branchTypes.distinct match {
      case single :: Nil => single
      case types if types.nonEmpty && types.contains("null") =>
        types.filterNot(_ == "null").headOption.getOrElse(TypeConstants.Any)
      case _ =>
        TypeConstants.Any
    }
  }

  private def typeForNavigationExpression(navigationExpression: KotlinAstNode, context: BodyContext): Option[String] = {
    val fieldName = navigationFieldName(navigationExpression)
    if (fieldName == "length") {
      Some("int")
    } else if (fieldName == "java") {
      navigationReceiverNode(navigationExpression).filter(isClassLiteralReference).map(_ => "java.lang.Class")
    } else {
      navigationReceiverNode(navigationExpression)
        .flatMap(receiverExpressionTypeFullName(_, context))
        .flatMap(typeFullName => memberTypeFullName(typeFullName, fieldName))
        .orElse(
          navigationReceiverNode(navigationExpression).flatMap(receiver =>
            expressionSpecificMemberTypeFullName(receiver, fieldName, context)
          )
        )
    }
  }

  private def expressionSpecificMemberTypeFullName(
    receiver: KotlinAstNode,
    memberName: String,
    context: BodyContext
  ): Option[String] =
    receiverExpressionTypeFullName(receiver, context).flatMap {
      case MapEntryTypeFullName =>
        memberName match {
          case "key"   => mapEntryKeyTypeForExpression(receiver, context)
          case "value" => mapEntryValueTypeForExpression(receiver, context)
          case _       => None
        }
      case PairTypeFullName =>
        memberName match {
          case "first"  => pairFirstTypeForExpression(receiver, context).orElse(Some(TypeConstants.JavaLangObject))
          case "second" => pairSecondTypeForExpression(receiver, context).orElse(Some(TypeConstants.JavaLangObject))
          case _        => None
        }
      case TripleTypeFullName =>
        memberName match {
          case "first"  => tripleFirstTypeForExpression(receiver, context).orElse(Some(TypeConstants.JavaLangObject))
          case "second" => tripleSecondTypeForExpression(receiver, context).orElse(Some(TypeConstants.JavaLangObject))
          case "third"  => tripleThirdTypeForExpression(receiver, context).orElse(Some(TypeConstants.JavaLangObject))
          case _        => None
        }
      case _ =>
        None
    }

  private def typeForIndexingExpression(indexingExpression: KotlinAstNode, context: BodyContext): Option[String] =
    indexingExpression.children
      .find(child => child.named && child.kind != "indexing_suffix")
      .flatMap { receiver =>
        collectionElementTypeForExpression(receiver, context)
          .orElse(receiverExpressionTypeFullName(receiver, context).flatMap(indexElementTypeFullName))
      }

  private def typeForCallExpression(callExpression: KotlinAstNode, context: BodyContext): Option[String] = {
    val callName              = callNameFor(callExpression)
    val argumentNodes         = callArgumentNodes(callExpression)
    val argumentCount         = argumentNodes.size
    val argumentTypeFullNames = argumentNodes.map(typeForCallArgument(_, context).getOrElse(TypeConstants.Any))
    val declaredMethodReturnType =
      context.methods
        .get((callName, argumentCount))
        .orElse(topLevelMethodsByNameAndArity.get((callName, argumentCount)))
        .map(_.returnTypeFullName)
    val builtinCollectionReturnType =
      Option
        .when(BuiltinCollectionFactoryNames(callName) && declaredMethodReturnType.isEmpty) {
          builtinTopLevelMethodInfo(callName, argumentNodes, context, Some(callExpression)).map(_.returnTypeFullName)
        }
        .flatten
    builtinCollectionReturnType
      .orElse(constructorInfoForCallExpression(callExpression, context).map(_.returnTypeFullName))
      .orElse {
        if (TypeArgumentNonReturnCallNames.contains(callName)) None
        else typeArgumentTypeFullNames(callExpression).headOption
      }
      .orElse {
        val callTarget         = callTargetExpression(callExpression)
        val receiverNode       = callTarget.children.find(child => child.named && child.kind != "call_suffix")
        val navigationReceiver = receiverNode.filter(_.kind == "navigation_expression").flatMap(navigationReceiverNode)
        navigationReceiver
          .flatMap(receiverCallTargetTypeFullName(_, context))
          .flatMap(owner =>
            methodInfoByOwnerNameAndArguments(owner, callName, argumentTypeFullNames)
              .map(info => normalizeCallMethodInfo(callName, info, navigationReceiver, argumentNodes, context))
              .map(info => callReturnTypeFullName(callName, info, navigationReceiver, argumentNodes, context))
          )
      }
      .orElse(declaredMethodReturnType)
      .orElse(
        builtinTopLevelMethodInfo(callName, argumentNodes, context, Some(callExpression)).map(_.returnTypeFullName)
      )
  }

  private def callReturnTypeFullName(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    argumentNodes: List[KotlinAstNode],
    context: BodyContext
  ): String =
    extensionCollectionLambdaResultReturnType(callName, methodInfo, receiver, argumentNodes, context)
      .orElse(extensionCollectionElementReturnType(callName, methodInfo, receiver, context))
      .orElse(toTypedArrayReturnType(callName, methodInfo, receiver, context))
      .orElse(arrayMemberElementReturnType(callName, methodInfo, receiver, context))
      .orElse(listMemberElementReturnType(callName, methodInfo, receiver, context))
      .orElse(mapValueReturnType(callName, methodInfo, receiver, context))
      .orElse(pairComponentReturnType(callName, methodInfo, receiver, context))
      .orElse(tripleComponentReturnType(callName, methodInfo, receiver, context))
      .getOrElse(methodInfo.returnTypeFullName)

  private def normalizeCallMethodInfo(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    argumentNodes: List[KotlinAstNode],
    context: BodyContext
  ): MethodInfo =
    if (methodInfo.isExtension && callName == "sumOf" && !isUnresolvedMethodInfo(methodInfo)) {
      val returnType = extensionLambdaResultType(receiver, argumentNodes, context)
        .filter(NumericPrimitiveTypeFullNames.contains)
        .getOrElse("int")
      val isSequenceSumOf = methodInfo.fullName.startsWith("kotlin.sequences.sumOf:")
      val arrayReceiverType =
        receiver
          .flatMap(receiverExpressionTypeFullName(_, context))
          .filter(isArrayTypeFullName)
          .map(arrayReceiverTypeForMemberSignature)
      val mapReceiverType =
        receiver
          .flatMap(receiverExpressionTypeFullName(_, context))
          .filter(MapTypeFullNames.contains)
          .map(_ => "java.util.Map")
      val receiverType = arrayReceiverType.getOrElse {
        mapReceiverType.getOrElse {
          if (isSequenceSumOf) "kotlin.sequences.Sequence" else "java.lang.Iterable"
        }
      }
      val packageName = if (isSequenceSumOf) "kotlin.sequences" else "kotlin.collections"
      val signature   = methodSignature(returnType, Seq(receiverType, "kotlin.jvm.functions.Function1"))
      methodInfo.copy(
        fullName = methodFullName(s"$packageName.sumOf", signature),
        signature = signature,
        returnTypeFullName = returnType
      )
    } else {
      methodInfo
    }

  private def extensionCollectionLambdaResultReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    argumentNodes: List[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(
        methodInfo.isExtension && !isUnresolvedMethodInfo(methodInfo) && IterableLambdaResultReturnNames
          .contains(callName)
      ) {
        extensionLambdaResultType(receiver, argumentNodes, context)
      }
      .flatten

  private def extensionLambdaResultType(
    receiver: Option[KotlinAstNode],
    argumentNodes: List[KotlinAstNode],
    context: BodyContext
  ): Option[String] = {
    val receiverElementType       = receiver.flatMap(iterableElementTypeForExpression(_, context))
    val receiverMapEntryKeyType   = receiver.flatMap(mapEntryKeyTypeForExpression(_, context))
    val receiverMapEntryValueType = receiver.flatMap(mapEntryValueTypeForExpression(_, context))
    argumentNodes.reverseIterator
      .flatMap(argument =>
        lambdaResultTypeForArgument(
          argument,
          receiverElementType,
          receiverMapEntryKeyType,
          receiverMapEntryValueType,
          context
        )
      )
      .toSeq
      .headOption
  }

  private def lambdaResultTypeForArgument(
    argument: KotlinAstNode,
    receiverElementType: Option[String],
    receiverMapEntryKeyType: Option[String],
    receiverMapEntryValueType: Option[String],
    context: BodyContext
  ): Option[String] =
    lambdaLiteralForArgument(argument)
      .flatMap(
        lambdaLiteralResultType(_, receiverElementType, receiverMapEntryKeyType, receiverMapEntryValueType, context)
      )

  private def lambdaLiteralForArgument(argument: KotlinAstNode): Option[KotlinAstNode] =
    argument.kind match {
      case "value_argument" =>
        valueArgumentExpressionNode(argument).flatMap(lambdaLiteralForArgument)
      case "annotated_lambda" =>
        argument.children.find(_.kind == "lambda_literal")
      case "lambda_literal" =>
        Some(argument)
      case _ =>
        None
    }

  private def lambdaLiteralResultType(
    lambdaLiteral: KotlinAstNode,
    receiverElementType: Option[String],
    receiverMapEntryKeyType: Option[String],
    receiverMapEntryValueType: Option[String],
    context: BodyContext
  ): Option[String] = {
    val explicitParams = lambdaParameterInfos(lambdaLiteral)
    val params =
      if (explicitParams.nonEmpty) {
        explicitParams.map { param =>
          Option
            .when(param.typeFullName == TypeConstants.Any && explicitParams.sizeCompare(1) == 0) {
              param.copy(
                typeFullName = receiverElementType.getOrElse(param.typeFullName),
                mapEntryKeyTypeFullName = receiverMapEntryKeyType.orElse(param.mapEntryKeyTypeFullName),
                mapEntryValueTypeFullName = receiverMapEntryValueType.orElse(param.mapEntryValueTypeFullName)
              )
            }
            .getOrElse(param)
        }
      } else if (lambdaUsesImplicitIt(lambdaLiteral)) {
        List(
          ParameterInfo(
            lambdaLiteral,
            "it",
            receiverElementType.getOrElse(TypeConstants.Any),
            "it",
            declaresMember = false,
            mapEntryKeyTypeFullName = receiverMapEntryKeyType,
            mapEntryValueTypeFullName = receiverMapEntryValueType
          )
        )
      } else {
        Nil
      }
    val lambdaContext = childContext(context)
    params.foreach { param =>
      lambdaContext.types.update(param.name, param.typeFullName)
      updateParameterTypeMetadata(param, lambdaContext)
    }
    lambdaStatements(lambdaLiteral).lastOption.flatMap(typeForExpression(_, lambdaContext))
  }

  private def extensionCollectionElementReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(
        methodInfo.isExtension && !isUnresolvedMethodInfo(methodInfo) && ExtensionElementReturnNames
          .contains(callName) &&
          !isMapValueReturnMethod(callName, methodInfo)
      ) {
        receiver.flatMap(iterableElementTypeForExpression(_, context))
      }
      .flatten

  private def isMapValueReturnMethod(callName: String, methodInfo: MethodInfo): Boolean =
    MapValueReturnNames.contains(callName) && methodInfo.signature.contains("java.util.Map")

  private def isUnresolvedMethodInfo(methodInfo: MethodInfo): Boolean =
    methodInfo.fullName.startsWith(s"${Defines.UnresolvedNamespace}.")

  private def toTypedArrayReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(callName == "toTypedArray" && methodInfo.fullName.startsWith("kotlin.collections.toTypedArray:")) {
        receiver
          .flatMap(receiverExpressionTypeFullName(_, context))
          .filter(isPrimitiveArrayTypeFullName)
          .orElse(
            receiver.flatMap(collectionElementTypeForExpression(_, context)).map(elementType => s"$elementType[]")
          )
      }
      .flatten

  private def arrayMemberElementReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(callName == "get" && methodReturnsArrayElement(methodInfo)) {
        receiver.flatMap(receiverExpressionTypeFullName(_, context)).flatMap(indexElementTypeFullName)
      }
      .flatten

  private def methodReturnsArrayElement(methodInfo: MethodInfo): Boolean =
    methodInfo.fullName.startsWith("kotlin.Array.get:") ||
      PrimitiveArrayTypeNames.keys.exists(name => methodInfo.fullName.startsWith(s"kotlin.$name.get:"))

  private def listMemberElementReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(ListMemberElementReturnNames.contains(callName) && methodReturnsListElement(methodInfo)) {
        receiver.flatMap(collectionElementTypeForExpression(_, context))
      }
      .flatten

  private def methodReturnsListElement(methodInfo: MethodInfo): Boolean =
    methodInfo.fullName.startsWith("kotlin.collections.List.") ||
      methodInfo.fullName.startsWith("kotlin.collections.MutableList.")

  private def mapValueReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(MapValueReturnNames.contains(callName) && methodReturnsMapValue(methodInfo)) {
        receiver.flatMap(mapValueTypeForExpression(_, context))
      }
      .flatten

  private def methodReturnsMapValue(methodInfo: MethodInfo): Boolean =
    methodInfo.fullName.startsWith("kotlin.collections.Map.") ||
      methodInfo.fullName.startsWith("kotlin.collections.MutableMap.") ||
      methodInfo.fullName.startsWith("kotlin.collections.getValue:") ||
      methodInfo.fullName.startsWith("kotlin.collections.getOrElse:") ||
      methodInfo.fullName.startsWith("kotlin.collections.getOrPut:")

  private def pairComponentReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(methodInfo.fullName.startsWith(s"$PairTypeFullName.") && PairComponentNames.contains(callName)) {
        if (callName == "component1") {
          receiver.flatMap(pairFirstTypeForExpression(_, context))
        } else {
          receiver.flatMap(pairSecondTypeForExpression(_, context))
        }
      }
      .flatten

  private def tripleComponentReturnType(
    callName: String,
    methodInfo: MethodInfo,
    receiver: Option[KotlinAstNode],
    context: BodyContext
  ): Option[String] =
    Option
      .when(methodInfo.fullName.startsWith(s"$TripleTypeFullName.") && TripleComponentNames.contains(callName)) {
        callName match {
          case "component1" => receiver.flatMap(tripleFirstTypeForExpression(_, context))
          case "component2" => receiver.flatMap(tripleSecondTypeForExpression(_, context))
          case _            => receiver.flatMap(tripleThirdTypeForExpression(_, context))
        }
      }
      .flatten

  private def typeForInfixExpression(infixExpression: KotlinAstNode, context: BodyContext): Option[String] = {
    val namedChildren = infixExpression.children.filter(_.named)
    namedChildren match {
      case lhsNode :: operatorNode :: _ :: Nil =>
        context.methods
          .get((operatorNode.code, 2))
          .orElse(topLevelMethodsByNameAndArity.get((operatorNode.code, 2)))
          .orElse {
            typeForExpression(lhsNode, context)
              .flatMap(owner => methodInfoByOwnerNameAndArity(owner, operatorNode.code, 1))
          }
          .map(_.returnTypeFullName)
      case _ =>
        None
    }
  }

  private def typeForTryExpression(tryExpression: KotlinAstNode, context: BodyContext): Option[String] =
    statementsChild(tryExpression)
      .flatMap(_.children.filter(_.named).lastOption)
      .flatMap(typeForExpression(_, context))

  private def callArgumentNodes(callExpression: KotlinAstNode): List[KotlinAstNode] =
    nestedCallExpressionWithTrailingLambda(callExpression)
      .map(nestedCall => directCallArgumentNodes(nestedCall) ++ directCallArgumentNodes(callExpression))
      .getOrElse(directCallArgumentNodes(callExpression))

  private def nestedCallExpressionWithTrailingLambda(callExpression: KotlinAstNode): Option[KotlinAstNode] =
    Option
      .when(directCallArgumentNodes(callExpression).exists(_.kind == "annotated_lambda")) {
        callExpression.children.find(child => child.named && child.kind == "call_expression")
      }
      .flatten

  private def callTargetExpression(callExpression: KotlinAstNode): KotlinAstNode =
    nestedCallExpressionWithTrailingLambda(callExpression).getOrElse(callExpression)

  private def navigationReceiverForCallExpression(callExpression: KotlinAstNode): Option[KotlinAstNode] = {
    val callTarget   = callTargetExpression(callExpression)
    val receiverNode = callTarget.children.find(child => child.named && child.kind != "call_suffix")
    receiverNode.filter(_.kind == "navigation_expression").flatMap(navigationReceiverNode)
  }

  private def directCallArgumentNodes(callExpression: KotlinAstNode): List[KotlinAstNode] = {
    callExpression.children
      .filter(_.kind == "call_suffix")
      .flatMap { suffix =>
        val valueArguments = suffix.children
          .find(_.kind == "value_arguments")
          .map(_.children.filter(_.kind == "value_argument"))
          .getOrElse(Nil)
        valueArguments ++ suffix.children.filter(_.kind == "annotated_lambda")
      }
  }

  private def typeArgumentTypeFullNames(callExpression: KotlinAstNode): List[String] =
    callTargetExpression(callExpression).children
      .filter(_.kind == "call_suffix")
      .flatMap(_.children.filter(_.kind == "type_arguments"))
      .flatMap(_.children.filter(_.kind == "type_projection"))
      .map(typeProjection =>
        typeProjection.children
          .find(child => TypeNodeKinds.contains(child.kind))
          .map(typeName)
          .getOrElse(typeName(typeProjection))
      )

  private def valueArgumentNodes(node: KotlinAstNode): List[KotlinAstNode] =
    node.children
      .filter(_.kind == "value_arguments")
      .flatMap(_.children.filter(_.kind == "value_argument"))

  private def lambdaParameterInfos(lambdaLiteral: KotlinAstNode): List[ParameterInfo] = {
    lambdaLiteral.children
      .find(_.kind == "lambda_parameters")
      .toList
      .flatMap(_.children.filter(_.kind == "variable_declaration"))
      .map { declaration =>
        val name = firstChildCode(declaration, "simple_identifier").getOrElse(nameFromDeclarationCode(declaration.code))
        val rawTypeText = declaration.children
          .find(child => TypeNodeKinds.contains(child.kind))
          .map(_.code)
          .orElse(rawTypeTextFromDeclarationText(declaration.code))
        val typeName = typeFromDirectChildren(declaration)
          .orElse(typeFromDeclarationText(declaration.code))
          .getOrElse(TypeConstants.Any)
        ParameterInfo(
          declaration,
          name,
          registerType(typeName),
          declaration.code,
          declaresMember = false,
          collectionElementTypeFullName = collectionElementTypeFromDirectChildren(declaration)
            .orElse(collectionElementTypeFromDeclarationText(declaration.code)),
          mapKeyTypeFullName = rawTypeText.flatMap(typeText => mapKeyTypeFromTypeText(typeText)),
          mapValueTypeFullName = rawTypeText.flatMap(typeText => mapValueTypeFromTypeText(typeText)),
          mapEntryKeyTypeFullName = rawTypeText.flatMap(typeText => mapEntryKeyTypeFromTypeText(typeText)),
          mapEntryValueTypeFullName = rawTypeText.flatMap(typeText => mapEntryValueTypeFromTypeText(typeText)),
          pairFirstTypeFullName = rawTypeText.flatMap(typeText => pairTypesFromTypeText(typeText).map(_._1)),
          pairSecondTypeFullName = rawTypeText.flatMap(typeText => pairTypesFromTypeText(typeText).map(_._2)),
          tripleFirstTypeFullName = rawTypeText.flatMap(typeText => tripleTypesFromTypeText(typeText).map(_._1)),
          tripleSecondTypeFullName = rawTypeText.flatMap(typeText => tripleTypesFromTypeText(typeText).map(_._2)),
          tripleThirdTypeFullName = rawTypeText.flatMap(typeText => tripleTypesFromTypeText(typeText).map(_._3))
        )
      }
  }

  private def lambdaStatements(lambdaLiteral: KotlinAstNode): List[KotlinAstNode] = {
    lambdaLiteral.children
      .find(_.kind == "statements")
      .map(_.children.filter(_.named))
      .getOrElse(Nil)
  }

  private def lambdaUsesImplicitIt(lambdaLiteral: KotlinAstNode): Boolean =
    lambdaStatements(lambdaLiteral).exists(statement =>
      (statement.kind == "simple_identifier" && statement.code == "it") ||
        statement.descendants.exists(node => node.kind == "simple_identifier" && node.code == "it")
    )

  private def callNameFor(callExpression: KotlinAstNode): String = {
    val callTarget = callTargetExpression(callExpression)
    callTarget.children.find(child => child.named && child.kind != "call_suffix") match {
      case Some(callee) if callee.kind == "navigation_expression" =>
        navigationFieldName(callee)
      case Some(callee) => callee.code
      case None         => callTarget.code.takeWhile(_ != '(').trim
    }
  }

  private def astForTypeNode(node: KotlinAstNode): Ast =
    Ast(typeRefNode(node, node.code, registerType(typeName(node))))

  private def navigationReceiverNode(navigationExpression: KotlinAstNode): Option[KotlinAstNode] =
    navigationExpression.children.find(child => child.named && child.kind != "navigation_suffix")

  private def navigationSuffixNode(navigationExpression: KotlinAstNode): Option[KotlinAstNode] =
    navigationExpression.children.find(_.kind == "navigation_suffix")

  private def navigationFieldName(navigationExpression: KotlinAstNode): String =
    navigationSuffixNode(navigationExpression)
      .map(navigationSuffixFieldName)
      .getOrElse(navigationExpression.code.split('.').lastOption.getOrElse(navigationExpression.code))

  private def navigationSuffixFieldName(navigationSuffix: KotlinAstNode): String =
    navigationSuffix.descendants
      .find(_.kind == "simple_identifier")
      .map(_.code)
      .getOrElse {
        val strippedPrefix = navigationSuffix.code.stripPrefix(".").stripPrefix("?.")
        strippedPrefix
          .split('.')
          .lastOption
          .getOrElse(strippedPrefix)
          .takeWhile(_ != '(')
          .trim
      }

  private def receiverTargetTypeFullName(receiver: KotlinAstNode): Option[String] =
    receiver.kind match {
      case "simple_identifier" =>
        typeAliases
          .get(receiver.code)
          .orElse(importAliases.get(receiver.code))
          .orElse(DefaultTypeFullNames.get(receiver.code))
      case _ => None
    }

  private def receiverCallTargetTypeFullName(receiver: KotlinAstNode, context: BodyContext): Option[String] =
    receiver.kind match {
      case "simple_identifier" => context.types.get(receiver.code).orElse(receiverTargetTypeFullName(receiver))
      case "this_expression"   => currentReceiverTypeFullName(context)
      case "super_expression"  => superReceiverTypeFullName(context)
      case _                   => receiverExpressionTypeFullName(receiver, context)
    }

  private def receiverExpressionTypeFullName(receiver: KotlinAstNode, context: BodyContext): Option[String] =
    receiver.kind match {
      case "simple_identifier" => context.types.get(receiver.code).orElse(receiverTargetTypeFullName(receiver))
      case "this_expression"   => currentReceiverTypeFullName(context)
      case "super_expression"  => superReceiverTypeFullName(context)
      case _                   => typeForExpression(receiver, context).orElse(receiverTargetTypeFullName(receiver))
    }

  private def memberTypeFullName(typeFullName: String, memberName: String): Option[String] =
    memberTypeFullName(typeFullName, memberName, Set.empty)

  private def indexElementTypeFullName(typeFullName: String): Option[String] =
    Option.when(typeFullName.endsWith("[]"))(typeFullName.stripSuffix("[]"))

  private def memberTypeFullName(typeFullName: String, memberName: String, visited: Set[String]): Option[String] =
    if (visited.contains(typeFullName)) {
      None
    } else {
      typeDeclarationInfos
        .get(typeFullName)
        .flatMap(_.members.get(memberName))
        .orElse(builtinMemberTypeFullName(typeFullName, memberName))
        .orElse(
          inheritedTypesByFullName
            .get(typeFullName)
            .flatMap(
              _.iterator
                .flatMap(inherited => memberTypeFullName(inherited, memberName, visited + typeFullName))
                .toSeq
                .headOption
            )
        )
    }

  private def builtinMemberTypeFullName(typeFullName: String, memberName: String): Option[String] =
    Option
      .when(CollectionInterfaceTypeFullNames.contains(typeFullName))(CollectionMemberReturnTypes.get(memberName))
      .flatten
      .orElse(
        Option.when(ListInterfaceTypeFullNames.contains(typeFullName))(ListMemberReturnTypes.get(memberName)).flatten
      )
      .orElse(Option.when(typeFullName.endsWith("[]"))(ArrayMemberReturnTypes.get(memberName)).flatten)
      .orElse(Option.when(MapTypeFullNames.contains(typeFullName))(MapMemberReturnTypes.get(memberName)).flatten)

  private def currentReceiverTypeFullName(context: BodyContext): Option[String] =
    context.types.get("this")

  private def superReceiverTypeFullName(context: BodyContext): Option[String] =
    currentReceiverTypeFullName(context)
      .flatMap(inheritedTypesByFullName.get)
      .flatMap(_.find(_ != TypeConstants.JavaLangObject))

  private def isClassLiteralReference(node: KotlinAstNode): Boolean =
    node.kind == "callable_reference" &&
      node.children.exists(_.kind == "::") &&
      node.children.exists(child => child.kind == "class" && child.code == "class")

  private def initializerNode(node: KotlinAstNode): Option[KotlinAstNode] =
    node.children.dropWhile(_.kind != "=").drop(1).find(_.named)

  private def bindMethodsToType(typeAst: Ast, typeDecl: NewTypeDecl, methodAsts: List[Ast]): Ast = {
    methodAsts
      .flatMap(_.root.collect { case method: NewMethod => method })
      .foldLeft(typeAst) { case (acc, method) =>
        val binding = bindingNode(method.name, method.signature, method.fullName)
        acc
          .merge(Ast(binding))
          .withBindsEdge(typeDecl, binding)
          .withRefEdge(binding, method)
      }
  }

  private def bindErasedGenericMethodsToType(
    classDeclaration: KotlinAstNode,
    typeAst: Ast,
    typeDecl: NewTypeDecl,
    methodAsts: List[Ast]
  ): Ast = {
    val genericMethods = genericSuperTypeInfos(classDeclaration).flatMap(_.methods)
    val seen           = mutable.Set.empty[(String, String, String)]
    methodAsts
      .flatMap(_.root.collect { case method: NewMethod => method })
      .foldLeft(typeAst) { case (acc, method) =>
        genericMethods
          .filter(superMethod =>
            superMethod.name == method.name &&
              superMethod.parameterCount == parameterCountFromSignature(method.signature) &&
              superMethod.signature != method.signature
          )
          .foldLeft(acc) { case (innerAcc, superMethod) =>
            val key = (method.name, superMethod.signature, method.fullName)
            if (seen.add(key)) {
              val binding = bindingNode(method.name, superMethod.signature, method.fullName)
              innerAcc
                .merge(Ast(binding))
                .withBindsEdge(typeDecl, binding)
                .withRefEdge(binding, method)
            } else {
              innerAcc
            }
          }
      }
  }

  private def parameterCountFromSignature(signature: String): Int =
    signature
      .split("\\(", 2)
      .lift(1)
      .map(_.stripSuffix(")"))
      .filter(_.nonEmpty)
      .map(_.split(",").length)
      .getOrElse(0)

  private def parameterTypesFromSignature(signature: String): List[String] =
    signature
      .split("\\(", 2)
      .lift(1)
      .map(_.stripSuffix(")"))
      .filter(_.nonEmpty)
      .map(_.split(",").toList)
      .getOrElse(Nil)

  private def genericSuperTypeInfos(classDeclaration: KotlinAstNode): List[TypeInfo] =
    inheritedUserTypeNodes(classDeclaration)
      .filter(_.children.exists(_.kind == "type_arguments"))
      .flatMap(node =>
        typeDeclarationInfos.get(typeName(node)).orElse(typeDeclarationInfos.get(baseTypeName(node.code)))
      )
      .filter(_.typeParameterNames.nonEmpty)

  private def inheritsForTypeDeclaration(classDeclaration: KotlinAstNode): Seq[String] = {
    val inheritedTypes = inheritedUserTypeNodes(classDeclaration).map(node => registerType(typeName(node)))
    if (inheritedTypes.nonEmpty) inheritedTypes else Seq(registerType(TypeConstants.JavaLangObject))
  }

  private def inheritedUserTypeNodes(classDeclaration: KotlinAstNode): List[KotlinAstNode] =
    classDeclaration.children
      .filter(_.kind == "delegation_specifier")
      .flatMap(delegation =>
        delegation.children.find(_.kind == "user_type").orElse(delegation.descendants.find(_.kind == "user_type"))
      )

  private def primarySuperConstructorInvocation(classDeclaration: KotlinAstNode): Option[KotlinAstNode] =
    classDeclaration.children
      .find(_.kind == "delegation_specifier")
      .flatMap(delegation => delegation.children.find(_.kind == "constructor_invocation"))

  private def primaryConstructor(classDeclaration: KotlinAstNode): Option[KotlinAstNode] =
    classDeclaration.children.find(_.kind == "primary_constructor")

  private def classParameterInfos(primaryConstructor: KotlinAstNode): List[ParameterInfo] = {
    primaryConstructor.children.filter(_.kind == "class_parameter").map { parameter =>
      parameterInfo(parameter, declaresMember = hasValOrVar(parameter))
    }
  }

  private def functionParameters(functionDeclaration: KotlinAstNode): List[ParameterInfo] = {
    functionDeclaration.children
      .find(_.kind == "function_value_parameters")
      .toList
      .flatMap(parameterInfosFromValueParameters)
  }

  private def functionParameterTypeFullNames(
    functionDeclaration: KotlinAstNode,
    bounds: Map[String, String]
  ): List[String] =
    functionDeclaration.children
      .find(_.kind == "function_value_parameters")
      .toList
      .flatMap(_.children.filter(_.kind == "parameter"))
      .map(parameter =>
        typeFromDirectChildren(parameter, bounds)
          .orElse(typeFromDeclarationText(parameter.code, bounds))
          .getOrElse(TypeConstants.Any)
      )

  private def parameterInfosFromValueParameters(parameters: KotlinAstNode): List[ParameterInfo] = {
    val infos              = mutable.ListBuffer.empty[ParameterInfo]
    var pendingAnnotations = List.empty[KotlinAstNode]
    parameters.children.foreach {
      case modifiers if modifiers.kind == "parameter_modifiers" =>
        pendingAnnotations = annotationNodesFor(modifiers)
      case parameter if parameter.kind == "parameter" =>
        infos.append(parameterInfo(parameter, declaresMember = false, pendingAnnotations))
        pendingAnnotations = Nil
      case _ =>
    }
    infos.toList
  }

  private def parameterInfo(
    parameter: KotlinAstNode,
    declaresMember: Boolean,
    externalAnnotations: List[KotlinAstNode] = Nil
  ): ParameterInfo = {
    val name = firstChildCode(parameter, "simple_identifier").getOrElse(nameFromDeclarationCode(parameter.code))
    val rawTypeText = parameter.children
      .find(child => TypeNodeKinds.contains(child.kind))
      .map(_.code)
      .orElse(rawTypeTextFromDeclarationText(parameter.code))
    val typeName = typeFromDirectChildren(parameter)
      .orElse(typeFromDeclarationText(parameter.code))
      .getOrElse(TypeConstants.Any)
    ParameterInfo(
      parameter,
      name,
      registerType(typeName),
      parameter.code,
      declaresMember,
      externalAnnotations ++ annotationNodesFor(parameter),
      collectionElementTypeFullName = collectionElementTypeFromDirectChildren(parameter).orElse(
        collectionElementTypeFromDeclarationText(parameter.code)
      ),
      mapKeyTypeFullName = rawTypeText.flatMap(typeText => mapKeyTypeFromTypeText(typeText)),
      mapValueTypeFullName = rawTypeText.flatMap(typeText => mapValueTypeFromTypeText(typeText)),
      mapEntryKeyTypeFullName = rawTypeText.flatMap(typeText => mapEntryKeyTypeFromTypeText(typeText)),
      mapEntryValueTypeFullName = rawTypeText.flatMap(typeText => mapEntryValueTypeFromTypeText(typeText)),
      pairFirstTypeFullName = rawTypeText.flatMap(typeText => pairTypesFromTypeText(typeText).map(_._1)),
      pairSecondTypeFullName = rawTypeText.flatMap(typeText => pairTypesFromTypeText(typeText).map(_._2)),
      tripleFirstTypeFullName = rawTypeText.flatMap(typeText => tripleTypesFromTypeText(typeText).map(_._1)),
      tripleSecondTypeFullName = rawTypeText.flatMap(typeText => tripleTypesFromTypeText(typeText).map(_._2)),
      tripleThirdTypeFullName = rawTypeText.flatMap(typeText => tripleTypesFromTypeText(typeText).map(_._3))
    )
  }

  private def withTypeParameterBoundsFor[T](node: KotlinAstNode)(block: => T): T = {
    val previousBounds = typeParameterBounds.toMap
    typeParameterBounds.clear()
    typeParameterBounds.addAll(previousBounds ++ typeParameterBoundsFor(node, previousBounds))
    try block
    finally {
      typeParameterBounds.clear()
      typeParameterBounds.addAll(previousBounds)
    }
  }

  private def typeParameterBoundsFor(node: KotlinAstNode, inheritedBounds: Map[String, String]): Map[String, String] =
    node.children
      .find(_.kind == "type_parameters")
      .toList
      .flatMap(_.children.filter(_.kind == "type_parameter"))
      .flatMap { typeParameter =>
        firstChildCode(typeParameter, "type_identifier").map { name =>
          val bound = typeParameter.children
            .dropWhile(_.kind != ":")
            .drop(1)
            .find(child => TypeNodeKinds.contains(child.kind))
            .map(boundNode => mapTypeName(boundNode.code, inheritedBounds))
            .getOrElse(TypeConstants.JavaLangObject)
          name -> registerType(bound)
        }
      }
      .toMap

  private def classBodyChildren(classDeclaration: KotlinAstNode): List[KotlinAstNode] =
    classDeclaration.children.find(_.kind == "class_body").map(_.children.filter(_.named)).getOrElse(Nil)

  private def enumEntries(classDeclaration: KotlinAstNode): List[KotlinAstNode] =
    classDeclaration.children
      .find(_.kind == "enum_class_body")
      .map(_.children.filter(_.kind == "enum_entry"))
      .getOrElse(Nil)

  private def typeDeclarationName(declaration: KotlinAstNode): String =
    if (declaration.kind == "companion_object") {
      firstChildCode(declaration, "type_identifier").getOrElse("Companion")
    } else {
      firstChildCode(declaration, "type_identifier")
        .orElse(firstDescendantCode(declaration, "type_identifier"))
        .getOrElse("<anonymous>")
    }

  private def fullNameForTypeDeclaration(
    declaration: KotlinAstNode,
    name: String,
    packageName: Option[String],
    ownerTypeFullName: Option[String]
  ): String = {
    declaration.kind match {
      case "companion_object" =>
        ownerTypeFullName
          .map(owner => s"$owner$$$name")
          .getOrElse(packageName.map(pkg => s"$pkg.$name").getOrElse(name))
      case _ =>
        ownerTypeFullName
          .map { owner =>
            if (owner.contains("$")) s"$owner$$$name" else s"$owner.$name"
          }
          .getOrElse(packageName.map(pkg => s"$pkg.$name").getOrElse(name))
    }
  }

  private def fullNameForTypeAlias(name: String, packageName: Option[String]): String =
    packageName.map(pkg => s"$pkg.$name").getOrElse(name)

  private def codeForTypeDeclaration(declaration: KotlinAstNode, name: String): String =
    declaration.kind match {
      case "object_declaration" | "companion_object" => name
      case _                                         => declarationHeader(declaration)
    }

  private def packageNameFor(root: KotlinAstNode): Option[String] =
    root.children
      .find(_.kind == "package_header")
      .flatMap(_.descendants.find(_.kind == "identifier").map(_.code))
      .filter(_.nonEmpty)

  private def returnTypeForFunction(functionDeclaration: KotlinAstNode): String = {
    val explicitType = explicitReturnTypeNode(functionDeclaration).map(typeName)
    explicitType.orElse(inferExpressionBodyType(functionDeclaration)).getOrElse(TypeConstants.Void)
  }

  private def inferExpressionBodyType(functionDeclaration: KotlinAstNode): Option[String] = {
    functionDeclaration.children
      .find(_.kind == "function_body")
      .flatMap(expressionBodyNode)
      .flatMap(typeForExpression(_, BodyContext(mutable.Map.empty, mutable.Map.empty, "")))
  }

  private def expressionBodyNode(functionBody: KotlinAstNode): Option[KotlinAstNode] =
    functionBody.children.dropWhile(_.kind != "=").drop(1).find(_.named)

  private def explicitReturnTypeNode(functionDeclaration: KotlinAstNode): Option[KotlinAstNode] =
    functionDeclaration.children
      .dropWhile(_.kind != "function_value_parameters")
      .drop(1)
      .find(child => TypeNodeKinds.contains(child.kind))

  private def typeFromDirectChildren(node: KotlinAstNode): Option[String] =
    typeFromDirectChildren(node, typeParameterBounds.toMap)

  private def typeFromDirectChildren(node: KotlinAstNode, bounds: Map[String, String]): Option[String] =
    node.children.find(child => TypeNodeKinds.contains(child.kind)).map(node => mapTypeName(node.code, bounds))

  private def typeFromDeclarationText(text: String): Option[String] =
    typeFromDeclarationText(text, typeParameterBounds.toMap)

  private def typeFromDeclarationText(text: String, bounds: Map[String, String]): Option[String] =
    rawTypeTextFromDeclarationText(text)
      .map(typeName => mapTypeName(typeName, bounds))

  private def rawTypeTextFromDeclarationText(text: String): Option[String] =
    text
      .split(":", 2)
      .lift(1)
      .map(_.takeWhile(ch => ch != '=' && ch != ',').trim)
      .filter(_.nonEmpty)

  private def updateCollectionElementType(
    name: String,
    declaredElementType: Option[String],
    initializer: Option[KotlinAstNode],
    context: BodyContext,
    declaredPairTypes: Option[(String, String)] = None,
    declaredTripleTypes: Option[(String, String, String)] = None
  ): Unit = {
    declaredElementType.orElse(initializer.flatMap(collectionElementTypeForExpression(_, context))) match {
      case Some(elementType) => context.collectionElementTypes.update(name, registerType(elementType))
      case None              => context.collectionElementTypes.remove(name)
    }
    initializer.flatMap(mapKeyTypeForExpression(_, context)) match {
      case Some(keyType) => context.mapKeyTypes.update(name, registerType(keyType))
      case None          => context.mapKeyTypes.remove(name)
    }
    initializer.flatMap(mapValueTypeForExpression(_, context)) match {
      case Some(valueType) => context.mapValueTypes.update(name, registerType(valueType))
      case None            => context.mapValueTypes.remove(name)
    }
    initializer.flatMap(mapEntryKeyTypeForExpression(_, context)) match {
      case Some(keyType) => context.mapEntryKeyTypes.update(name, registerType(keyType))
      case None          => context.mapEntryKeyTypes.remove(name)
    }
    initializer.flatMap(mapEntryValueTypeForExpression(_, context)) match {
      case Some(valueType) => context.mapEntryValueTypes.update(name, registerType(valueType))
      case None            => context.mapEntryValueTypes.remove(name)
    }
    declaredPairTypes.orElse(initializer.flatMap(pairTypesForExpression(_, context))) match {
      case Some((firstType, secondType)) =>
        context.pairFirstTypes.update(name, registerType(firstType))
        context.pairSecondTypes.update(name, registerType(secondType))
      case None =>
        context.pairFirstTypes.remove(name)
        context.pairSecondTypes.remove(name)
    }
    declaredTripleTypes.orElse(initializer.flatMap(tripleTypesForExpression(_, context))) match {
      case Some((firstType, secondType, thirdType)) =>
        context.tripleFirstTypes.update(name, registerType(firstType))
        context.tripleSecondTypes.update(name, registerType(secondType))
        context.tripleThirdTypes.update(name, registerType(thirdType))
      case None =>
        context.tripleFirstTypes.remove(name)
        context.tripleSecondTypes.remove(name)
        context.tripleThirdTypes.remove(name)
    }
  }

  private def collectionElementTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.collectionElementTypeFullName.map(param.name -> _)))

  private def mapKeyTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.mapKeyTypeFullName.map(param.name -> _)))

  private def mapValueTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.mapValueTypeFullName.map(param.name -> _)))

  private def mapEntryKeyTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap { param =>
      param.mapEntryKeyTypeFullName
        .orElse(Option.when(MapTypeFullNames.contains(param.typeFullName))(param.mapKeyTypeFullName).flatten)
        .map(param.name -> _)
    })

  private def mapEntryValueTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap { param =>
      param.mapEntryValueTypeFullName
        .orElse(Option.when(MapTypeFullNames.contains(param.typeFullName))(param.mapValueTypeFullName).flatten)
        .map(param.name -> _)
    })

  private def pairFirstTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.pairFirstTypeFullName.map(param.name -> _)))

  private def pairSecondTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.pairSecondTypeFullName.map(param.name -> _)))

  private def tripleFirstTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.tripleFirstTypeFullName.map(param.name -> _)))

  private def tripleSecondTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.tripleSecondTypeFullName.map(param.name -> _)))

  private def tripleThirdTypesForParams(params: List[ParameterInfo]): mutable.Map[String, String] =
    mutable.Map.from(params.flatMap(param => param.tripleThirdTypeFullName.map(param.name -> _)))

  private def updateParameterTypeMetadata(param: ParameterInfo, context: BodyContext): Unit = {
    param.collectionElementTypeFullName.foreach { elementType =>
      context.collectionElementTypes.update(param.name, elementType)
    }
    param.mapKeyTypeFullName.foreach { keyType =>
      context.mapKeyTypes.update(param.name, keyType)
    }
    param.mapValueTypeFullName.foreach { valueType =>
      context.mapValueTypes.update(param.name, valueType)
    }
    param.mapEntryKeyTypeFullName.foreach { keyType =>
      context.mapEntryKeyTypes.update(param.name, keyType)
    }
    param.mapEntryValueTypeFullName.foreach { valueType =>
      context.mapEntryValueTypes.update(param.name, valueType)
    }
    param.pairFirstTypeFullName.foreach { firstType =>
      context.pairFirstTypes.update(param.name, firstType)
    }
    param.pairSecondTypeFullName.foreach { secondType =>
      context.pairSecondTypes.update(param.name, secondType)
    }
    param.tripleFirstTypeFullName.foreach { firstType =>
      context.tripleFirstTypes.update(param.name, firstType)
    }
    param.tripleSecondTypeFullName.foreach { secondType =>
      context.tripleSecondTypes.update(param.name, secondType)
    }
    param.tripleThirdTypeFullName.foreach { thirdType =>
      context.tripleThirdTypes.update(param.name, thirdType)
    }
  }

  private def collectionElementTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.collectionElementTypes
          .get(expression.code)
          .orElse(context.types.get(expression.code).flatMap(rangeElementTypeFullName))
          .orElse(context.types.get(expression.code).flatMap(indexElementTypeFullName))
      case "navigation_expression" =>
        mapViewElementTypeForNavigation(expression, context)
      case "range_expression" =>
        rangeExpressionTypeFullName(expression, context).flatMap(rangeElementTypeFullName)
      case "infix_expression" =>
        typeForInfixExpression(expression, context).flatMap(rangeElementTypeFullName)
      case "call_expression" if BuiltinArrayFactoryNames(callNameFor(expression)) =>
        Some(arrayFactoryElementType(callArgumentNodes(expression), context, Some(expression)))
      case "call_expression" if BuiltinMapValueTypeArgumentNames(callNameFor(expression)) =>
        mapValueTypeFromCallTypeArguments(expression)
      case "call_expression" if BuiltinIterableFactoryNames(callNameFor(expression)) =>
        typeArgumentTypeFullNames(expression).headOption.orElse {
          homogeneousCallArgumentType(callArgumentNodes(expression), context)
        }
      case "call_expression" if ReceiverElementPreservingCallNames.contains(callNameFor(expression)) =>
        navigationReceiverForCallExpression(expression).flatMap(iterableElementTypeForExpression(_, context)).orElse {
          typeArgumentTypeFullNames(expression).headOption
        }
      case "call_expression" =>
        typeArgumentTypeFullNames(expression).headOption.orElse {
          typeForCallExpression(expression, context).flatMap(rangeElementTypeFullName)
        }
      case _ =>
        None
    }

  private def iterableElementTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    receiverExpressionTypeFullName(expression, context)
      .filter(MapTypeFullNames.contains)
      .map(_ => MapEntryTypeFullName)
      .orElse(collectionElementTypeForExpression(expression, context))

  private def mapValueTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.mapValueTypes.get(expression.code).orElse(context.collectionElementTypes.get(expression.code))
      case "navigation_expression" =>
        navigationFieldName(expression) match {
          case "values" => mapViewValueTypeForNavigation(expression, context)
          case _        => None
        }
      case "call_expression" if BuiltinMapValueTypeArgumentNames(callNameFor(expression)) =>
        mapValueTypeFromCallTypeArguments(expression)
      case "call_expression" if ExtensionElementReturnNames.contains(callNameFor(expression)) =>
        navigationReceiverForCallExpression(expression).flatMap(mapValueTypeForExpression(_, context))
      case _ =>
        collectionElementTypeForExpression(expression, context)
    }

  private def mapKeyTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.mapKeyTypes.get(expression.code)
      case "navigation_expression" =>
        navigationFieldName(expression) match {
          case "keys" => mapViewKeyTypeForNavigation(expression, context)
          case _      => None
        }
      case "call_expression" if BuiltinMapValueTypeArgumentNames(callNameFor(expression)) =>
        typeArgumentTypeFullNames(expression).headOption
      case "call_expression" if ExtensionElementReturnNames.contains(callNameFor(expression)) =>
        navigationReceiverForCallExpression(expression).flatMap(mapKeyTypeForExpression(_, context))
      case _ =>
        None
    }

  private def mapEntryKeyTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.mapEntryKeyTypes.get(expression.code)
      case "navigation_expression" =>
        mapEntryKeyTypeForNavigation(expression, context)
      case "call_expression" if ExtensionElementReturnNames.contains(callNameFor(expression)) =>
        navigationReceiverForCallExpression(expression).flatMap(mapEntryKeyTypeForExpression(_, context))
      case _ =>
        None
    }

  private def mapEntryValueTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.mapEntryValueTypes.get(expression.code)
      case "navigation_expression" =>
        mapEntryValueTypeForNavigation(expression, context)
      case "call_expression" if ExtensionElementReturnNames.contains(callNameFor(expression)) =>
        navigationReceiverForCallExpression(expression).flatMap(mapEntryValueTypeForExpression(_, context))
      case _ =>
        None
    }

  private def pairTypesForExpression(expression: KotlinAstNode, context: BodyContext): Option[(String, String)] =
    pairFirstTypeForExpression(expression, context).zip(pairSecondTypeForExpression(expression, context))

  private def pairFirstTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.pairFirstTypes.get(expression.code)
      case "infix_expression" =>
        pairInfixOperandNodes(expression).filter(_._2.code == "to").flatMap { case (lhsNode, _, _) =>
          typeForExpression(lhsNode, context)
        }
      case "call_expression" if callNameFor(expression) == "Pair" =>
        callArgumentNodes(expression).headOption.flatMap(typeForCallArgument(_, context))
      case _ =>
        None
    }

  private def pairSecondTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.pairSecondTypes.get(expression.code)
      case "infix_expression" =>
        pairInfixOperandNodes(expression).filter(_._2.code == "to").flatMap { case (_, _, rhsNode) =>
          typeForExpression(rhsNode, context)
        }
      case "call_expression" if callNameFor(expression) == "Pair" =>
        callArgumentNodes(expression).lift(1).flatMap(typeForCallArgument(_, context))
      case _ =>
        None
    }

  private def tripleTypesForExpression(
    expression: KotlinAstNode,
    context: BodyContext
  ): Option[(String, String, String)] =
    for {
      firstType  <- tripleFirstTypeForExpression(expression, context)
      secondType <- tripleSecondTypeForExpression(expression, context)
      thirdType  <- tripleThirdTypeForExpression(expression, context)
    } yield (firstType, secondType, thirdType)

  private def tripleFirstTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.tripleFirstTypes.get(expression.code)
      case "call_expression" if callNameFor(expression) == "Triple" =>
        callArgumentNodes(expression).headOption.flatMap(typeForCallArgument(_, context))
      case _ =>
        None
    }

  private def tripleSecondTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.tripleSecondTypes.get(expression.code)
      case "call_expression" if callNameFor(expression) == "Triple" =>
        callArgumentNodes(expression).lift(1).flatMap(typeForCallArgument(_, context))
      case _ =>
        None
    }

  private def tripleThirdTypeForExpression(expression: KotlinAstNode, context: BodyContext): Option[String] =
    expression.kind match {
      case "simple_identifier" =>
        context.tripleThirdTypes.get(expression.code)
      case "call_expression" if callNameFor(expression) == "Triple" =>
        callArgumentNodes(expression).lift(2).flatMap(typeForCallArgument(_, context))
      case _ =>
        None
    }

  private def pairInfixOperandNodes(expression: KotlinAstNode): Option[(KotlinAstNode, KotlinAstNode, KotlinAstNode)] =
    expression.children.filter(_.named) match {
      case lhsNode :: operatorNode :: rhsNode :: Nil => Some((lhsNode, operatorNode, rhsNode))
      case _                                         => None
    }

  private def mapValueTypeFromCallTypeArguments(callExpression: KotlinAstNode): Option[String] =
    typeArgumentTypeFullNames(callExpression).lift(1).orElse(typeArgumentTypeFullNames(callExpression).headOption)

  private def mapViewElementTypeForNavigation(
    navigationExpression: KotlinAstNode,
    context: BodyContext
  ): Option[String] =
    navigationFieldName(navigationExpression) match {
      case "entries" =>
        navigationReceiverNode(navigationExpression)
          .flatMap(receiverExpressionTypeFullName(_, context))
          .filter(MapTypeFullNames.contains)
          .map(_ => MapEntryTypeFullName)
      case "keys"   => mapViewKeyTypeForNavigation(navigationExpression, context)
      case "values" => mapViewValueTypeForNavigation(navigationExpression, context)
      case _        => None
    }

  private def mapViewKeyTypeForNavigation(navigationExpression: KotlinAstNode, context: BodyContext): Option[String] =
    navigationReceiverNode(navigationExpression).flatMap(mapKeyTypeForExpression(_, context))

  private def mapViewValueTypeForNavigation(navigationExpression: KotlinAstNode, context: BodyContext): Option[String] =
    navigationReceiverNode(navigationExpression).flatMap(mapValueTypeForExpression(_, context))

  private def mapEntryKeyTypeForNavigation(navigationExpression: KotlinAstNode, context: BodyContext): Option[String] =
    navigationFieldName(navigationExpression) match {
      case "entries" => mapViewKeyTypeForNavigation(navigationExpression, context)
      case _         => None
    }

  private def mapEntryValueTypeForNavigation(
    navigationExpression: KotlinAstNode,
    context: BodyContext
  ): Option[String] =
    navigationFieldName(navigationExpression) match {
      case "entries" => mapViewValueTypeForNavigation(navigationExpression, context)
      case _         => None
    }

  private def collectionElementTypeFromDirectChildren(node: KotlinAstNode): Option[String] =
    collectionElementTypeFromDirectChildren(node, typeParameterBounds.toMap)

  private def collectionElementTypeFromDirectChildren(
    node: KotlinAstNode,
    bounds: Map[String, String]
  ): Option[String] =
    node.children
      .find(child => TypeNodeKinds.contains(child.kind))
      .flatMap(typeNode => collectionElementTypeFromTypeText(typeNode.code, bounds))

  private def collectionElementTypeFromDeclarationText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[String] =
    text
      .split(":", 2)
      .lift(1)
      .map(_.takeWhile(ch => ch != '=' && ch != ',').trim)
      .filter(_.nonEmpty)
      .flatMap(typeName => collectionElementTypeFromTypeText(typeName, bounds))

  private def collectionElementTypeFromTypeText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[String] = {
    val base       = baseTypeName(text)
    val simpleBase = base.split('.').lastOption.getOrElse(base)
    val mappedBase = mapTypeName(base, bounds)
    Option
      .when(CollectionTypeNames(base) || CollectionTypeNames(simpleBase) || IterableTypeFullNames(mappedBase)) {
        val argument =
          if (MapTypeNames(base) || MapTypeNames(simpleBase) || MapTypeFullNames(mappedBase)) {
            genericArguments(text).lift(1).orElse(genericArguments(text).headOption)
          } else {
            genericArguments(text).headOption
          }
        argument.map(argument => registerType(mapTypeName(argument, bounds)))
      }
      .flatten
  }

  private def pairTypesFromDirectChildren(node: KotlinAstNode): Option[(String, String)] =
    pairTypesFromDirectChildren(node, typeParameterBounds.toMap)

  private def pairTypesFromDirectChildren(node: KotlinAstNode, bounds: Map[String, String]): Option[(String, String)] =
    node.children
      .find(child => TypeNodeKinds.contains(child.kind))
      .flatMap(typeNode => pairTypesFromTypeText(typeNode.code, bounds))

  private def pairTypesFromDeclarationText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[(String, String)] =
    rawTypeTextFromDeclarationText(text).flatMap(typeName => pairTypesFromTypeText(typeName, bounds))

  private def pairTypesFromTypeText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[(String, String)] =
    Option
      .when(isPairTypeText(text, bounds)) {
        val arguments = genericArguments(text)
        arguments.headOption.zip(arguments.lift(1)).map { case (firstType, secondType) =>
          registerType(mapTypeName(firstType, bounds)) -> registerType(mapTypeName(secondType, bounds))
        }
      }
      .flatten

  private def tripleTypesFromDirectChildren(node: KotlinAstNode): Option[(String, String, String)] =
    tripleTypesFromDirectChildren(node, typeParameterBounds.toMap)

  private def tripleTypesFromDirectChildren(
    node: KotlinAstNode,
    bounds: Map[String, String]
  ): Option[(String, String, String)] =
    node.children
      .find(child => TypeNodeKinds.contains(child.kind))
      .flatMap(typeNode => tripleTypesFromTypeText(typeNode.code, bounds))

  private def tripleTypesFromDeclarationText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[(String, String, String)] =
    rawTypeTextFromDeclarationText(text).flatMap(typeName => tripleTypesFromTypeText(typeName, bounds))

  private def tripleTypesFromTypeText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[(String, String, String)] =
    Option
      .when(isTripleTypeText(text, bounds)) {
        val arguments = genericArguments(text)
        for {
          firstType  <- arguments.headOption
          secondType <- arguments.lift(1)
          thirdType  <- arguments.lift(2)
        } yield (
          registerType(mapTypeName(firstType, bounds)),
          registerType(mapTypeName(secondType, bounds)),
          registerType(mapTypeName(thirdType, bounds))
        )
      }
      .flatten

  private def mapKeyTypeFromTypeText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[String] =
    Option
      .when(isMapTypeText(text, bounds)) {
        genericArguments(text).headOption.map(argument => registerType(mapTypeName(argument, bounds)))
      }
      .flatten

  private def mapValueTypeFromTypeText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[String] =
    Option
      .when(isMapTypeText(text, bounds)) {
        genericArguments(text)
          .lift(1)
          .orElse(genericArguments(text).headOption)
          .map(argument => registerType(mapTypeName(argument, bounds)))
      }
      .flatten

  private def mapEntryKeyTypeFromTypeText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[String] =
    Option
      .when(isMapEntryTypeText(text, bounds)) {
        genericArguments(text).headOption.map(argument => registerType(mapTypeName(argument, bounds)))
      }
      .flatten

  private def mapEntryValueTypeFromTypeText(
    text: String,
    bounds: Map[String, String] = typeParameterBounds.toMap
  ): Option[String] =
    Option
      .when(isMapEntryTypeText(text, bounds)) {
        genericArguments(text)
          .lift(1)
          .orElse(genericArguments(text).headOption)
          .map(argument => registerType(mapTypeName(argument, bounds)))
      }
      .flatten

  private def isMapTypeText(text: String, bounds: Map[String, String]): Boolean = {
    val base       = baseTypeName(text)
    val simpleBase = base.split('.').lastOption.getOrElse(base)
    val mappedBase = mapTypeName(base, bounds)
    MapTypeNames(base) || MapTypeNames(simpleBase) || MapTypeFullNames(mappedBase)
  }

  private def isMapEntryTypeText(text: String, bounds: Map[String, String]): Boolean = {
    val base       = baseTypeName(text)
    val simpleBase = base.split('.').lastOption.getOrElse(base)
    val mappedBase = mapTypeName(base, bounds)
    MapEntryTypeNames(base) || MapEntryTypeNames(simpleBase) || mappedBase == MapEntryTypeFullName
  }

  private def isPairTypeText(text: String, bounds: Map[String, String]): Boolean = {
    val base       = baseTypeName(text)
    val simpleBase = base.split('.').lastOption.getOrElse(base)
    val mappedBase = mapTypeName(base, bounds)
    PairTypeNames(base) || PairTypeNames(simpleBase) || mappedBase == PairTypeFullName
  }

  private def isTripleTypeText(text: String, bounds: Map[String, String]): Boolean = {
    val base       = baseTypeName(text)
    val simpleBase = base.split('.').lastOption.getOrElse(base)
    val mappedBase = mapTypeName(base, bounds)
    TripleTypeNames(base) || TripleTypeNames(simpleBase) || mappedBase == TripleTypeFullName
  }

  private def genericArguments(text: String): List[String] = {
    val start = text.indexOf('<')
    if (start < 0) {
      Nil
    } else {
      val arguments = mutable.ListBuffer.empty[String]
      val builder   = new StringBuilder
      var index     = start + 1
      var depth     = 0
      var done      = false
      while (index < text.length && !done) {
        text.charAt(index) match {
          case '<' =>
            depth += 1
            builder.append('<')
          case '>' if depth == 0 =>
            arguments.append(builder.result().trim)
            done = true
          case '>' =>
            depth -= 1
            builder.append('>')
          case ',' if depth == 0 =>
            arguments.append(builder.result().trim)
            builder.clear()
          case ch =>
            builder.append(ch)
        }
        index += 1
      }
      arguments.toList.filter(_.nonEmpty)
    }
  }

  private def typeName(node: KotlinAstNode): String = mapTypeName(node.code)

  private def mapTypeName(rawTypeName: String, bounds: Map[String, String] = typeParameterBounds.toMap): String = {
    val stripped   = rawTypeName.trim.stripSuffix("?")
    val base       = baseTypeName(stripped)
    val simpleBase = base.split('.').lastOption.getOrElse(base)
    arrayTypeFullName(stripped, bounds)
      .orElse(PrimitiveArrayTypeNames.get(base))
      .orElse(PrimitiveArrayTypeNames.get(simpleBase))
      .orElse(bounds.get(base))
      .orElse(bounds.get(simpleBase))
      .orElse(typeAliases.get(base))
      .orElse(typeAliases.get(simpleBase))
      .orElse(BuiltinTypeNames.get(base))
      .orElse(BuiltinTypeNames.get(simpleBase))
      .orElse(importAliases.get(base))
      .orElse(importAliases.get(simpleBase))
      .orElse(DefaultTypeFullNames.get(base))
      .orElse(DefaultTypeFullNames.get(simpleBase))
      .getOrElse(base)
  }

  private def arrayTypeFullName(rawTypeName: String, bounds: Map[String, String]): Option[String] = {
    val stripped = rawTypeName.trim.stripSuffix("?")
    val inner = stripped match {
      case array if array.startsWith("Array<") && array.endsWith(">") =>
        Some(array.stripPrefix("Array<").stripSuffix(">"))
      case array if array.startsWith("kotlin.Array<") && array.endsWith(">") =>
        Some(array.stripPrefix("kotlin.Array<").stripSuffix(">"))
      case _ => None
    }
    inner.map(typeName => s"${mapTypeName(typeName, bounds)}[]")
  }

  private def baseTypeName(rawTypeName: String): String =
    rawTypeName.trim.stripSuffix("?").takeWhile(_ != '<').trim

  private def nameFromDeclarationCode(code: String): String = {
    val withoutBinding = code.trim.stripPrefix("val ").stripPrefix("var ")
    withoutBinding.takeWhile(ch => ch != ':' && ch != '=' && ch != ',').trim
  }

  private def firstChildCode(node: KotlinAstNode, kind: String): Option[String] =
    node.children.find(_.kind == kind).map(_.code).filter(_.nonEmpty)

  private def firstDescendantCode(node: KotlinAstNode, kind: String): Option[String] =
    node.descendants.find(_.kind == kind).map(_.code).filter(_.nonEmpty)

  private def methodModifierNodes(
    node: KotlinAstNode,
    withVirtualModifier: Boolean,
    isAbstract: Boolean
  ): Seq[NewModifier] = {
    val visibility =
      explicitKotlinModifierTypes(node).find(VisibilityModifierTypes.contains).getOrElse(ModifierTypes.PUBLIC)
    val modifierTypes =
      Seq(visibility) ++ Option.when(withVirtualModifier)(ModifierTypes.VIRTUAL) ++ Option.when(isAbstract)(
        ModifierTypes.ABSTRACT
      )
    modifierTypes.distinct.map(modifierType => modifierNode(node, modifierType))
  }

  private def typeDeclarationModifierNodes(node: KotlinAstNode): Seq[NewModifier] =
    Option.when(isAbstractTypeDeclaration(node))(modifierNode(node, ModifierTypes.ABSTRACT)).toSeq

  private def isAbstractTypeDeclaration(node: KotlinAstNode): Boolean =
    hasKotlinModifier(node, "abstract") || isInterfaceDeclaration(node)

  private def isInterfaceDeclaration(node: KotlinAstNode): Boolean =
    node.children.exists(_.kind == "interface")

  private def isDataClassDeclaration(node: KotlinAstNode): Boolean =
    node.kind == "class_declaration" &&
      (directKotlinModifierTokens(node).contains("data") || declarationHeader(node).split("\\s+").contains("data"))

  private def explicitKotlinModifierTypes(node: KotlinAstNode): List[String] =
    directKotlinModifierTokens(node).flatMap(KotlinModifierTypeByKeyword.get)

  private def hasKotlinModifier(node: KotlinAstNode, keyword: String): Boolean =
    directKotlinModifierTokens(node).contains(keyword)

  private def isPrivateMethod(node: KotlinAstNode): Boolean =
    explicitKotlinModifierTypes(node).contains(ModifierTypes.PRIVATE)

  private def directKotlinModifierTokens(node: KotlinAstNode): List[String] =
    node.children
      .filter(_.kind == "modifiers")
      .flatMap(modifiers => modifiers :: modifiers.descendants)
      .map(_.code.trim)
      .filter(_.nonEmpty)
      .distinct

  private def hasReturnKeyword(node: KotlinAstNode): Boolean =
    node.children.exists(child => child.kind == "return" || child.kind == "return@")

  private def hasThrowKeyword(node: KotlinAstNode): Boolean =
    node.children.exists(_.kind == "throw")

  private def hasBreakKeyword(node: KotlinAstNode): Boolean =
    node.children.exists(child => child.kind == "break" || child.kind == "break@")

  private def hasContinueKeyword(node: KotlinAstNode): Boolean =
    node.children.exists(child => child.kind == "continue" || child.kind == "continue@")

  private def jumpLabelAst(node: KotlinAstNode): Option[Ast] =
    node.children.find(_.kind == "label").map { label =>
      Ast(
        NewJumpLabel()
          .parserTypeName(node.kind)
          .name(label.code)
          .code(label.code)
          .lineNumber(line(label))
          .columnNumber(column(label))
          .order(1)
      )
    }

  private def hasValOrVar(node: KotlinAstNode): Boolean =
    node.children.exists(child => child.kind == "binding_pattern_kind" && (child.code == "val" || child.code == "var"))

  private def declarationHeader(node: KotlinAstNode): String =
    node.code.takeWhile(_ != '{').trim

  private def methodSignature(returnType: String, parameterTypes: Seq[String]): String =
    s"$returnType(${parameterTypes.mkString(",")})"

  private def methodFullName(descFullName: String, signature: String): String =
    s"$descFullName:$signature"

  private def methodBaseFullName(fullName: String): String =
    fullName.takeWhile(_ != ':')

  private def methodInfoByOwnerNameAndArity(owner: String, name: String, arity: Int): Option[MethodInfo] =
    methodInfoByOwnerNameAndArity(owner, name, arity, Set.empty)

  private def methodInfoByOwnerNameAndArguments(
    owner: String,
    name: String,
    argumentTypeFullNames: Seq[String]
  ): Option[MethodInfo] =
    methodInfoByOwnerNameAndArguments(owner, name, argumentTypeFullNames, Set.empty)

  private def methodInfoByOwnerNameAndArity(
    owner: String,
    name: String,
    arity: Int,
    visited: Set[String]
  ): Option[MethodInfo] =
    if (visited.contains(owner)) {
      None
    } else {
      methodInfoForExactOwnerNameAndArity(owner, name, arity).orElse(
        inheritedTypesByFullName
          .get(owner)
          .flatMap(
            _.iterator
              .flatMap(inherited => methodInfoByOwnerNameAndArity(inherited, name, arity, visited + owner))
              .toSeq
              .headOption
          )
      )
    }

  private def methodInfoByOwnerNameAndArguments(
    owner: String,
    name: String,
    argumentTypeFullNames: Seq[String],
    visited: Set[String]
  ): Option[MethodInfo] =
    if (visited.contains(owner)) {
      None
    } else {
      methodInfoForExactOwnerNameAndArguments(owner, name, argumentTypeFullNames).orElse(
        inheritedTypesByFullName
          .get(owner)
          .flatMap(
            _.iterator
              .flatMap(inherited =>
                methodInfoByOwnerNameAndArguments(inherited, name, argumentTypeFullNames, visited + owner)
              )
              .toSeq
              .headOption
          )
      )
    }

  private def methodInfoForExactOwnerNameAndArity(owner: String, name: String, arity: Int): Option[MethodInfo] =
    declaredMethodInfoForOwnerNameAndArity(owner, name, arity)
      .orElse(builtinMemberMethodInfo(owner, name, arity))
      .orElse(builtinExtensionMethodInfo(owner, name, arity))

  private def methodInfoForExactOwnerNameAndArguments(
    owner: String,
    name: String,
    argumentTypeFullNames: Seq[String]
  ): Option[MethodInfo] =
    declaredMethodInfoForOwnerNameAndArity(owner, name, argumentTypeFullNames.size)
      .orElse(builtinMemberMethodInfo(owner, name, argumentTypeFullNames))
      .orElse(builtinExtensionMethodInfo(owner, name, argumentTypeFullNames))
      .orElse(builtinMemberMethodInfo(owner, name, argumentTypeFullNames.size))
      .orElse(builtinExtensionMethodInfo(owner, name, argumentTypeFullNames.size))

  private def declaredMethodInfoForOwnerNameAndArity(owner: String, name: String, arity: Int): Option[MethodInfo] =
    methodsByOwnerNameAndArity
      .get((owner, name, arity))
      .orElse(
        typeDeclarationInfos
          .get(owner)
          .flatMap(_.methods.find(method => method.name == name && method.parameterCount == arity))
          .map { method =>
            MethodInfo(
              methodFullName(s"$owner.$name", method.signature),
              method.signature,
              methodReturnType(method.signature),
              method.isPrivate
            )
          }
      )

  private def methodReturnType(signature: String): String =
    signature.takeWhile(_ != '(')

  private def builtinTopLevelMethodInfo(
    name: String,
    argumentNodes: List[KotlinAstNode],
    context: BodyContext,
    callExpression: Option[KotlinAstNode] = None
  ): Option[MethodInfo] =
    name match {
      case "print" | "println" if argumentNodes.sizeCompare(1) <= 0 =>
        val parameterTypes = argumentNodes.map(argument =>
          printlnParameterType(typeForCallArgument(argument, context).getOrElse(TypeConstants.JavaLangObject))
        )
        val signature = methodSignature(TypeConstants.Void, parameterTypes)
        Some(MethodInfo(methodFullName(s"kotlin.io.$name", signature), signature, TypeConstants.Void))
      case "emptyList" =>
        val signature = methodSignature("java.util.List", Nil)
        Some(MethodInfo(methodFullName("kotlin.collections.emptyList", signature), signature, "java.util.List"))
      case "listOf" | "listOfNotNull" =>
        val parameterTypes = collectionFactoryParameterTypes(argumentNodes.size, TypeConstants.JavaLangObject)
        val signature      = methodSignature("java.util.List", parameterTypes)
        Some(MethodInfo(methodFullName(s"kotlin.collections.$name", signature), signature, "java.util.List"))
      case "mutableListOf" =>
        val parameterTypes = varargFactoryParameterTypes(argumentNodes.size, TypeConstants.JavaLangObject)
        val signature      = methodSignature("java.util.List", parameterTypes)
        Some(MethodInfo(methodFullName("kotlin.collections.mutableListOf", signature), signature, "java.util.List"))
      case "arrayListOf" =>
        val parameterTypes = varargFactoryParameterTypes(argumentNodes.size, TypeConstants.JavaLangObject)
        val signature      = methodSignature("java.util.ArrayList", parameterTypes)
        Some(MethodInfo(methodFullName("kotlin.collections.arrayListOf", signature), signature, "java.util.ArrayList"))
      case "emptySet" =>
        val signature = methodSignature("java.util.Set", Nil)
        Some(MethodInfo(methodFullName("kotlin.collections.emptySet", signature), signature, "java.util.Set"))
      case "setOf" | "setOfNotNull" =>
        val parameterTypes = collectionFactoryParameterTypes(argumentNodes.size, TypeConstants.JavaLangObject)
        val signature      = methodSignature("java.util.Set", parameterTypes)
        Some(MethodInfo(methodFullName(s"kotlin.collections.$name", signature), signature, "java.util.Set"))
      case "mutableSetOf" =>
        val parameterTypes = varargFactoryParameterTypes(argumentNodes.size, TypeConstants.JavaLangObject)
        val signature      = methodSignature("java.util.Set", parameterTypes)
        Some(MethodInfo(methodFullName("kotlin.collections.mutableSetOf", signature), signature, "java.util.Set"))
      case "hashSetOf" =>
        val parameterTypes = varargFactoryParameterTypes(argumentNodes.size, TypeConstants.JavaLangObject)
        val signature      = methodSignature("java.util.HashSet", parameterTypes)
        Some(MethodInfo(methodFullName("kotlin.collections.hashSetOf", signature), signature, "java.util.HashSet"))
      case "linkedSetOf" =>
        val parameterTypes = varargFactoryParameterTypes(argumentNodes.size, TypeConstants.JavaLangObject)
        val signature      = methodSignature("java.util.LinkedHashSet", parameterTypes)
        Some(
          MethodInfo(methodFullName("kotlin.collections.linkedSetOf", signature), signature, "java.util.LinkedHashSet")
        )
      case "emptyMap" =>
        val signature = methodSignature("java.util.Map", Nil)
        Some(MethodInfo(methodFullName("kotlin.collections.emptyMap", signature), signature, "java.util.Map"))
      case "mapOf" =>
        val parameterTypes = collectionFactoryParameterTypes(argumentNodes.size, "kotlin.Pair")
        val signature      = methodSignature("java.util.Map", parameterTypes)
        Some(MethodInfo(methodFullName("kotlin.collections.mapOf", signature), signature, "java.util.Map"))
      case "mutableMapOf" =>
        val parameterTypes = varargFactoryParameterTypes(argumentNodes.size, "kotlin.Pair")
        val signature      = methodSignature("java.util.Map", parameterTypes)
        Some(MethodInfo(methodFullName("kotlin.collections.mutableMapOf", signature), signature, "java.util.Map"))
      case "emptyArray" =>
        val returnType = arrayFactoryElementType(argumentNodes, context, callExpression)
        val signature  = methodSignature(TypeConstants.JavaLangObject + "[]", Nil)
        Some(MethodInfo(methodFullName("kotlin.emptyArray", signature), signature, s"$returnType[]"))
      case "arrayOfNulls" =>
        val returnType = arrayFactoryElementType(argumentNodes, context, callExpression)
        val signature  = methodSignature(TypeConstants.JavaLangObject + "[]", Seq("int"))
        Some(MethodInfo(methodFullName("kotlin.arrayOfNulls", signature), signature, s"$returnType[]"))
      case "arrayOf" =>
        val elementType    = arrayFactoryElementType(argumentNodes, context, callExpression)
        val returnType     = s"$elementType[]"
        val parameterTypes = collectionFactoryParameterTypes(argumentNodes.size, elementType)
        val signature      = methodSignature(returnType, parameterTypes)
        Some(MethodInfo(methodFullName("kotlin.arrayOf", signature), signature, returnType))
      case primitiveArrayFactoryName if PrimitiveArrayFactoryReturnTypes.contains(primitiveArrayFactoryName) =>
        val returnType     = PrimitiveArrayFactoryReturnTypes(primitiveArrayFactoryName)
        val elementType    = returnType.stripSuffix("[]")
        val parameterTypes = varargFactoryParameterTypes(argumentNodes.size, elementType)
        val signature      = methodSignature(returnType, parameterTypes)
        Some(MethodInfo(methodFullName(s"kotlin.$primitiveArrayFactoryName", signature), signature, returnType))
      case _ if importAliases.get(name).contains("kotlin.math.max") && argumentNodes.sizeCompare(2) == 0 =>
        val argumentTypes = argumentNodes.flatMap(typeForCallArgument(_, context))
        argumentTypes match {
          case List(typeFullName, otherTypeFullName)
              if typeFullName == otherTypeFullName && MathMaxTypeFullNames.contains(typeFullName) =>
            val signature = methodSignature(typeFullName, Seq(typeFullName, typeFullName))
            Some(MethodInfo(methodFullName("kotlin.math.max", signature), signature, typeFullName))
          case _ =>
            None
        }
      case _ =>
        None
    }

  private def typeForCallArgument(argument: KotlinAstNode, context: BodyContext): Option[String] =
    argument.kind match {
      case "value_argument" => valueArgumentExpressionNode(argument).flatMap(typeForExpression(_, context))
      case _                => typeForExpression(argument, context)
    }

  private def arrayFactoryElementType(
    argumentNodes: List[KotlinAstNode],
    context: BodyContext,
    callExpression: Option[KotlinAstNode]
  ): String =
    callExpression
      .map(callNameFor)
      .flatMap(PrimitiveArrayFactoryReturnTypes.get)
      .map(_.stripSuffix("[]"))
      .orElse(
        callExpression
          .filter(callExpression =>
            callNameFor(callExpression) == "arrayOfNulls" || callNameFor(callExpression) == "emptyArray"
          )
          .flatMap(typeArgumentTypeFullNames(_).headOption)
      )
      .orElse(
        callExpression
          .flatMap(typeArgumentTypeFullNames(_).headOption)
      )
      .orElse(homogeneousCallArgumentType(argumentNodes, context))
      .getOrElse(TypeConstants.JavaLangObject)

  private def homogeneousCallArgumentType(argumentNodes: List[KotlinAstNode], context: BodyContext): Option[String] = {
    val argumentTypes = argumentNodes.flatMap(typeForCallArgument(_, context)).distinct
    argumentTypes match {
      case single :: Nil => Some(single)
      case _             => None
    }
  }

  private def printlnParameterType(typeFullName: String): String =
    if (PrintPrimitiveTypeFullNames.contains(typeFullName)) typeFullName else TypeConstants.JavaLangObject

  private def collectionFactoryParameterTypes(argumentCount: Int, elementTypeFullName: String): Seq[String] =
    argumentCount match {
      case 0 => Nil
      case 1 => Seq(elementTypeFullName)
      case _ => Seq(s"$elementTypeFullName[]")
    }

  private def varargFactoryParameterTypes(argumentCount: Int, elementTypeFullName: String): Seq[String] =
    if (argumentCount == 0) Nil else Seq(s"$elementTypeFullName[]")

  private def builtinMemberMethodInfo(owner: String, name: String, arity: Int): Option[MethodInfo] =
    builtinMemberMethodInfo(owner, name, Seq.fill(arity)(TypeConstants.Any))

  private def builtinMemberMethodInfo(
    owner: String,
    name: String,
    argumentTypeFullNames: Seq[String]
  ): Option[MethodInfo] = {
    val arity = argumentTypeFullNames.size
    (owner, name, arity) match {
      case (_, "toString", 0) if PrimitiveKotlinTypeFullNames.contains(owner) =>
        val ownerFullName = PrimitiveKotlinTypeFullNames(owner)
        val signature     = methodSignature("java.lang.String", Nil)
        Some(MethodInfo(methodFullName(s"$ownerFullName.toString", signature), signature, "java.lang.String"))
      case (_, conversionName, 0)
          if NumericPrimitiveTypeFullNames.contains(owner) && PrimitiveConversionReturnTypes.contains(conversionName) =>
        val ownerFullName = PrimitiveKotlinTypeFullNames(owner)
        val returnType    = PrimitiveConversionReturnTypes(conversionName)
        val signature     = methodSignature(returnType, Nil)
        Some(MethodInfo(methodFullName(s"$ownerFullName.$conversionName", signature), signature, returnType))
      case (arrayOwner, "get", 1) if isArrayTypeFullName(arrayOwner) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature  = methodSignature(returnType, Seq("int"))
        Some(
          MethodInfo(
            methodFullName(s"${arrayMemberOwnerTypeFullName(arrayOwner)}.get", signature),
            signature,
            returnType
          )
        )
      case (arrayOwner, "set", 2) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq("int", arrayElementTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"${arrayMemberOwnerTypeFullName(arrayOwner)}.set", signature),
            signature,
            TypeConstants.Void
          )
        )
      case (arrayOwner, "iterator", 0) if isArrayTypeFullName(arrayOwner) =>
        val returnType = arrayIteratorTypeFullName(arrayOwner)
        val signature  = methodSignature(returnType, Nil)
        Some(
          MethodInfo(
            methodFullName(s"${arrayMemberOwnerTypeFullName(arrayOwner)}.iterator", signature),
            signature,
            returnType
          )
        )
      case (arrayOwner, "contains", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature(
          "boolean",
          Seq(arrayReceiverTypeForMemberSignature(arrayOwner), arrayElementTypeForMemberSignature(arrayOwner))
        )
        Some(
          MethodInfo(methodFullName("kotlin.collections.contains", signature), signature, "boolean", isExtension = true)
        )
      case (arrayOwner, indexName, 1)
          if isArrayTypeFullName(arrayOwner) && ArrayIndexElementNames.contains(indexName) =>
        val signature = methodSignature(
          "int",
          Seq(arrayReceiverTypeForMemberSignature(arrayOwner), arrayElementTypeForMemberSignature(arrayOwner))
        )
        Some(
          MethodInfo(methodFullName(s"kotlin.collections.$indexName", signature), signature, "int", isExtension = true)
        )
      case (arrayOwner, emptyName, 0)
          if isArrayTypeFullName(arrayOwner) && ArrayEmptyPredicateNames.contains(emptyName) =>
        val signature = methodSignature("boolean", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$emptyName", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (listOwner, "get", 1) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("int"))
        Some(
          MethodInfo(methodFullName("kotlin.collections.List.get", signature), signature, TypeConstants.JavaLangObject)
        )
      case (listOwner, "contains", 1) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("boolean", Seq(TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName("kotlin.collections.List.contains", signature), signature, "boolean"))
      case (listOwner, "containsAll", 1) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("boolean", Seq("java.util.Collection"))
        Some(MethodInfo(methodFullName("kotlin.collections.List.containsAll", signature), signature, "boolean"))
      case (listOwner, "isEmpty", 0) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("boolean", Nil)
        Some(MethodInfo(methodFullName("kotlin.collections.List.isEmpty", signature), signature, "boolean"))
      case (listOwner, indexName, 1)
          if ListInterfaceTypeFullNames.contains(listOwner) && ListIndexElementNames.contains(indexName) =>
        val signature = methodSignature("int", Seq(TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName(s"kotlin.collections.List.$indexName", signature), signature, "int"))
      case (listOwner, "add", 1) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("boolean", Seq(TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName("kotlin.collections.MutableList.add", signature), signature, "boolean"))
      case (listOwner, "add", 2) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq("int", TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName("kotlin.collections.MutableList.add", signature), signature, TypeConstants.Void))
      case (listOwner, "addAll", 1) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("boolean", Seq("java.util.Collection"))
        Some(MethodInfo(methodFullName("kotlin.collections.MutableList.addAll", signature), signature, "boolean"))
      case (listOwner, "addAll", 2) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("boolean", Seq("int", "java.util.Collection"))
        Some(MethodInfo(methodFullName("kotlin.collections.MutableList.addAll", signature), signature, "boolean"))
      case (listOwner, "remove", 1) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("boolean", Seq(TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName("kotlin.collections.MutableList.remove", signature), signature, "boolean"))
      case (listOwner, "removeAt", 1) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("int"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.MutableList.removeAt", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case (listOwner, mutationName, 1)
          if ListInterfaceTypeFullNames
            .contains(listOwner) && MutableListCollectionMutationNames.contains(mutationName) =>
        val signature = methodSignature("boolean", Seq("java.util.Collection"))
        Some(
          MethodInfo(methodFullName(s"kotlin.collections.MutableList.$mutationName", signature), signature, "boolean")
        )
      case (listOwner, "set", 2) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("int", TypeConstants.JavaLangObject))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.MutableList.set", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case (listOwner, "clear", 0) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature(TypeConstants.Void, Nil)
        Some(
          MethodInfo(methodFullName("kotlin.collections.MutableList.clear", signature), signature, TypeConstants.Void)
        )
      case (setOwner, "contains", 1) if SetInterfaceTypeFullNames.contains(setOwner) =>
        val signature = methodSignature("boolean", Seq(TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName("kotlin.collections.Set.contains", signature), signature, "boolean"))
      case (setOwner, "isEmpty", 0) if SetInterfaceTypeFullNames.contains(setOwner) =>
        val signature = methodSignature("boolean", Nil)
        Some(MethodInfo(methodFullName("kotlin.collections.Set.isEmpty", signature), signature, "boolean"))
      case (setOwner, mutationName, 1)
          if SetInterfaceTypeFullNames.contains(setOwner) && MutableSetElementMutationNames.contains(mutationName) =>
        val signature = methodSignature("boolean", Seq(TypeConstants.JavaLangObject))
        Some(
          MethodInfo(methodFullName(s"kotlin.collections.MutableSet.$mutationName", signature), signature, "boolean")
        )
      case (setOwner, mutationName, 1)
          if SetInterfaceTypeFullNames.contains(setOwner) && MutableSetCollectionMutationNames.contains(mutationName) =>
        val signature = methodSignature("boolean", Seq("java.util.Collection"))
        Some(
          MethodInfo(methodFullName(s"kotlin.collections.MutableSet.$mutationName", signature), signature, "boolean")
        )
      case (setOwner, "clear", 0) if SetInterfaceTypeFullNames.contains(setOwner) =>
        val signature = methodSignature(TypeConstants.Void, Nil)
        Some(
          MethodInfo(methodFullName("kotlin.collections.MutableSet.clear", signature), signature, TypeConstants.Void)
        )
      case ("java.util.HashMap", "containsKey", 1) =>
        val signature = methodSignature("boolean", Seq(TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName("java.util.HashMap.containsKey", signature), signature, "boolean"))
      case (mapOwner, "get", 1) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq(TypeConstants.JavaLangObject))
        Some(
          MethodInfo(methodFullName("kotlin.collections.Map.get", signature), signature, TypeConstants.JavaLangObject)
        )
      case (mapOwner, "getOrDefault", 2) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq(TypeConstants.JavaLangObject, TypeConstants.JavaLangObject))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.Map.getOrDefault", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case (mapOwner, mapPredicate, 1)
          if MapInterfaceTypeFullNames.contains(mapOwner) &&
            MapMemberPredicateNames.contains(mapPredicate) =>
        val signature = methodSignature("boolean", Seq(TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName(s"kotlin.collections.Map.$mapPredicate", signature), signature, "boolean"))
      case (mapOwner, "isEmpty", 0) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("boolean", Nil)
        Some(MethodInfo(methodFullName("kotlin.collections.Map.isEmpty", signature), signature, "boolean"))
      case (mapOwner, "iterator", 0) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.Iterator", Nil)
        Some(MethodInfo(methodFullName(s"$mapOwner.iterator", signature), signature, "java.util.Iterator"))
      case (mapOwner, "put", 2) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq(TypeConstants.JavaLangObject, TypeConstants.JavaLangObject))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.MutableMap.put", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case (mapOwner, "remove", 1) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq(TypeConstants.JavaLangObject))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.MutableMap.remove", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case (mapOwner, "putAll", 1) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq("java.util.Map"))
        Some(
          MethodInfo(methodFullName("kotlin.collections.MutableMap.putAll", signature), signature, TypeConstants.Void)
        )
      case (mapOwner, "clear", 0) if MapInterfaceTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature(TypeConstants.Void, Nil)
        Some(
          MethodInfo(methodFullName("kotlin.collections.MutableMap.clear", signature), signature, TypeConstants.Void)
        )
      case (iterableOwner, "iterator", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.Iterator", Nil)
        Some(MethodInfo(methodFullName(s"$iterableOwner.iterator", signature), signature, "java.util.Iterator"))
      case (MapEntryTypeFullName, componentName, 0) if MapEntryComponentNames.contains(componentName) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq(MapEntryTypeFullName))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$componentName", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (PairTypeFullName, componentName, 0) if PairComponentNames.contains(componentName) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Nil)
        Some(
          MethodInfo(
            methodFullName(s"$PairTypeFullName.$componentName", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case (TripleTypeFullName, componentName, 0) if TripleComponentNames.contains(componentName) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Nil)
        Some(
          MethodInfo(
            methodFullName(s"$TripleTypeFullName.$componentName", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case ("java.util.Iterator", "hasNext", 0) =>
        val signature = methodSignature("boolean", Nil)
        Some(MethodInfo(methodFullName("kotlin.collections.Iterator.hasNext", signature), signature, "boolean"))
      case ("java.util.Iterator", "next", 0) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Nil)
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.Iterator.next", signature),
            signature,
            TypeConstants.JavaLangObject
          )
        )
      case ("java.lang.Runtime", "getRuntime", 0) =>
        val signature = methodSignature("java.lang.Runtime", Nil)
        Some(
          MethodInfo(
            methodFullName("java.lang.Runtime.getRuntime", signature),
            signature,
            "java.lang.Runtime",
            isStatic = true
          )
        )
      case ("java.lang.Runtime", "exec", 1) =>
        val signature = methodSignature("java.lang.Process", Seq("java.lang.String"))
        Some(MethodInfo(methodFullName("java.lang.Runtime.exec", signature), signature, "java.lang.Process"))
      case ("java.lang.StringBuilder", "append", 1) =>
        val signature = methodSignature("java.lang.StringBuilder", Seq("java.lang.String"))
        Some(
          MethodInfo(methodFullName("java.lang.StringBuilder.append", signature), signature, "java.lang.StringBuilder")
        )
      case ("java.lang.StringBuilder", "toString", 0) =>
        val signature = methodSignature("java.lang.String", Nil)
        Some(MethodInfo(methodFullName("java.lang.StringBuilder.toString", signature), signature, "java.lang.String"))
      case ("java.util.UUID", "randomUUID", 0) =>
        val signature = methodSignature("java.util.UUID", Nil)
        Some(
          MethodInfo(
            methodFullName("java.util.UUID.randomUUID", signature),
            signature,
            "java.util.UUID",
            isStatic = true
          )
        )
      case ("kotlin.random.Random", "nextBoolean", 0) =>
        val signature = methodSignature("boolean", Nil)
        Some(MethodInfo(methodFullName("kotlin.random.Random.nextBoolean", signature), signature, "boolean"))
      case ("kotlin.random.Random", "nextInt", 0) =>
        val signature = methodSignature("int", Nil)
        Some(MethodInfo(methodFullName("kotlin.random.Random.nextInt", signature), signature, "int"))
      case ("kotlin.random.Random", "nextInt", 1) =>
        val signature = methodSignature("int", Seq("int"))
        Some(MethodInfo(methodFullName("kotlin.random.Random.nextInt", signature), signature, "int", isStatic = true))
      case ("kotlin.random.Random", "nextInt", 2) =>
        val signature = methodSignature("int", Seq("int", "int"))
        Some(MethodInfo(methodFullName("kotlin.random.Random.nextInt", signature), signature, "int", isStatic = true))
      case _ =>
        None
    }
  }

  private def builtinExtensionMethodInfo(owner: String, name: String, arity: Int): Option[MethodInfo] =
    builtinExtensionMethodInfo(owner, name, Seq.fill(arity)(TypeConstants.Any))

  private def builtinExtensionMethodInfo(
    owner: String,
    name: String,
    argumentTypeFullNames: Seq[String]
  ): Option[MethodInfo] = {
    val arity = argumentTypeFullNames.size
    (owner, name, arity) match {
      case ("java.lang.String", "trim", 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String"))
        Some(
          MethodInfo(methodFullName("kotlin.text.trim", signature), signature, "java.lang.String", isExtension = true)
        )
      case ("java.lang.String", caseConversionName, 0) if StringCaseConversionNames.contains(caseConversionName) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$caseConversionName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", prefixSuffixName, 1)
          if StringPrefixSuffixNames.contains(prefixSuffixName) && isStringArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("boolean", Seq("java.lang.String", "java.lang.String", "boolean"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$prefixSuffixName", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case ("java.lang.String", "startsWith", 2)
          if isStringArgument(argumentTypeFullNames, 0) && isIntArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("boolean", Seq("java.lang.String", "java.lang.String", "int", "boolean"))
        Some(MethodInfo(methodFullName("kotlin.text.startsWith", signature), signature, "boolean", isExtension = true))
      case ("java.lang.String", prefixSuffixName, 2)
          if StringPrefixSuffixNames.contains(prefixSuffixName) &&
            isStringArgument(argumentTypeFullNames, 0) &&
            isBooleanArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("boolean", Seq("java.lang.String", "java.lang.String", "boolean"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$prefixSuffixName", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (charSequenceOwner, "contains", 1)
          if CharSequenceTypeFullNames
            .contains(charSequenceOwner) && isCharSequenceArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature("boolean", Seq("java.lang.CharSequence", "java.lang.CharSequence", "boolean"))
        Some(MethodInfo(methodFullName("kotlin.text.contains", signature), signature, "boolean", isExtension = true))
      case (charSequenceOwner, "contains", 1)
          if CharSequenceTypeFullNames.contains(charSequenceOwner) && isCharArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("boolean", Seq("java.lang.CharSequence", "char", "boolean"))
        Some(MethodInfo(methodFullName("kotlin.text.contains", signature), signature, "boolean", isExtension = true))
      case (charSequenceOwner, "contains", 2)
          if CharSequenceTypeFullNames.contains(charSequenceOwner) &&
            isCharSequenceArgument(argumentTypeFullNames, 0) &&
            isBooleanArgument(argumentTypeFullNames, 1) =>
        val signature =
          methodSignature("boolean", Seq("java.lang.CharSequence", "java.lang.CharSequence", "boolean"))
        Some(MethodInfo(methodFullName("kotlin.text.contains", signature), signature, "boolean", isExtension = true))
      case (charSequenceOwner, "contains", 2)
          if CharSequenceTypeFullNames.contains(charSequenceOwner) &&
            isCharArgument(argumentTypeFullNames, 0) &&
            isBooleanArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("boolean", Seq("java.lang.CharSequence", "char", "boolean"))
        Some(MethodInfo(methodFullName("kotlin.text.contains", signature), signature, "boolean", isExtension = true))
      case (charSequenceOwner, searchName, _)
          if CharSequenceTypeFullNames.contains(charSequenceOwner) && StringSearchNames.contains(searchName) =>
        stringSearchNeedleType(argumentTypeFullNames).map { needleType =>
          val signature = methodSignature("int", Seq("java.lang.CharSequence", needleType, "int", "boolean"))
          MethodInfo(methodFullName(s"kotlin.text.$searchName", signature), signature, "int", isExtension = true)
        }
      case ("java.lang.String", "substring", 1) if isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "int"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.substring", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "substring", 2)
          if isIntArgument(argumentTypeFullNames, 0) && isIntArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "int", "int"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.substring", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replace", 2)
          if isStringArgument(argumentTypeFullNames, 0) && isStringArgument(argumentTypeFullNames, 1) =>
        val signature =
          methodSignature(
            "java.lang.String",
            Seq("java.lang.String", "java.lang.String", "java.lang.String", "boolean")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replace", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replace", 2)
          if isCharArgument(argumentTypeFullNames, 0) && isCharArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "char", "char", "boolean"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replace", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replace", 3)
          if isStringArgument(argumentTypeFullNames, 0) &&
            isStringArgument(argumentTypeFullNames, 1) &&
            isBooleanArgument(argumentTypeFullNames, 2) =>
        val signature =
          methodSignature(
            "java.lang.String",
            Seq("java.lang.String", "java.lang.String", "java.lang.String", "boolean")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replace", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replace", 3)
          if isCharArgument(argumentTypeFullNames, 0) &&
            isCharArgument(argumentTypeFullNames, 1) &&
            isBooleanArgument(argumentTypeFullNames, 2) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "char", "char", "boolean"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replace", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replaceFirst", 2)
          if isStringArgument(argumentTypeFullNames, 0) && isStringArgument(argumentTypeFullNames, 1) =>
        val signature =
          methodSignature(
            "java.lang.String",
            Seq("java.lang.String", "java.lang.String", "java.lang.String", "boolean")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replaceFirst", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replaceFirst", 2)
          if isCharArgument(argumentTypeFullNames, 0) && isCharArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "char", "char", "boolean"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replaceFirst", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replaceFirst", 3)
          if isStringArgument(argumentTypeFullNames, 0) &&
            isStringArgument(argumentTypeFullNames, 1) &&
            isBooleanArgument(argumentTypeFullNames, 2) =>
        val signature =
          methodSignature(
            "java.lang.String",
            Seq("java.lang.String", "java.lang.String", "java.lang.String", "boolean")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replaceFirst", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "replaceFirst", 3)
          if isCharArgument(argumentTypeFullNames, 0) &&
            isCharArgument(argumentTypeFullNames, 1) &&
            isBooleanArgument(argumentTypeFullNames, 2) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "char", "char", "boolean"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.replaceFirst", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", trimEndName, 0) if StringTrimEndNames.contains(trimEndName) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$trimEndName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", trimEndName, 1)
          if StringTrimEndNames.contains(trimEndName) && isCharArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "char[]"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$trimEndName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "reversed", 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.reversed", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (charSequenceOwner, "lines", 0) if CharSequenceTypeFullNames.contains(charSequenceOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.CharSequence"))
        Some(
          MethodInfo(methodFullName("kotlin.text.lines", signature), signature, "java.util.List", isExtension = true)
        )
      case (charSequenceOwner, "lineSequence", 0) if CharSequenceTypeFullNames.contains(charSequenceOwner) =>
        val signature = methodSignature("kotlin.sequences.Sequence", Seq("java.lang.CharSequence"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.lineSequence", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (charSequenceOwner, "onEach", 1) if CharSequenceTypeFullNames.contains(charSequenceOwner) =>
        val signature =
          methodSignature("java.lang.CharSequence", Seq("java.lang.CharSequence", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(methodFullName("kotlin.text.onEach", signature), signature, charSequenceOwner, isExtension = true)
        )
      case ("java.lang.String", padName, 1)
          if StringPadNames.contains(padName) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "int", "char"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$padName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", padName, 2)
          if StringPadNames.contains(padName) &&
            isIntArgument(argumentTypeFullNames, 0) &&
            isCharArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "int", "char"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$padName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (charSequenceOwner, defaultingName, 1)
          if CharSequenceTypeFullNames.contains(charSequenceOwner) &&
            StringDefaultingExtensionNames.contains(defaultingName) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("java.lang.CharSequence&java.lang.Object", "kotlin.jvm.functions.Function0")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$defaultingName", signature),
            signature,
            charSequenceOwner,
            isExtension = true
          )
        )
      case ("java.lang.String", removeAffixName, 1)
          if StringRemoveAffixNames.contains(removeAffixName) && isCharSequenceArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "java.lang.CharSequence"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$removeAffixName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "removeSurrounding", 1) if isCharSequenceArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "java.lang.CharSequence"))
        Some(
          MethodInfo(
            methodFullName("kotlin.text.removeSurrounding", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", "removeSurrounding", 2)
          if isCharSequenceArgument(argumentTypeFullNames, 0) && isCharSequenceArgument(argumentTypeFullNames, 1) =>
        val signature =
          methodSignature(
            "java.lang.String",
            Seq("java.lang.String", "java.lang.CharSequence", "java.lang.CharSequence")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.text.removeSurrounding", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", substringAroundName, 1)
          if StringSubstringAroundNames.contains(substringAroundName) && isStringArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature("java.lang.String", Seq("java.lang.String", "java.lang.String", "java.lang.String"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$substringAroundName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", substringAroundName, 1)
          if StringSubstringAroundNames.contains(substringAroundName) && isCharArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "char", "java.lang.String"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$substringAroundName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", substringAroundName, 2)
          if StringSubstringAroundNames.contains(substringAroundName) &&
            isStringArgument(argumentTypeFullNames, 0) &&
            isStringArgument(argumentTypeFullNames, 1) =>
        val signature =
          methodSignature("java.lang.String", Seq("java.lang.String", "java.lang.String", "java.lang.String"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$substringAroundName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case ("java.lang.String", substringAroundName, 2)
          if StringSubstringAroundNames.contains(substringAroundName) &&
            isCharArgument(argumentTypeFullNames, 0) &&
            isStringArgument(argumentTypeFullNames, 1) =>
        val signature = methodSignature("java.lang.String", Seq("java.lang.String", "char", "java.lang.String"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$substringAroundName", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (charSequenceOwner, textPredicateName, 0)
          if CharSequenceTypeFullNames.contains(charSequenceOwner) &&
            StringTextPredicateExtensionNames.contains(textPredicateName) =>
        val signature = methodSignature("boolean", Seq("java.lang.CharSequence"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$textPredicateName", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case ("java.lang.String", conversionName, 0) if StringNumericConversionReturnTypes.contains(conversionName) =>
        val returnType = StringNumericConversionReturnTypes(conversionName)
        val signature  = methodSignature(returnType, Seq("java.lang.String"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.text.$conversionName", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case ("java.lang.String", "split", explicitArgCount) if explicitArgCount >= 1 =>
        val signature =
          methodSignature("java.util.List", Seq("java.lang.CharSequence", "java.lang.String[]", "boolean", "int"))
        Some(
          MethodInfo(methodFullName("kotlin.text.split", signature), signature, "java.util.List", isExtension = true)
        )
      case ("java.io.File", "writeText", explicitArgCount) if explicitArgCount >= 1 =>
        val signature =
          methodSignature(TypeConstants.Void, Seq("java.io.File", "java.lang.String", "java.nio.charset.Charset"))
        Some(
          MethodInfo(
            methodFullName("kotlin.io.writeText", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (_, "to", 1) =>
        val signature = methodSignature("kotlin.Pair", Seq(TypeConstants.JavaLangObject, TypeConstants.JavaLangObject))
        Some(MethodInfo(methodFullName("kotlin.to", signature), signature, "kotlin.Pair", isExtension = true))
      case (rangeOwner, "until", 1) if RangeUntilReturnTypes.contains(rangeOwner) =>
        val returnType = RangeUntilReturnTypes(rangeOwner)
        val signature  = methodSignature(returnType, Seq(rangeOwner, rangeOwner))
        Some(MethodInfo(methodFullName("kotlin.ranges.until", signature), signature, returnType, isExtension = true))
      case (rangeOwner, "downTo", 1) if RangeDownToReturnTypes.contains(rangeOwner) =>
        val returnType = RangeDownToReturnTypes(rangeOwner)
        val signature  = methodSignature(returnType, Seq(rangeOwner, rangeOwner))
        Some(MethodInfo(methodFullName("kotlin.ranges.downTo", signature), signature, returnType, isExtension = true))
      case (rangeOwner, "step", 1) if rangeProgressionTypeFullName(rangeOwner).nonEmpty =>
        val progressionType = rangeProgressionTypeFullName(rangeOwner).get
        val stepType        = if (progressionType == LongProgressionTypeFullName) "long" else "int"
        val signature       = methodSignature(progressionType, Seq(progressionType, stepType))
        Some(
          MethodInfo(methodFullName("kotlin.ranges.step", signature), signature, progressionType, isExtension = true)
        )
      case (arrayOwner, "ifEmpty", 1) if isArrayTypeFullName(arrayOwner) && isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function0")
          )
        Some(
          MethodInfo(methodFullName("kotlin.collections.ifEmpty", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, "ifEmpty", 1) if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq(
              s"${arrayReceiverTypeForMemberSignature(arrayOwner)}&${TypeConstants.JavaLangObject}",
              "kotlin.jvm.functions.Function0"
            )
          )
        Some(
          MethodInfo(methodFullName("kotlin.collections.ifEmpty", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, "orEmpty", 0) if isArrayTypeFullName(arrayOwner) && isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner))
          )
        Some(
          MethodInfo(methodFullName("kotlin.collections.orEmpty", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, "orEmpty", 0) if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner))
          )
        Some(
          MethodInfo(methodFullName("kotlin.collections.orEmpty", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, randomName, 0)
          if isArrayTypeFullName(arrayOwner) && IterableRandomElementNames.contains(randomName) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature  = methodSignature(returnType, Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$randomName", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, "asList", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.util.List", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.asList", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "toTypedArray", 0)
          if isArrayTypeFullName(arrayOwner) && isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature(arrayOwner, Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toTypedArray", signature),
            signature,
            arrayOwner,
            isExtension = true
          )
        )
      case (arrayOwner, "copyOf", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(arrayReturnTypeForSignature(arrayOwner), Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(methodFullName("kotlin.collections.copyOf", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, "copyOf", 1) if isArrayTypeFullName(arrayOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(
            arrayReturnTypeForSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int")
          )
        Some(
          MethodInfo(methodFullName("kotlin.collections.copyOf", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, "copyOfRange", 2)
          if isArrayTypeFullName(arrayOwner) &&
            isIntArgument(argumentTypeFullNames, 0) &&
            isIntArgument(argumentTypeFullNames, 1) =>
        val signature =
          methodSignature(
            arrayReturnTypeForSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int", "int")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.copyOfRange", signature),
            signature,
            arrayOwner,
            isExtension = true
          )
        )
      case (arrayOwner, "sliceArray", 1)
          if isArrayTypeFullName(arrayOwner) && arraySliceArgumentType(argumentTypeFullNames).nonEmpty =>
        val signature =
          methodSignature(
            arrayReturnTypeForSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), arraySliceArgumentType(argumentTypeFullNames).get)
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.sliceArray", signature),
            signature,
            arrayOwner,
            isExtension = true
          )
        )
      case (arrayOwner, "slice", 1)
          if isArrayTypeFullName(arrayOwner) && iterableSliceArgumentType(argumentTypeFullNames).nonEmpty =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), iterableSliceArgumentType(argumentTypeFullNames).get)
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.slice", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "plus", 1)
          if isArrayTypeFullName(arrayOwner) && arrayPlusArgumentType(arrayOwner, argumentTypeFullNames).nonEmpty =>
        val signature =
          methodSignature(
            arrayReturnTypeForSignature(arrayOwner),
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              arrayPlusArgumentType(arrayOwner, argumentTypeFullNames).get
            )
          )
        Some(
          MethodInfo(methodFullName("kotlin.collections.plus", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, "contentEquals", 1)
          if isArrayTypeFullName(arrayOwner) && arrayContentPeerArgumentType(
            arrayOwner,
            argumentTypeFullNames
          ).nonEmpty =>
        val signature =
          methodSignature(
            "boolean",
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              arrayContentPeerArgumentType(arrayOwner, argumentTypeFullNames).get
            )
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.contentEquals", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (arrayOwner, "contentDeepEquals", 1)
          if isArrayTypeFullName(arrayOwner) &&
            !isPrimitiveArrayTypeFullName(arrayOwner) &&
            isArrayArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(
            "boolean",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), TypeConstants.JavaLangObject + "[]")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.contentDeepEquals", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (arrayOwner, "contentHashCode", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("int", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.contentHashCode", signature),
            signature,
            "int",
            isExtension = true
          )
        )
      case (arrayOwner, "contentDeepHashCode", 0)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("int", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.contentDeepHashCode", signature),
            signature,
            "int",
            isExtension = true
          )
        )
      case (arrayOwner, "contentToString", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.lang.String", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.contentToString", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (arrayOwner, "contentDeepToString", 0)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.lang.String", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.contentDeepToString", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (arrayOwner, sortName @ ("sort" | "sortDescending"), 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq(arrayComparableReceiverTypeForSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$sortName", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (arrayOwner, sortName @ ("sortedArray" | "sortedArrayDescending"), 0) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            arrayComparableReceiverTypeForSignature(arrayOwner),
            Seq(arrayComparableReceiverTypeForSignature(arrayOwner))
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$sortName", signature),
            signature,
            arrayOwner,
            isExtension = true
          )
        )
      case (arrayOwner, "sortWith", 1)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature(
          TypeConstants.Void,
          Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.util.Comparator")
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.sortWith", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (arrayOwner, "sortedArrayWith", 1)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.util.Comparator")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.sortedArrayWith", signature),
            signature,
            arrayOwner,
            isExtension = true
          )
        )
      case (arrayOwner, "fill", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            TypeConstants.Void,
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              arrayElementTypeForMemberSignature(arrayOwner),
              "int",
              "int"
            )
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.fill", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (arrayOwner, "reverse", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.reverse", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 0)
          if isArrayTypeFullName(arrayOwner) && IterablePlainBooleanNames.contains(collectionFunction) =>
        val signature = methodSignature("boolean", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && IterablePredicateBooleanNames.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "boolean",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (arrayOwner, "count", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("int", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(MethodInfo(methodFullName("kotlin.collections.count", signature), signature, "int", isExtension = true))
      case (arrayOwner, "count", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature("int", Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.collections.count", signature), signature, "int", isExtension = true))
      case (arrayOwner, "toSortedSet", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.util.SortedSet", Seq(arrayComparableReceiverTypeForSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toSortedSet", signature),
            signature,
            "java.util.SortedSet",
            isExtension = true
          )
        )
      case (arrayOwner, "toSortedSet", 1)
          if isArrayTypeFullName(arrayOwner) && isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.util.SortedSet", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toSortedSet", signature),
            signature,
            "java.util.SortedSet",
            isExtension = true
          )
        )
      case (arrayOwner, "toSortedSet", 1)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.SortedSet",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.util.Comparator")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toSortedSet", signature),
            signature,
            "java.util.SortedSet",
            isExtension = true
          )
        )
      case (arrayOwner, conversionName, 0)
          if isArrayTypeFullName(arrayOwner) && IterableConversionReturnTypes.contains(conversionName) =>
        val returnType = IterableConversionReturnTypes(conversionName)
        val signature  = methodSignature(returnType, Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$conversionName", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, "toMutableList", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.util.List", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMutableList", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "toCollection", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.util.Collection")
          )
        val returnType = argumentTypeFullNames.headOption.getOrElse("java.util.Collection")
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toCollection", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 0)
          if isArrayTypeFullName(arrayOwner) && IterableElementPlainNames.contains(collectionFunction) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature  = methodSignature(returnType, Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && IterableElementFunction1Names.contains(collectionFunction) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature =
          methodSignature(
            returnType,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) &&
            IterableElementAtNames.contains(collectionFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature  = methodSignature(returnType, Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, "getOrNull", 1) if isArrayTypeFullName(arrayOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature  = methodSignature(returnType, Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.getOrNull", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, "elementAtOrElse", 2)
          if isArrayTypeFullName(arrayOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature =
          methodSignature(
            returnType,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.elementAtOrElse", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, "getOrElse", 2) if isArrayTypeFullName(arrayOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature =
          methodSignature(
            returnType,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.getOrElse", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && IterableIndexFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature("int", Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "int",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && IterableToListFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) &&
            IterableIntToListNames.contains(collectionFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.util.List", Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) &&
            ListIntToListNames.contains(collectionFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.util.List", Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "int"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && IterableFunction1ToListNames.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && ListFunction1ToListNames.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction @ ("filterIndexed" | "flatMapIndexed" | "mapIndexed"), 1)
          if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "mapIndexedNotNull", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.mapIndexedNotNull", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "filterNotNull", 0)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.util.List", Seq(arrayReceiverTypeForMemberSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.filterNotNull", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "onEach", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(methodFullName("kotlin.collections.onEach", signature), signature, arrayOwner, isExtension = true)
        )
      case (arrayOwner, "onEachIndexed", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.onEachIndexed", signature),
            signature,
            arrayOwner,
            isExtension = true
          )
        )
      case (arrayOwner, "joinToString", _) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature(
          "java.lang.String",
          Seq(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "int",
            "java.lang.CharSequence",
            "kotlin.jvm.functions.Function1"
          )
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.joinToString", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 2)
          if isArrayTypeFullName(arrayOwner) && IterableFoldFunction2Names.contains(collectionFunction) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            TypeConstants.JavaLangObject,
            "kotlin.jvm.functions.Function2"
          )
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 2)
          if isArrayTypeFullName(arrayOwner) && IterableFoldFunction3Names.contains(collectionFunction) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(
            arrayReceiverTypeForMemberSignature(arrayOwner),
            TypeConstants.JavaLangObject,
            "kotlin.jvm.functions.Function3"
          )
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && ArrayReduceFunction2Names.contains(collectionFunction) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature =
          methodSignature(
            returnType,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && ArrayReduceFunction3Names.contains(collectionFunction) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature =
          methodSignature(
            returnType,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, "sum", 0) if PrimitiveArraySumReturnTypes.contains(arrayOwner) =>
        val returnType = PrimitiveArraySumReturnTypes(arrayOwner)
        val signature  = methodSignature(returnType, Seq(arrayOwner))
        Some(MethodInfo(methodFullName("kotlin.collections.sum", signature), signature, returnType, isExtension = true))
      case (arrayOwner, "average", 0) if PrimitiveArraySumReturnTypes.contains(arrayOwner) =>
        val signature = methodSignature("double", Seq(arrayOwner))
        Some(
          MethodInfo(methodFullName("kotlin.collections.average", signature), signature, "double", isExtension = true)
        )
      case (arrayOwner, collectionFunction, 0)
          if isArrayTypeFullName(arrayOwner) && IterableComparableElementNames.contains(collectionFunction) =>
        val returnType = arrayComparableReturnTypeForSignature(arrayOwner)
        val signature  = methodSignature(returnType, Seq(arrayComparableReceiverTypeForSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && IterableFunction1SelectorElementNames.contains(collectionFunction) =>
        val returnType = arrayElementTypeForMemberSignature(arrayOwner)
        val signature =
          methodSignature(
            returnType,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) && IterableLambdaComparableReturnNames.contains(collectionFunction) =>
        val signature = methodSignature(
          "java.lang.Comparable",
          Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.lang.Comparable",
            isExtension = true
          )
        )
      case (arrayOwner, "sumOf", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature("int", Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.collections.sumOf", signature), signature, "int", isExtension = true))
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) &&
            IterableLambdaObjectReturnNames.contains(collectionFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (arrayOwner, "filterIsInstance", 0)
          if isArrayTypeFullName(arrayOwner) && isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = s"${Defines.UnresolvedSignature}(0)"
        Some(
          MethodInfo(
            methodFullName(s"${Defines.UnresolvedNamespace}.filterIsInstance", signature),
            signature,
            TypeConstants.Any,
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 0)
          if isArrayTypeFullName(arrayOwner) && IterablePlainToListNames.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForPlainToListSignature(collectionFunction, arrayOwner))
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "filterIsInstanceTo", 1)
          if isArrayTypeFullName(arrayOwner) && isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature = s"${Defines.UnresolvedSignature}(1)"
        Some(
          MethodInfo(
            methodFullName(s"${Defines.UnresolvedNamespace}.filterIsInstanceTo", signature),
            signature,
            TypeConstants.Any,
            isExtension = true
          )
        )
      case (arrayOwner, "filterIsInstanceTo", 1)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.util.Collection")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.filterIsInstanceTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (arrayOwner, "forEach", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            TypeConstants.Void,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.forEach", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (arrayOwner, "forEachIndexed", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            TypeConstants.Void,
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.forEachIndexed", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (arrayOwner, "toMap", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("java.util.Map", Seq(arrayPairReceiverTypeForSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (arrayOwner, "toMap", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature("java.util.Map", Seq(arrayPairReceiverTypeForSignature(arrayOwner), "java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (arrayOwner, "zip", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), arrayZipArgumentType(argumentTypeFullNames))
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.zip", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "zip", 2) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              arrayZipArgumentType(argumentTypeFullNames),
              "kotlin.jvm.functions.Function2"
            )
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.zip", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (arrayOwner, "unzip", 0) if isArrayTypeFullName(arrayOwner) =>
        val signature = methodSignature("kotlin.Pair", Seq(arrayPairReceiverTypeForSignature(arrayOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.unzip", signature),
            signature,
            "kotlin.Pair",
            isExtension = true
          )
        )
      case (arrayOwner, "partition", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "kotlin.Pair",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.partition", signature),
            signature,
            "kotlin.Pair",
            isExtension = true
          )
        )
      case (arrayOwner, setOperation, 1)
          if isArrayTypeFullName(arrayOwner) && IterableSetOperationNames.contains(setOperation) =>
        val signature =
          methodSignature("java.util.Set", Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$setOperation", signature),
            signature,
            "java.util.Set",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 1)
          if isArrayTypeFullName(arrayOwner) &&
            IterableToMapFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 2)
          if isArrayTypeFullName(arrayOwner) &&
            IterableToMapFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              "kotlin.jvm.functions.Function1",
              "kotlin.jvm.functions.Function1"
            )
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (arrayOwner, "groupingBy", 1) if isArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "kotlin.collections.Grouping",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.groupingBy", signature),
            signature,
            "kotlin.collections.Grouping",
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 2)
          if isArrayTypeFullName(arrayOwner) &&
            IterableToCollectionDestinationFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              "java.util.Collection",
              "kotlin.jvm.functions.Function1"
            )
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 2)
          if isArrayTypeFullName(arrayOwner) &&
            IterableToCollectionDestinationFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              "java.util.Collection",
              "kotlin.jvm.functions.Function2"
            )
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (arrayOwner, "filterNotNullTo", 1)
          if isArrayTypeFullName(arrayOwner) && !isPrimitiveArrayTypeFullName(arrayOwner) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.util.Collection")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.filterNotNullTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 2)
          if isArrayTypeFullName(arrayOwner) &&
            IterableToMapDestinationFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq(arrayReceiverTypeForMemberSignature(arrayOwner), "java.util.Map", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (arrayOwner, collectionFunction, 3)
          if isArrayTypeFullName(arrayOwner) &&
            IterableToMapDestinationFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq(
              arrayReceiverTypeForMemberSignature(arrayOwner),
              "java.util.Map",
              "kotlin.jvm.functions.Function1",
              "kotlin.jvm.functions.Function1"
            )
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 0)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterablePlainBooleanNames.contains(collectionFunction) =>
        val signature = methodSignature("boolean", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterablePredicateBooleanNames.contains(collectionFunction) =>
        val signature = methodSignature("boolean", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (collectionOwner, "isNotEmpty", 0) if CollectionInterfaceTypeFullNames.contains(collectionOwner) =>
        val signature = methodSignature("boolean", Seq("java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.isNotEmpty", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (iterableOwner, "count", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("int", Seq(collectionReceiverTypeForSignature(iterableOwner)))
        Some(MethodInfo(methodFullName("kotlin.collections.count", signature), signature, "int", isExtension = true))
      case (iterableOwner, "count", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("int", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.collections.count", signature), signature, "int", isExtension = true))
      case (iterableOwner, conversionName, 0)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableConversionReturnTypes.contains(conversionName) =>
        val returnType = IterableConversionReturnTypes(conversionName)
        val signature  = methodSignature(returnType, Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$conversionName", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (iterableOwner, "toSortedSet", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.SortedSet", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toSortedSet", signature),
            signature,
            "java.util.SortedSet",
            isExtension = true
          )
        )
      case (iterableOwner, "toSortedSet", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.SortedSet", Seq("java.lang.Iterable", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toSortedSet", signature),
            signature,
            "java.util.SortedSet",
            isExtension = true
          )
        )
      case (iterableOwner, "toMutableList", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.List", Seq(collectionReceiverTypeForSignature(iterableOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMutableList", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (collectionOwner, "toTypedArray", 0) if CollectionInterfaceTypeFullNames.contains(collectionOwner) =>
        val signature = methodSignature(TypeConstants.JavaLangObject + "[]", Seq("java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toTypedArray", signature),
            signature,
            TypeConstants.JavaLangObject + "[]",
            isExtension = true
          )
        )
      case (collectionOwner, arrayConversionName, 0)
          if CollectionInterfaceTypeFullNames.contains(collectionOwner) &&
            PrimitiveCollectionArrayConversionReturnTypes.contains(arrayConversionName) =>
        val returnType = PrimitiveCollectionArrayConversionReturnTypes(arrayConversionName)
        val signature  = methodSignature(returnType, Seq("java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$arrayConversionName", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (iterableOwner, "slice", 1)
          if IterableTypeFullNames
            .contains(iterableOwner) && iterableSliceArgumentType(argumentTypeFullNames).nonEmpty =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(
              listOrIterableReceiverTypeForSignature(iterableOwner),
              iterableSliceArgumentType(argumentTypeFullNames).get
            )
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.slice", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "toCollection", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature  = methodSignature("java.util.Collection", Seq("java.lang.Iterable", "java.util.Collection"))
        val returnType = argumentTypeFullNames.headOption.getOrElse("java.util.Collection")
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toCollection", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (collectionOwner, "ifEmpty", 1) if CollectionInterfaceTypeFullNames.contains(collectionOwner) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq(s"java.util.Collection&${TypeConstants.JavaLangObject}", "kotlin.jvm.functions.Function0")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.ifEmpty", signature),
            signature,
            collectionOwner,
            isExtension = true
          )
        )
      case (listOwner, "orEmpty", 0) if ListInterfaceTypeFullNames.contains(listOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.util.List"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.orEmpty", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (setOwner, "orEmpty", 0) if SetInterfaceTypeFullNames.contains(setOwner) =>
        val signature = methodSignature("java.util.Set", Seq("java.util.Set"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.orEmpty", signature),
            signature,
            "java.util.Set",
            isExtension = true
          )
        )
      case (collectionOwner, randomName, 0)
          if CollectionInterfaceTypeFullNames
            .contains(collectionOwner) && IterableRandomElementNames.contains(randomName) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$randomName", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, "requireNoNulls", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.List", Seq(listOrIterableReceiverTypeForSignature(iterableOwner)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.requireNoNulls", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "toMap", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.Map", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (iterableOwner, "toMap", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.Map", Seq("java.lang.Iterable", "java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (iterableOwner, "zip", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.zip", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "zip", 2) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq("java.lang.Iterable", "java.lang.Iterable", "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.zip", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "unzip", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("kotlin.Pair", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.unzip", signature),
            signature,
            "kotlin.Pair",
            isExtension = true
          )
        )
      case (iterableOwner, "partition", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("kotlin.Pair", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.partition", signature),
            signature,
            "kotlin.Pair",
            isExtension = true
          )
        )
      case (iterableOwner, setOperation, 1)
          if IterableTypeFullNames.contains(iterableOwner) && IterableSetOperationNames.contains(setOperation) =>
        val signature = methodSignature("java.util.Set", Seq("java.lang.Iterable", "java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$setOperation", signature),
            signature,
            "java.util.Set",
            isExtension = true
          )
        )
      case (setOwner, "plus", 1) if SetOperatorTypeFullNames.contains(setOwner) =>
        val signature =
          methodSignature("java.util.Set", Seq("java.util.Set", collectionPlusMinusArgumentType(argumentTypeFullNames)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.plus", signature),
            signature,
            "java.util.Set",
            isExtension = true
          )
        )
      case (setOwner, "minus", 1) if SetOperatorTypeFullNames.contains(setOwner) =>
        val signature =
          methodSignature("java.util.Set", Seq("java.util.Set", collectionPlusMinusArgumentType(argumentTypeFullNames)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.minus", signature),
            signature,
            "java.util.Set",
            isExtension = true
          )
        )
      case (iterableOwner, "plus", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq(
              collectionReceiverTypeForSignature(iterableOwner),
              collectionPlusMinusArgumentType(argumentTypeFullNames)
            )
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.plus", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "minus", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq("java.lang.Iterable", collectionPlusMinusArgumentType(argumentTypeFullNames))
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.minus", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, "plus", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature("java.util.Map", Seq("java.util.Map", mapPlusArgumentType(argumentTypeFullNames)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.plus", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "minus", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature("java.util.Map", Seq("java.util.Map", mapMinusArgumentType(argumentTypeFullNames)))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.minus", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) &&
            MapFunction1ToMapNames.contains(mapFunction) =>
        val signature = methodSignature("java.util.Map", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "getValue", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.util.Map", TypeConstants.JavaLangObject))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.getValue", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (mapOwner, defaultingName, 2)
          if MapTypeFullNames.contains(mapOwner) && MapDefaultingFunctionNames.contains(defaultingName) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("java.util.Map", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function0")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$defaultingName", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (mapOwner, "set", 2) if MapTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature(
            TypeConstants.Void,
            Seq("java.util.Map", TypeConstants.JavaLangObject, TypeConstants.JavaLangObject)
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.set", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (mapOwner, "isNotEmpty", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("boolean", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.isNotEmpty", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 0)
          if MapTypeFullNames.contains(mapOwner) && MapPlainBooleanNames.contains(mapFunction) =>
        val signature = methodSignature("boolean", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) && MapPredicateBooleanNames.contains(mapFunction) =>
        val signature = methodSignature("boolean", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (mapOwner, "count", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("int", Seq("java.util.Map"))
        Some(MethodInfo(methodFullName("kotlin.collections.count", signature), signature, "int", isExtension = true))
      case (mapOwner, "count", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("int", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.collections.count", signature), signature, "int", isExtension = true))
      case (mapOwner, "forEach", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.forEach", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (mapOwner, "onEach", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.Map", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.onEach", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "flatMap", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.flatMap", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) &&
            IterableFunction1SelectorElementNames.contains(mapFunction) =>
        val signature = methodSignature(MapEntryTypeFullName, Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            MapEntryTypeFullName,
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) &&
            IterableComparatorElementNames.contains(mapFunction) =>
        val signature = methodSignature(MapEntryTypeFullName, Seq("java.util.Map", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            MapEntryTypeFullName,
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) &&
            IterableLambdaComparableReturnNames.contains(mapFunction) =>
        val signature = methodSignature("java.lang.Comparable", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.lang.Comparable",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) &&
            IterableLambdaObjectReturnNames.contains(mapFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (mapOwner, "joinToString", _) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature(
          "java.lang.String",
          Seq(
            "java.util.Map",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "int",
            "java.lang.CharSequence",
            "kotlin.jvm.functions.Function1"
          )
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.joinToString", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (mapOwner, "joinTo", explicitArgCount) if MapTypeFullNames.contains(mapOwner) && explicitArgCount >= 1 =>
        val signature = methodSignature(
          "java.lang.Appendable",
          Seq(
            "java.util.Map",
            "java.lang.Appendable",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "int",
            "java.lang.CharSequence",
            "kotlin.jvm.functions.Function1"
          )
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.joinTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.lang.Appendable"),
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 2)
          if MapTypeFullNames.contains(mapOwner) && IterableFoldFunction2Names.contains(mapFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("java.util.Map", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 2)
          if MapTypeFullNames.contains(mapOwner) && IterableFoldFunction3Names.contains(mapFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("java.util.Map", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) && IterableReduceFunction2Names.contains(mapFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.util.Map", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) && IterableReduceFunction3Names.contains(mapFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.util.Map", "kotlin.jvm.functions.Function3"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 2)
          if MapTypeFullNames.contains(mapOwner) && IterableRunningFoldFunction2Names.contains(mapFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq("java.util.Map", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 2)
          if MapTypeFullNames.contains(mapOwner) && IterableRunningFoldFunction3Names.contains(mapFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq("java.util.Map", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) && IterableRunningReduceFunction2Names.contains(mapFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.util.Map", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) && IterableRunningReduceFunction3Names.contains(mapFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.util.Map", "kotlin.jvm.functions.Function3"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, "sumOf", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("int", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.collections.sumOf", signature), signature, "int", isExtension = true))
      case (mapOwner, conversionName, 0)
          if MapTypeFullNames.contains(mapOwner) && MapConversionReturnTypes.contains(conversionName) =>
        val returnType = MapConversionReturnTypes(conversionName)
        val signature  = methodSignature(returnType, Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$conversionName", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (mapOwner, "filterNotNull", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.filterNotNull", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, "requireNoNulls", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.requireNoNulls", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 1)
          if MapTypeFullNames.contains(mapOwner) && IterableToMapFunction1Names.contains(mapFunction) =>
        val signature = methodSignature("java.util.Map", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 2)
          if MapTypeFullNames.contains(mapOwner) && IterableToMapFunction2Names.contains(mapFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq("java.util.Map", "kotlin.jvm.functions.Function1", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "groupingBy", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature("kotlin.collections.Grouping", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.groupingBy", signature),
            signature,
            "kotlin.collections.Grouping",
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 2)
          if MapTypeFullNames.contains(mapOwner) && IterableToMapDestinationFunction1Names.contains(mapFunction) =>
        val signature =
          methodSignature("java.util.Map", Seq("java.util.Map", "java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (mapOwner, mapFunction, 3)
          if MapTypeFullNames.contains(mapOwner) && IterableToMapDestinationFunction2Names.contains(mapFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq("java.util.Map", "java.util.Map", "kotlin.jvm.functions.Function1", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$mapFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (mapOwner, "map", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.map", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, collectionFunction, 2)
          if MapTypeFullNames.contains(mapOwner) &&
            MapToCollectionDestinationFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq("java.util.Map", "java.util.Collection", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (mapOwner, "ifEmpty", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq(s"java.util.Map&${TypeConstants.JavaLangObject}", "kotlin.jvm.functions.Function0")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.ifEmpty", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "toMap", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.Map", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "toMap", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.Map", Seq("java.util.Map", "java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "asIterable", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.lang.Iterable", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.asIterable", signature),
            signature,
            "java.lang.Iterable",
            isExtension = true
          )
        )
      case (mapOwner, "asSequence", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("kotlin.sequences.Sequence", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.asSequence", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (mapOwner, "toList", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toList", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (mapOwner, "toProperties", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.Properties", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toProperties", signature),
            signature,
            "java.util.Properties",
            isExtension = true
          )
        )
      case (mapOwner, "toSortedMap", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.SortedMap", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toSortedMap", signature),
            signature,
            "java.util.SortedMap",
            isExtension = true
          )
        )
      case (mapOwner, "toSortedMap", 1) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.SortedMap", Seq("java.util.Map", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toSortedMap", signature),
            signature,
            "java.util.SortedMap",
            isExtension = true
          )
        )
      case (mapOwner, "orEmpty", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.Map", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.orEmpty", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (mapOwner, "toMutableMap", 0) if MapTypeFullNames.contains(mapOwner) =>
        val signature = methodSignature("java.util.Map", Seq("java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMutableMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToMapFunction1Names.contains(collectionFunction) =>
        val signature = methodSignature("java.util.Map", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToMapFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (iterableOwner, "groupingBy", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature =
          methodSignature("kotlin.collections.Grouping", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.groupingBy", signature),
            signature,
            "kotlin.collections.Grouping",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToCollectionDestinationFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq("java.lang.Iterable", "java.util.Collection", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToCollectionDestinationFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq("java.lang.Iterable", "java.util.Collection", "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (iterableOwner, "filterIsInstanceTo", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.Collection", Seq("java.lang.Iterable", "java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.filterIsInstanceTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (iterableOwner, "filterNotNullTo", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.Collection", Seq("java.lang.Iterable", "java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.filterNotNullTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToMapDestinationFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature("java.util.Map", Seq("java.lang.Iterable", "java.util.Map", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 3)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToMapDestinationFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq(
              "java.lang.Iterable",
              "java.util.Map",
              "kotlin.jvm.functions.Function1",
              "kotlin.jvm.functions.Function1"
            )
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceFunction1ReturnSequenceNames.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceFunction2ReturnSequenceNames.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceIntReturnSequenceNames.contains(sequenceFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("kotlin.sequences.Sequence", Seq("kotlin.sequences.Sequence", "int"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "ifEmpty", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function0")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.ifEmpty", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, randomName, 0)
          if SequenceTypeFullNames.contains(sequenceOwner) && IterableRandomElementNames.contains(randomName) =>
        val signature = s"${Defines.UnresolvedSignature}(0)"
        Some(
          MethodInfo(
            methodFullName(s"${Defines.UnresolvedNamespace}.$randomName", signature),
            signature,
            TypeConstants.Any,
            isExtension = true
          )
        )
      case (sequenceOwner, "chunked", 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "int", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.chunked", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "chunked", 1)
          if SequenceTypeFullNames.contains(sequenceOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("kotlin.sequences.Sequence", Seq("kotlin.sequences.Sequence", "int"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.chunked", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "windowed", explicitArgCount)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            explicitArgCount >= 4 &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "int", "int", "boolean", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.windowed", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "windowed", explicitArgCount)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            explicitArgCount >= 1 &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature("kotlin.sequences.Sequence", Seq("kotlin.sequences.Sequence", "int", "int", "boolean"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.windowed", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 0)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceElementPlainNames.contains(sequenceFunction) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceElementFunction1Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceElementAtNames.contains(sequenceFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("kotlin.sequences.Sequence", "int"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, "elementAtOrElse", 2)
          if SequenceTypeFullNames.contains(sequenceOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", "int", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.elementAtOrElse", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 0)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequencePlainBooleanNames.contains(sequenceFunction) =>
        val signature = methodSignature("boolean", Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (sequenceOwner, "contains", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("boolean", Seq("kotlin.sequences.Sequence", TypeConstants.JavaLangObject))
        Some(
          MethodInfo(methodFullName("kotlin.sequences.contains", signature), signature, "boolean", isExtension = true)
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceIndexFunction1Names.contains(sequenceFunction) =>
        val signature = methodSignature("int", Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "int",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequencePredicateBooleanNames.contains(sequenceFunction) =>
        val signature = methodSignature("boolean", Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "boolean",
            isExtension = true
          )
        )
      case (sequenceOwner, "count", 0) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("int", Seq("kotlin.sequences.Sequence"))
        Some(MethodInfo(methodFullName("kotlin.sequences.count", signature), signature, "int", isExtension = true))
      case (sequenceOwner, "count", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("int", Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.sequences.count", signature), signature, "int", isExtension = true))
      case (sequenceOwner, conversionName, 0)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceConversionReturnTypes.contains(conversionName) =>
        val returnType = SequenceConversionReturnTypes(conversionName)
        val signature  = methodSignature(returnType, Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$conversionName", signature),
            signature,
            returnType,
            isExtension = true
          )
        )
      case (sequenceOwner, "toCollection", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature("java.util.Collection", Seq("kotlin.sequences.Sequence", "java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.toCollection", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (sequenceOwner, "toSortedSet", 0) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("java.util.SortedSet", Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.toSortedSet", signature),
            signature,
            "java.util.SortedSet",
            isExtension = true
          )
        )
      case (sequenceOwner, "toSortedSet", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature("java.util.SortedSet", Seq("kotlin.sequences.Sequence", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.toSortedSet", signature),
            signature,
            "java.util.SortedSet",
            isExtension = true
          )
        )
      case (sequenceOwner, "toMap", 0) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("java.util.Map", Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (sequenceOwner, "toMap", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("java.util.Map", Seq("kotlin.sequences.Sequence", "java.util.Map"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.toMap", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (sequenceOwner, "joinToString", _) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature(
          "java.lang.String",
          Seq(
            "kotlin.sequences.Sequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "int",
            "java.lang.CharSequence",
            "kotlin.jvm.functions.Function1"
          )
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.joinToString", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (sequenceOwner, "joinTo", explicitArgCount)
          if SequenceTypeFullNames.contains(sequenceOwner) && explicitArgCount >= 1 =>
        val signature = methodSignature(
          "java.lang.Appendable",
          Seq(
            "kotlin.sequences.Sequence",
            "java.lang.Appendable",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "int",
            "java.lang.CharSequence",
            "kotlin.jvm.functions.Function1"
          )
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.joinTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.lang.Appendable"),
            isExtension = true
          )
        )
      case (sequenceOwner, "forEach", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature(TypeConstants.Void, Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.forEach", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (sequenceOwner, "forEachIndexed", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature(TypeConstants.Void, Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.forEachIndexed", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableLambdaComparableReturnNames.contains(sequenceFunction) =>
        val signature =
          methodSignature("java.lang.Comparable", Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "java.lang.Comparable",
            isExtension = true
          )
        )
      case (sequenceOwner, "sumOf", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("int", Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.sequences.sumOf", signature), signature, "int", isExtension = true))
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableFoldFunction2Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableFoldFunction3Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableReduceFunction2Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableReduceFunction3Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableRunningFoldFunction2Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableRunningFoldFunction3Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableRunningReduceFunction2Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableRunningReduceFunction3Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "sum", 0) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("int", Seq("kotlin.sequences.Sequence"))
        Some(MethodInfo(methodFullName("kotlin.sequences.sum", signature), signature, "int", isExtension = true))
      case (sequenceOwner, "average", 0) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("double", Seq("kotlin.sequences.Sequence"))
        Some(MethodInfo(methodFullName("kotlin.sequences.average", signature), signature, "double", isExtension = true))
      case (sequenceOwner, sequenceFunction, 0)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequenceComparableElementNames.contains(sequenceFunction) =>
        val signature = methodSignature("java.lang.Comparable", Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "java.lang.Comparable",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableFunction1SelectorElementNames.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableComparatorElementNames.contains(sequenceFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("kotlin.sequences.Sequence", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableLambdaObjectReturnNames.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 0)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            SequencePlainReturnSequenceNames.contains(sequenceFunction) =>
        val signature = methodSignature("kotlin.sequences.Sequence", Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "plus", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", sequencePlusMinusArgumentType(argumentTypeFullNames))
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.plus", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "minus", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", sequencePlusMinusArgumentType(argumentTypeFullNames))
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.minus", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "sortedWith", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature("kotlin.sequences.Sequence", Seq("kotlin.sequences.Sequence", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.sortedWith", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "zip", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature("kotlin.sequences.Sequence", Seq("kotlin.sequences.Sequence", "kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.zip", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "zip", 2) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature(
          "kotlin.sequences.Sequence",
          Seq("kotlin.sequences.Sequence", "kotlin.sequences.Sequence", "kotlin.jvm.functions.Function2")
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.zip", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "unzip", 0) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("kotlin.Pair", Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(methodFullName("kotlin.sequences.unzip", signature), signature, "kotlin.Pair", isExtension = true)
        )
      case (sequenceOwner, "partition", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature("kotlin.Pair", Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.partition", signature),
            signature,
            "kotlin.Pair",
            isExtension = true
          )
        )
      case (sequenceOwner, "zipWithNext", 0) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature = methodSignature("kotlin.sequences.Sequence", Seq("kotlin.sequences.Sequence"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.zipWithNext", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, "zipWithNext", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature(
            "kotlin.sequences.Sequence",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.zipWithNext", signature),
            signature,
            "kotlin.sequences.Sequence",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 1)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableToMapFunction1Names.contains(sequenceFunction) =>
        val signature =
          methodSignature("java.util.Map", Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableToMapFunction2Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            "java.util.Map",
            isExtension = true
          )
        )
      case (sequenceOwner, "groupingBy", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature(
            "kotlin.collections.Grouping",
            Seq("kotlin.sequences.Sequence", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.groupingBy", signature),
            signature,
            "kotlin.collections.Grouping",
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableToCollectionDestinationFunction1Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq("kotlin.sequences.Sequence", "java.util.Collection", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableToCollectionDestinationFunction2Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "java.util.Collection",
            Seq("kotlin.sequences.Sequence", "java.util.Collection", "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (sequenceOwner, "filterIsInstanceTo", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature("java.util.Collection", Seq("kotlin.sequences.Sequence", "java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.filterIsInstanceTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (sequenceOwner, "filterNotNullTo", 1) if SequenceTypeFullNames.contains(sequenceOwner) =>
        val signature =
          methodSignature("java.util.Collection", Seq("kotlin.sequences.Sequence", "java.util.Collection"))
        Some(
          MethodInfo(
            methodFullName("kotlin.sequences.filterNotNullTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Collection"),
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 2)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableToMapDestinationFunction1Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq("kotlin.sequences.Sequence", "java.util.Map", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (sequenceOwner, sequenceFunction, 3)
          if SequenceTypeFullNames.contains(sequenceOwner) &&
            IterableToMapDestinationFunction2Names.contains(sequenceFunction) =>
        val signature =
          methodSignature(
            "java.util.Map",
            Seq(
              "kotlin.sequences.Sequence",
              "java.util.Map",
              "kotlin.jvm.functions.Function1",
              "kotlin.jvm.functions.Function1"
            )
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.sequences.$sequenceFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.util.Map"),
            isExtension = true
          )
        )
      case (iterableOwner, "joinToString", _) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature(
          "java.lang.String",
          Seq(
            "java.lang.Iterable",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "int",
            "java.lang.CharSequence",
            "kotlin.jvm.functions.Function1"
          )
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.joinToString", signature),
            signature,
            "java.lang.String",
            isExtension = true
          )
        )
      case (iterableOwner, "joinTo", explicitArgCount)
          if IterableTypeFullNames.contains(iterableOwner) && explicitArgCount >= 1 =>
        val signature = methodSignature(
          "java.lang.Appendable",
          Seq(
            "java.lang.Iterable",
            "java.lang.Appendable",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "java.lang.CharSequence",
            "int",
            "java.lang.CharSequence",
            "kotlin.jvm.functions.Function1"
          )
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.joinTo", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse("java.lang.Appendable"),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableFoldFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("java.lang.Iterable", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableFoldFunction3Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq("java.lang.Iterable", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListFoldFunction2Names.contains(collectionFunction) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(
            listOrIterableReceiverTypeForSignature(iterableOwner),
            TypeConstants.JavaLangObject,
            "kotlin.jvm.functions.Function2"
          )
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListFoldFunction3Names.contains(collectionFunction) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(
            listOrIterableReceiverTypeForSignature(iterableOwner),
            TypeConstants.JavaLangObject,
            "kotlin.jvm.functions.Function3"
          )
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            argumentTypeFullNames.headOption.getOrElse(TypeConstants.JavaLangObject),
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableReduceFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.lang.Iterable", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableReduceFunction3Names.contains(collectionFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.lang.Iterable", "kotlin.jvm.functions.Function3"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListReduceFunction2Names.contains(collectionFunction) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "kotlin.jvm.functions.Function2")
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListReduceFunction3Names.contains(collectionFunction) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "kotlin.jvm.functions.Function3")
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, "sum", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("int", Seq("java.lang.Iterable"))
        Some(MethodInfo(methodFullName("kotlin.collections.sum", signature), signature, "int", isExtension = true))
      case (iterableOwner, "average", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("double", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(methodFullName("kotlin.collections.average", signature), signature, "double", isExtension = true)
        )
      case (iterableOwner, collectionFunction, 0)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableComparableElementNames.contains(collectionFunction) =>
        val signature = methodSignature("java.lang.Comparable", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.lang.Comparable",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableFunction1SelectorElementNames.contains(collectionFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableComparatorElementNames.contains(collectionFunction) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("java.lang.Iterable", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableLambdaComparableReturnNames.contains(collectionFunction) =>
        val signature =
          methodSignature("java.lang.Comparable", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.lang.Comparable",
            isExtension = true
          )
        )
      case (iterableOwner, "sumOf", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("int", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(MethodInfo(methodFullName("kotlin.collections.sumOf", signature), signature, "int", isExtension = true))
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableLambdaObjectReturnNames.contains(collectionFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 0)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterablePlainToListNames.contains(collectionFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 0)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListPlainToListNames.contains(collectionFunction) =>
        val signature = methodSignature("java.util.List", Seq(listOrIterableReceiverTypeForSignature(iterableOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "sortedWith", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "java.util.Comparator"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.sortedWith", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableIntToListNames.contains(collectionFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "int"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "chunked", 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature("java.util.List", Seq("java.lang.Iterable", "int", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.chunked", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "chunked", 1)
          if IterableTypeFullNames.contains(iterableOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "int"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.chunked", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "windowed", explicitArgCount)
          if IterableTypeFullNames.contains(iterableOwner) &&
            explicitArgCount >= 4 &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq("java.lang.Iterable", "int", "int", "boolean", "kotlin.jvm.functions.Function1")
          )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.windowed", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "windowed", explicitArgCount)
          if IterableTypeFullNames.contains(iterableOwner) &&
            explicitArgCount >= 1 &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "int", "int", "boolean"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.windowed", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListIntToListNames.contains(collectionFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature("java.util.List", Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "int"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableFunction1ToListNames.contains(collectionFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListFunction1ToListNames.contains(collectionFunction) =>
        val signature = methodSignature(
          "java.util.List",
          Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "kotlin.jvm.functions.Function1")
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 0)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableElementPlainNames.contains(collectionFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq(listOrIterableReceiverTypeForSignature(iterableOwner)))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableElementFunction1Names.contains(collectionFunction) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            ListElementFunction1Names.contains(collectionFunction) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "kotlin.jvm.functions.Function1")
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableElementAtNames.contains(collectionFunction) &&
            isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(
            TypeConstants.JavaLangObject,
            Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "int")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (listOwner, "getOrNull", 1)
          if ListInterfaceTypeFullNames.contains(listOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature(TypeConstants.JavaLangObject, Seq("java.util.List", "int"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.getOrNull", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, "elementAtOrElse", 2)
          if IterableTypeFullNames.contains(iterableOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "int", "kotlin.jvm.functions.Function1")
        )
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.elementAtOrElse", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (listOwner, "getOrElse", 2)
          if ListInterfaceTypeFullNames.contains(listOwner) && isIntArgument(argumentTypeFullNames, 0) =>
        val signature =
          methodSignature(TypeConstants.JavaLangObject, Seq("java.util.List", "int", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.getOrElse", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableIndexFunction1Names.contains(collectionFunction) =>
        val signature = methodSignature(
          "int",
          Seq(listOrIterableReceiverTypeForSignature(iterableOwner), "kotlin.jvm.functions.Function1")
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "int",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToListFunction1Names.contains(collectionFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "filterNotNull", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.filterNotNull", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableToListFunction2Names.contains(collectionFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "zipWithNext", 0) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.zipWithNext", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "zipWithNext", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.zipWithNext", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableRunningFoldFunction2Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq("java.lang.Iterable", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function2")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 2)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableRunningFoldFunction3Names.contains(collectionFunction) =>
        val signature =
          methodSignature(
            "java.util.List",
            Seq("java.lang.Iterable", TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function3")
          )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableRunningReduceFunction2Names.contains(collectionFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, collectionFunction, 1)
          if IterableTypeFullNames.contains(iterableOwner) &&
            IterableRunningReduceFunction3Names.contains(collectionFunction) =>
        val signature = methodSignature("java.util.List", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function3"))
        Some(
          MethodInfo(
            methodFullName(s"kotlin.collections.$collectionFunction", signature),
            signature,
            "java.util.List",
            isExtension = true
          )
        )
      case (iterableOwner, "onEach", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature =
          methodSignature("java.lang.Iterable", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.onEach", signature),
            signature,
            iterableOwner,
            isExtension = true
          )
        )
      case (iterableOwner, "onEachIndexed", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature =
          methodSignature("java.lang.Iterable", Seq("java.lang.Iterable", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.onEachIndexed", signature),
            signature,
            iterableOwner,
            isExtension = true
          )
        )
      case (iterableOwner, "forEach", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq("java.lang.Iterable", "kotlin.jvm.functions.Function1"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.forEach", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (iterableOwner, "forEachIndexed", 1) if IterableTypeFullNames.contains(iterableOwner) =>
        val signature = methodSignature(TypeConstants.Void, Seq("java.lang.Iterable", "kotlin.jvm.functions.Function2"))
        Some(
          MethodInfo(
            methodFullName("kotlin.collections.forEachIndexed", signature),
            signature,
            TypeConstants.Void,
            isExtension = true
          )
        )
      case (_, scopeFunction @ ("also" | "apply" | "takeIf" | "takeUnless"), 1) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function1")
        )
        Some(MethodInfo(methodFullName(s"kotlin.$scopeFunction", signature), signature, owner, isExtension = true))
      case (_, scopeFunction @ ("let" | "run"), 1) =>
        val signature = methodSignature(
          TypeConstants.JavaLangObject,
          Seq(TypeConstants.JavaLangObject, "kotlin.jvm.functions.Function1")
        )
        Some(
          MethodInfo(
            methodFullName(s"kotlin.$scopeFunction", signature),
            signature,
            TypeConstants.JavaLangObject,
            isExtension = true
          )
        )
      case _ =>
        None
    }
  }

  private def collectionReceiverTypeForSignature(owner: String): String =
    if (CollectionInterfaceTypeFullNames.contains(owner)) "java.util.Collection" else "java.lang.Iterable"

  private def listOrIterableReceiverTypeForSignature(owner: String): String =
    if (owner == "java.util.List") "java.util.List" else "java.lang.Iterable"

  private def mapPlusArgumentType(argumentTypeFullNames: Seq[String]): String =
    argumentTypeFullNames.headOption match {
      case Some("kotlin.Pair")                                  => "kotlin.Pair"
      case Some(argumentType) if MapTypeFullNames(argumentType) => "java.util.Map"
      case Some(argumentType) if IterableTypeFullNames(argumentType) =>
        "java.lang.Iterable"
      case Some(argumentType) if SequenceTypeFullNames(argumentType) =>
        "kotlin.sequences.Sequence"
      case Some(argumentType) if argumentType.endsWith("[]") => argumentType
      case _                                                 => TypeConstants.JavaLangObject
    }

  private def mapMinusArgumentType(argumentTypeFullNames: Seq[String]): String =
    argumentTypeFullNames.headOption match {
      case Some(argumentType) if IterableTypeFullNames(argumentType) =>
        "java.lang.Iterable"
      case Some(argumentType) if SequenceTypeFullNames(argumentType) =>
        "kotlin.sequences.Sequence"
      case Some(argumentType) if argumentType.endsWith("[]") => argumentType
      case _                                                 => TypeConstants.JavaLangObject
    }

  private def collectionPlusMinusArgumentType(argumentTypeFullNames: Seq[String]): String =
    argumentTypeFullNames.headOption match {
      case Some(argumentType) if IterableTypeFullNames(argumentType) =>
        "java.lang.Iterable"
      case Some(argumentType) if SequenceTypeFullNames(argumentType) =>
        "kotlin.sequences.Sequence"
      case Some(argumentType) if argumentType.endsWith("[]") => argumentType
      case _                                                 => TypeConstants.JavaLangObject
    }

  private def sequencePlusMinusArgumentType(argumentTypeFullNames: Seq[String]): String =
    argumentTypeFullNames.headOption match {
      case Some(argumentType) if IterableTypeFullNames(argumentType) =>
        "java.lang.Iterable"
      case Some(argumentType) if SequenceTypeFullNames(argumentType) =>
        "kotlin.sequences.Sequence"
      case Some(argumentType) if isArrayTypeFullName(argumentType) =>
        arrayReceiverTypeForMemberSignature(argumentType)
      case _ => TypeConstants.JavaLangObject
    }

  private def isStringArgument(argumentTypeFullNames: Seq[String], index: Int): Boolean =
    argumentTypeFullNames.lift(index).contains("java.lang.String")

  private def stringSearchNeedleType(argumentTypeFullNames: Seq[String]): Option[String] =
    argumentTypeFullNames.headOption
      .collect {
        case "java.lang.String" => "java.lang.String"
        case "char"             => "char"
      }
      .filter(_ =>
        argumentTypeFullNames.size match {
          case 1 => true
          case 2 => isIntArgument(argumentTypeFullNames, 1)
          case 3 => isIntArgument(argumentTypeFullNames, 1) && isBooleanArgument(argumentTypeFullNames, 2)
          case _ => false
        }
      )

  private def isCharSequenceArgument(argumentTypeFullNames: Seq[String], index: Int): Boolean =
    argumentTypeFullNames.lift(index).exists(CharSequenceTypeFullNames.contains)

  private def isBooleanArgument(argumentTypeFullNames: Seq[String], index: Int): Boolean =
    argumentTypeFullNames.lift(index).contains("boolean")

  private def isIntArgument(argumentTypeFullNames: Seq[String], index: Int): Boolean =
    argumentTypeFullNames.lift(index).contains("int")

  private def isCharArgument(argumentTypeFullNames: Seq[String], index: Int): Boolean =
    argumentTypeFullNames.lift(index).contains("char")

  private def isArrayArgument(argumentTypeFullNames: Seq[String], index: Int): Boolean =
    argumentTypeFullNames.lift(index).exists(isArrayTypeFullName)

  private def isArrayTypeFullName(typeFullName: String): Boolean =
    typeFullName.endsWith("[]")

  private def isPrimitiveArrayTypeFullName(typeFullName: String): Boolean =
    PrimitiveArrayOwnerTypeFullNames.contains(typeFullName)

  private def arrayMemberOwnerTypeFullName(arrayTypeFullName: String): String =
    PrimitiveArrayOwnerTypeFullNames.getOrElse(arrayTypeFullName, "kotlin.Array")

  private def arrayReceiverTypeForMemberSignature(arrayTypeFullName: String): String =
    if (isPrimitiveArrayTypeFullName(arrayTypeFullName)) arrayTypeFullName
    else TypeConstants.JavaLangObject + "[]"

  private def arrayReturnTypeForSignature(arrayTypeFullName: String): String =
    arrayReceiverTypeForMemberSignature(arrayTypeFullName)

  private def arrayContentPeerArgumentType(
    arrayTypeFullName: String,
    argumentTypeFullNames: Seq[String]
  ): Option[String] =
    argumentTypeFullNames.headOption.collect {
      case argumentType if isPrimitiveArrayTypeFullName(arrayTypeFullName) && argumentType == arrayTypeFullName =>
        arrayTypeFullName
      case argumentType if !isPrimitiveArrayTypeFullName(arrayTypeFullName) && isArrayTypeFullName(argumentType) =>
        TypeConstants.JavaLangObject + "[]"
    }

  private def arrayPlusArgumentType(arrayTypeFullName: String, argumentTypeFullNames: Seq[String]): Option[String] =
    argumentTypeFullNames.headOption.collect {
      case argumentType if isPrimitiveArrayTypeFullName(arrayTypeFullName) && argumentType == arrayTypeFullName =>
        arrayTypeFullName
      case argumentType
          if isPrimitiveArrayTypeFullName(arrayTypeFullName) && argumentType == arrayElementTypeForMemberSignature(
            arrayTypeFullName
          ) =>
        arrayElementTypeForMemberSignature(arrayTypeFullName)
      case argumentType if !isPrimitiveArrayTypeFullName(arrayTypeFullName) && isArrayTypeFullName(argumentType) =>
        TypeConstants.JavaLangObject + "[]"
      case _ if !isPrimitiveArrayTypeFullName(arrayTypeFullName) =>
        TypeConstants.JavaLangObject
    }

  private def arraySliceArgumentType(argumentTypeFullNames: Seq[String]): Option[String] =
    argumentTypeFullNames.headOption.collect {
      case IntRangeTypeFullName                                           => IntRangeTypeFullName
      case argumentType if CollectionInterfaceTypeFullNames(argumentType) => "java.util.Collection"
    }

  private def iterableSliceArgumentType(argumentTypeFullNames: Seq[String]): Option[String] =
    argumentTypeFullNames.headOption.collect {
      case IntRangeTypeFullName                                         => IntRangeTypeFullName
      case argumentType if IterableTypeFullNames.contains(argumentType) => "java.lang.Iterable"
    }

  private def arrayReceiverTypePreservingDimensionsForSignature(arrayTypeFullName: String): String =
    if (isPrimitiveArrayTypeFullName(arrayTypeFullName)) {
      arrayTypeFullName
    } else {
      TypeConstants.JavaLangObject + List.fill(math.max(1, arrayDimensionCount(arrayTypeFullName)))("[]").mkString
    }

  private def arrayPairReceiverTypeForSignature(arrayTypeFullName: String): String =
    if (arrayTypeFullName == s"$PairTypeFullName[]") arrayTypeFullName
    else arrayReceiverTypeForMemberSignature(arrayTypeFullName)

  private def arrayZipArgumentType(argumentTypeFullNames: Seq[String]): String =
    argumentTypeFullNames.headOption match {
      case Some(argumentType) if IterableTypeFullNames(argumentType) => "java.lang.Iterable"
      case Some(argumentType) if isArrayTypeFullName(argumentType) => arrayReceiverTypeForMemberSignature(argumentType)
      case _                                                       => "java.lang.Iterable"
    }

  private def arrayDimensionCount(arrayTypeFullName: String): Int =
    arrayTypeFullName.sliding(2).count(_ == "[]")

  private def arrayComparableReceiverTypeForSignature(arrayTypeFullName: String): String =
    if (isPrimitiveArrayTypeFullName(arrayTypeFullName)) arrayTypeFullName
    else "java.lang.Comparable[]"

  private def arrayComparableReturnTypeForSignature(arrayTypeFullName: String): String =
    if (isPrimitiveArrayTypeFullName(arrayTypeFullName)) arrayTypeFullName.stripSuffix("[]")
    else "java.lang.Comparable"

  private def arrayReceiverTypeForPlainToListSignature(functionName: String, arrayTypeFullName: String): String =
    if (functionName == "flatten") {
      arrayReceiverTypePreservingDimensionsForSignature(arrayTypeFullName)
    } else if (
      !isPrimitiveArrayTypeFullName(arrayTypeFullName) && ArrayComparablePlainToListNames.contains(functionName)
    ) {
      "java.lang.Comparable[]"
    } else {
      arrayReceiverTypeForMemberSignature(arrayTypeFullName)
    }

  private def arrayElementTypeForMemberSignature(arrayTypeFullName: String): String =
    if (isPrimitiveArrayTypeFullName(arrayTypeFullName)) {
      arrayTypeFullName.stripSuffix("[]")
    } else {
      TypeConstants.JavaLangObject
    }

  private def arrayIteratorTypeFullName(arrayTypeFullName: String): String =
    arrayTypeFullName match {
      case "boolean[]" => "kotlin.collections.BooleanIterator"
      case "byte[]"    => "kotlin.collections.ByteIterator"
      case "char[]"    => "kotlin.collections.CharIterator"
      case "double[]"  => "kotlin.collections.DoubleIterator"
      case "float[]"   => "kotlin.collections.FloatIterator"
      case "int[]"     => "kotlin.collections.IntIterator"
      case "long[]"    => "kotlin.collections.LongIterator"
      case "short[]"   => "kotlin.collections.ShortIterator"
      case _           => "java.util.Iterator"
    }

  private def constructorMethodInfo(classFullName: String, params: List[ParameterInfo]): MethodInfo = {
    val signature = methodSignature(TypeConstants.Void, params.map(_.typeFullName))
    MethodInfo(
      methodFullName(s"$classFullName.${Defines.ConstructorMethodName}", signature),
      signature,
      TypeConstants.Void
    )
  }

  private def namespaceAstParentFullName(packageName: Option[String]): String =
    packageName.map(pkg => s"${document.relativeName}:$pkg").getOrElse(NamespaceTraversal.globalNamespaceName)

  private def registerType(typeName: String): String = {
    usedTypeNames.add(typeName)
    typeName
  }

  private def annotationNodesFor(node: KotlinAstNode): List[KotlinAstNode] =
    (Option.when(node.kind == "modifiers" || node.kind == "parameter_modifiers")(node).toList ++
      node.children.filter(child => child.kind == "modifiers" || child.kind == "parameter_modifiers"))
      .flatMap(_.children.filter(child => child.kind == "annotation" && child.code.trim.startsWith("@")))

  private def directAnnotationNodesFor(node: KotlinAstNode): List[KotlinAstNode] =
    node.children.filter(child => child.kind == "annotation" && child.code.trim.startsWith("@"))

  private def astForAnnotationEntry(annotation: KotlinAstNode): Ast = {
    val name          = annotationName(annotation)
    val localFullName = sourcePackageName.map(pkg => s"$pkg.$name").filter(usedTypeNames.contains)
    val fullName = registerType(
      importAliases.getOrElse(name, localFullName.getOrElse(s"${Defines.UnresolvedNamespace}.$name"))
    )
    val annotationNode_ = annotationNode(annotation, annotation.code, name, fullName)
    val literals = annotation.children
      .find(_.kind == "constructor_invocation")
      .toList
      .flatMap(valueArgumentNodes)
      .flatMap(_.children.find(_.named))
      .map(value => Ast(annotationLiteralNode(value, value.code)))
    annotationAst(annotationNode_, literals)
  }

  private def annotationName(annotation: KotlinAstNode): String =
    annotation.children
      .find(_.kind == "constructor_invocation")
      .flatMap(_.descendants.find(_.kind == "type_identifier").map(_.code))
      .orElse(annotation.descendants.find(_.kind == "type_identifier").map(_.code))
      .getOrElse(annotation.code.stripPrefix("@").takeWhile(_ != '(').trim)

  private def importParts(importHeader: KotlinAstNode): ImportParts = {
    val rawImport = importHeader.code.stripPrefix("import").trim
    val (entityWithWildcard, alias) = rawImport.split("\\s+as\\s+", 2).toList match {
      case entity :: importedAs :: Nil => (entity.trim, Some(importedAs.trim))
      case entity :: Nil               => (entity.trim, None)
      case _                           => (rawImport, None)
    }
    val isWildcard     = entityWithWildcard.endsWith(".*")
    val importedEntity = if (isWildcard) entityWithWildcard.stripSuffix(".*") else entityWithWildcard
    val importedAs =
      if (isWildcard) "*" else alias.getOrElse(importedEntity.split('.').lastOption.getOrElse(importedEntity))
    ImportParts(importedEntity, importedAs, isWildcard)
  }

  private case class ImportParts(importedEntity: String, importedAs: String, isWildcard: Boolean)
  private case class CompanionInfo(name: String, fullName: String)
  private case class GlobalInfo(name: String, typeFullName: String, local: NewLocal)
  private case class BoundCallableReferenceInfo(
    methodInfo: MethodInfo,
    receiverAst: Ast,
    receiverCode: String,
    receiverTypeFullName: String,
    isStatic: Boolean,
    functionTypeFullName: String
  )
  private case class MethodInfo(
    fullName: String,
    signature: String,
    returnTypeFullName: String,
    isPrivate: Boolean = false,
    isExtension: Boolean = false,
    isStatic: Boolean = false
  )
  private case class TypeInfo(
    name: String,
    fullName: String,
    typeParameterNames: List[String],
    typeParameterBounds: Map[String, String],
    methods: List[MethodDeclInfo],
    members: Map[String, String]
  )
  private case class MethodDeclInfo(name: String, signature: String, parameterCount: Int, isPrivate: Boolean)
  private case class BodyContext(
    types: mutable.Map[String, String],
    refs: mutable.Map[String, NewNode],
    ownerMethodFullName: String,
    methods: mutable.Map[(String, Int), MethodInfo] = mutable.Map.empty,
    collectionElementTypes: mutable.Map[String, String] = mutable.Map.empty,
    iteratorElementTypes: mutable.Map[String, String] = mutable.Map.empty,
    mapKeyTypes: mutable.Map[String, String] = mutable.Map.empty,
    mapValueTypes: mutable.Map[String, String] = mutable.Map.empty,
    mapEntryKeyTypes: mutable.Map[String, String] = mutable.Map.empty,
    mapEntryValueTypes: mutable.Map[String, String] = mutable.Map.empty,
    expectedLambdaElementType: Option[String] = None,
    expectedLambdaReturnType: Option[String] = None,
    expectedLambdaMapEntryKeyType: Option[String] = None,
    expectedLambdaMapEntryValueType: Option[String] = None,
    pairFirstTypes: mutable.Map[String, String] = mutable.Map.empty,
    pairSecondTypes: mutable.Map[String, String] = mutable.Map.empty,
    tripleFirstTypes: mutable.Map[String, String] = mutable.Map.empty,
    tripleSecondTypes: mutable.Map[String, String] = mutable.Map.empty,
    tripleThirdTypes: mutable.Map[String, String] = mutable.Map.empty
  )
  private case class DestructuringEntry(node: KotlinAstNode, name: String, originalIndex: Int, typeFullName: String)
  private case class DestructuringBase(name: String, typeFullName: String, prologueAsts: List[Ast])
  private case class MemberDeclInfo(node: KotlinAstNode, name: String, typeFullName: String)
  private case class ParameterInfo(
    node: KotlinAstNode,
    name: String,
    typeFullName: String,
    code: String,
    declaresMember: Boolean,
    annotations: List[KotlinAstNode] = Nil,
    collectionElementTypeFullName: Option[String] = None,
    mapKeyTypeFullName: Option[String] = None,
    mapValueTypeFullName: Option[String] = None,
    mapEntryKeyTypeFullName: Option[String] = None,
    mapEntryValueTypeFullName: Option[String] = None,
    pairFirstTypeFullName: Option[String] = None,
    pairSecondTypeFullName: Option[String] = None,
    tripleFirstTypeFullName: Option[String] = None,
    tripleSecondTypeFullName: Option[String] = None,
    tripleThirdTypeFullName: Option[String] = None
  )

  private val TypeNodeKinds: Set[String] =
    Set("user_type", "nullable_type", "type_identifier", "function_type", "parenthesized_type")
  private val IntegerLiteralTypeFullNames: Set[String] = Set("byte", "short", "int", "long")

  private val PrintPrimitiveTypeFullNames: Set[String] =
    Set("boolean", "byte", "char", "double", "float", "int", "long", "short")

  private val MathMaxTypeFullNames: Set[String] =
    Set("double", "float", "int", "long")

  private val PrimitiveArrayFactoryReturnTypes: Map[String, String] =
    Map(
      "booleanArrayOf" -> "boolean[]",
      "byteArrayOf"    -> "byte[]",
      "charArrayOf"    -> "char[]",
      "doubleArrayOf"  -> "double[]",
      "floatArrayOf"   -> "float[]",
      "intArrayOf"     -> "int[]",
      "longArrayOf"    -> "long[]",
      "shortArrayOf"   -> "short[]"
    )

  private val BuiltinCollectionFactoryNames: Set[String] =
    Set(
      "arrayOf",
      "arrayOfNulls",
      "arrayListOf",
      "booleanArrayOf",
      "byteArrayOf",
      "charArrayOf",
      "doubleArrayOf",
      "emptyList",
      "emptyMap",
      "emptySet",
      "emptyArray",
      "floatArrayOf",
      "hashSetOf",
      "intArrayOf",
      "linkedSetOf",
      "listOf",
      "listOfNotNull",
      "longArrayOf",
      "mapOf",
      "mutableListOf",
      "mutableMapOf",
      "mutableSetOf",
      "shortArrayOf",
      "setOf",
      "setOfNotNull"
    )

  private val BuiltinArrayFactoryNames: Set[String] =
    Set("arrayOf", "arrayOfNulls", "emptyArray") ++ PrimitiveArrayFactoryReturnTypes.keySet

  private val BuiltinIterableFactoryNames: Set[String] =
    Set(
      "arrayListOf",
      "emptyList",
      "emptySet",
      "hashSetOf",
      "linkedSetOf",
      "listOf",
      "listOfNotNull",
      "mutableListOf",
      "mutableSetOf",
      "setOf",
      "setOfNotNull"
    )

  private val BuiltinMapValueTypeArgumentNames: Set[String] =
    Set("HashMap", "emptyMap", "mapOf", "mutableMapOf")

  private val TypeArgumentNonReturnCallNames: Set[String] =
    Set("filterIsInstance")

  private val StdlibExternalConstructorPackagePrefixes: Seq[String] =
    Seq("java.", "kotlin.")

  private val IterableTypeFullNames: Set[String] =
    Set(
      "java.lang.Iterable",
      "java.util.ArrayList",
      "java.util.Collection",
      "java.util.HashSet",
      "java.util.LinkedHashSet",
      "java.util.List",
      "java.util.Set"
    )

  private val SequenceTypeFullNames: Set[String] =
    Set("kotlin.sequences.Sequence")

  private val CollectionInterfaceTypeFullNames: Set[String] =
    Set(
      "java.util.ArrayList",
      "java.util.Collection",
      "java.util.HashSet",
      "java.util.LinkedHashSet",
      "java.util.List",
      "java.util.Set"
    )

  private val CollectionMemberReturnTypes: Map[String, String] =
    Map("size" -> "int")

  private val IntRangeTypeFullName: String        = "kotlin.ranges.IntRange"
  private val LongRangeTypeFullName: String       = "kotlin.ranges.LongRange"
  private val CharRangeTypeFullName: String       = "kotlin.ranges.CharRange"
  private val IntProgressionTypeFullName: String  = "kotlin.ranges.IntProgression"
  private val LongProgressionTypeFullName: String = "kotlin.ranges.LongProgression"
  private val CharProgressionTypeFullName: String = "kotlin.ranges.CharProgression"

  private val RangeUntilReturnTypes: Map[String, String] =
    Map("int" -> IntRangeTypeFullName, "long" -> LongRangeTypeFullName)

  private val RangeDownToReturnTypes: Map[String, String] =
    Map(
      "int"  -> IntProgressionTypeFullName,
      "long" -> LongProgressionTypeFullName,
      "char" -> CharProgressionTypeFullName
    )

  private val ListInterfaceTypeFullNames: Set[String] =
    Set("java.util.List", "kotlin.collections.List", "kotlin.collections.MutableList")

  private val ListOperatorTypeFullNames: Set[String] =
    ListInterfaceTypeFullNames ++ Set("java.util.ArrayList")

  private val ListMemberReturnTypes: Map[String, String] =
    Map("indices" -> IntRangeTypeFullName, "lastIndex" -> "int")

  private val ArrayMemberReturnTypes: Map[String, String] =
    Map("size" -> "int", "indices" -> IntRangeTypeFullName, "lastIndex" -> "int")

  private val ArrayIndexElementNames: Set[String] =
    Set("indexOf", "lastIndexOf")

  private val ArrayEmptyPredicateNames: Set[String] =
    Set("isEmpty", "isNotEmpty")

  private val ArrayComparablePlainToListNames: Set[String] =
    Set("sorted", "sortedDescending")

  private val ListIndexElementNames: Set[String] =
    Set("indexOf", "lastIndexOf")

  private val ListMemberElementReturnNames: Set[String] =
    Set("get", "removeAt", "set")

  private val MutableListCollectionMutationNames: Set[String] =
    Set("removeAll", "retainAll")

  private val SetInterfaceTypeFullNames: Set[String] =
    Set("java.util.Set", "kotlin.collections.Set", "kotlin.collections.MutableSet")

  private val SetOperatorTypeFullNames: Set[String] =
    SetInterfaceTypeFullNames ++ Set("java.util.HashSet", "java.util.LinkedHashSet")

  private val MutableSetElementMutationNames: Set[String] =
    Set("add", "remove")

  private val MutableSetCollectionMutationNames: Set[String] =
    Set("addAll", "removeAll", "retainAll")

  private val MapTypeFullNames: Set[String] =
    Set("java.util.Map", "java.util.HashMap", "kotlin.collections.Map", "kotlin.collections.MutableMap")

  private val MapInterfaceTypeFullNames: Set[String] =
    Set("java.util.Map", "kotlin.collections.Map", "kotlin.collections.MutableMap")

  private val MapTypeNames: Set[String] =
    Set("Map", "MutableMap", "HashMap", "java.util.Map", "java.util.HashMap")

  private val MapEntryTypeFullName: String = "java.util.Map$Entry"

  private val MapEntryTypeNames: Set[String] =
    Set("Entry", "Map.Entry", "MutableMap.MutableEntry", "java.util.Map.Entry", "java.util.Map$Entry")

  private val MapEntryComponentNames: Set[String] =
    Set("component1", "component2")

  private val PairTypeFullName: String = "kotlin.Pair"

  private val PairTypeNames: Set[String] =
    Set("Pair", PairTypeFullName)

  private val PairComponentNames: Set[String] =
    Set("component1", "component2")

  private val TripleTypeFullName: String = "kotlin.Triple"

  private val TripleTypeNames: Set[String] =
    Set("Triple", TripleTypeFullName)

  private val TripleComponentNames: Set[String] =
    Set("component1", "component2", "component3")

  private val MapMemberReturnTypes: Map[String, String] =
    Map("entries" -> "java.util.Set", "keys" -> "java.util.Set", "size" -> "int", "values" -> "java.util.Collection")

  private val MapMemberPredicateNames: Set[String] =
    Set("containsKey", "containsValue")

  private val MapValueReturnNames: Set[String] =
    Set("get", "getOrDefault", "getOrElse", "getOrPut", "getValue", "put", "remove")

  private val MapDefaultingFunctionNames: Set[String] =
    Set("getOrElse", "getOrPut")

  private val MapFunction1ToMapNames: Set[String] =
    Set("filter", "filterKeys", "filterValues", "mapKeys", "mapValues")

  private val MapConversionReturnTypes: Map[String, String] =
    Map(
      "toHashSet"    -> "java.util.HashSet",
      "toMutableSet" -> "java.util.Set",
      "toSet"        -> "java.util.Set",
      "withIndex"    -> "java.lang.Iterable"
    )

  private val MapToCollectionDestinationFunction1Names: Set[String] =
    Set("flatMapTo", "mapTo")

  private val MapPlainBooleanNames: Set[String] =
    Set("any", "none")

  private val MapPredicateBooleanNames: Set[String] =
    Set("all", "any", "none")

  private val IterablePlainBooleanNames: Set[String] =
    Set("any", "none")

  private val IterablePredicateBooleanNames: Set[String] =
    Set("all", "any", "none")

  private val IterableFoldFunction2Names: Set[String] =
    Set("fold")

  private val IterableFoldFunction3Names: Set[String] =
    Set("foldIndexed")

  private val ListFoldFunction2Names: Set[String] =
    Set("foldRight")

  private val ListFoldFunction3Names: Set[String] =
    Set("foldRightIndexed")

  private val IterableReduceFunction2Names: Set[String] =
    Set("reduce", "reduceOrNull")

  private val IterableReduceFunction3Names: Set[String] =
    Set("reduceIndexed", "reduceIndexedOrNull")

  private val ListReduceFunction2Names: Set[String] =
    Set("reduceRight", "reduceRightOrNull")

  private val ListReduceFunction3Names: Set[String] =
    Set("reduceRightIndexed", "reduceRightIndexedOrNull")

  private val ArrayReduceFunction2Names: Set[String] =
    IterableReduceFunction2Names ++ ListReduceFunction2Names

  private val ArrayReduceFunction3Names: Set[String] =
    IterableReduceFunction3Names ++ ListReduceFunction3Names

  private val IterableRunningFoldFunction2Names: Set[String] =
    Set("runningFold", "scan")

  private val IterableRunningFoldFunction3Names: Set[String] =
    Set("runningFoldIndexed", "scanIndexed")

  private val IterableRunningReduceFunction2Names: Set[String] =
    Set("runningReduce")

  private val IterableRunningReduceFunction3Names: Set[String] =
    Set("runningReduceIndexed")

  private val IterableComparableElementNames: Set[String] =
    Set("maxOrNull", "minOrNull")

  private val IterableFunction1SelectorElementNames: Set[String] =
    Set("maxBy", "maxByOrNull", "minBy", "minByOrNull")

  private val IterableComparatorElementNames: Set[String] =
    Set("maxWith", "maxWithOrNull", "minWith", "minWithOrNull")

  private val IterableLambdaComparableReturnNames: Set[String] =
    Set("maxOf", "maxOfOrNull", "minOf", "minOfOrNull")

  private val IterableLambdaObjectReturnNames: Set[String] =
    Set("firstNotNullOf", "firstNotNullOfOrNull")

  private val IterableLambdaResultReturnNames: Set[String] =
    IterableLambdaComparableReturnNames ++ IterableLambdaObjectReturnNames + "sumOf"

  private val IterableConversionReturnTypes: Map[String, String] =
    Map(
      "asIterable"   -> "java.lang.Iterable",
      "asSequence"   -> "kotlin.sequences.Sequence",
      "toHashSet"    -> "java.util.HashSet",
      "toList"       -> "java.util.List",
      "toMutableSet" -> "java.util.Set",
      "toSet"        -> "java.util.Set",
      "withIndex"    -> "java.lang.Iterable"
    )

  private val IterablePlainToListNames: Set[String] =
    Set("distinct", "filterIsInstance", "flatten", "reversed", "shuffled", "sorted", "sortedDescending")

  private val ListPlainToListNames: Set[String] =
    Set("asReversed")

  private val IterableIntToListNames: Set[String] =
    Set("drop", "take")

  private val ListIntToListNames: Set[String] =
    Set("dropLast", "takeLast")

  private val IterableFunction1ToListNames: Set[String] =
    Set("distinctBy", "dropWhile", "sortedBy", "sortedByDescending", "takeWhile")

  private val ListFunction1ToListNames: Set[String] =
    Set("dropLastWhile", "takeLastWhile")

  private val IterableElementPlainNames: Set[String] =
    Set("first", "firstOrNull", "last", "lastOrNull", "single", "singleOrNull")

  private val IterableElementFunction1Names: Set[String] =
    Set("find", "first", "firstOrNull", "single", "singleOrNull")

  private val ListElementFunction1Names: Set[String] =
    Set("findLast", "last", "lastOrNull")

  private val IterableElementAtNames: Set[String] =
    Set("elementAt", "elementAtOrNull")

  private val IndexedElementDefaultNames: Set[String] =
    Set("getOrElse", "getOrNull")

  private val IterableRandomElementNames: Set[String] =
    Set("random", "randomOrNull")

  private val IterableElementReturnNames: Set[String] =
    IterableElementPlainNames ++ IterableElementFunction1Names ++ ListElementFunction1Names ++ IterableElementAtNames +
      "elementAtOrElse" ++ IndexedElementDefaultNames ++ IterableReduceFunction2Names ++ IterableReduceFunction3Names ++
      ListReduceFunction2Names ++ ListReduceFunction3Names ++ IterableComparableElementNames ++
      IterableFunction1SelectorElementNames ++ IterableComparatorElementNames ++ IterableRandomElementNames

  private lazy val ExtensionElementReturnNames: Set[String] =
    IterableElementReturnNames ++ SequenceElementPlainNames ++ SequenceComparableElementNames ++
      IterableFunction1SelectorElementNames ++ IterableComparatorElementNames

  private val IterableIndexFunction1Names: Set[String] =
    Set("indexOfFirst", "indexOfLast")

  private val IterableToListFunction1Names: Set[String] =
    Set("filter", "filterNot", "flatMap", "map", "mapNotNull")

  private val IterableToListFunction2Names: Set[String] =
    Set("filterIndexed", "flatMapIndexed", "mapIndexed", "mapIndexedNotNull")

  private val IterableToMapFunction1Names: Set[String] =
    Set("associate", "associateBy", "associateWith", "groupBy")

  private val IterableToMapFunction2Names: Set[String] =
    Set("associateBy", "groupBy")

  private val IterableSetOperationNames: Set[String] =
    Set("intersect", "subtract", "union")

  private val PrimitiveCollectionArrayConversionReturnTypes: Map[String, String] =
    Map(
      "toBooleanArray" -> "boolean[]",
      "toByteArray"    -> "byte[]",
      "toCharArray"    -> "char[]",
      "toDoubleArray"  -> "double[]",
      "toFloatArray"   -> "float[]",
      "toIntArray"     -> "int[]",
      "toLongArray"    -> "long[]",
      "toShortArray"   -> "short[]"
    )

  private val IterableToCollectionDestinationFunction1Names: Set[String] =
    Set("filterNotTo", "filterTo", "flatMapTo", "mapNotNullTo", "mapTo")

  private val IterableToCollectionDestinationFunction2Names: Set[String] =
    Set("filterIndexedTo", "flatMapIndexedTo", "mapIndexedNotNullTo", "mapIndexedTo")

  private val IterableToMapDestinationFunction1Names: Set[String] =
    Set("associateByTo", "associateTo", "associateWithTo", "groupByTo")

  private val IterableToMapDestinationFunction2Names: Set[String] =
    Set("associateByTo", "groupByTo")

  private val SequenceFunction1ReturnSequenceNames: Set[String] =
    Set(
      "distinctBy",
      "dropWhile",
      "filter",
      "filterNot",
      "flatMap",
      "map",
      "mapNotNull",
      "onEach",
      "sortedBy",
      "sortedByDescending",
      "takeWhile"
    )

  private val SequenceFunction2ReturnSequenceNames: Set[String] =
    Set("filterIndexed", "flatMapIndexed", "mapIndexed", "mapIndexedNotNull", "onEachIndexed")

  private val SequenceIntReturnSequenceNames: Set[String] =
    Set("drop", "take")

  private val SequenceElementPlainNames: Set[String] =
    Set("first", "firstOrNull", "last", "lastOrNull", "single", "singleOrNull")

  private val SequenceElementFunction1Names: Set[String] =
    Set("find", "first", "firstOrNull", "last", "lastOrNull", "single", "singleOrNull")

  private val SequenceElementAtNames: Set[String] =
    Set("elementAt", "elementAtOrNull")

  private val SequenceIndexFunction1Names: Set[String] =
    Set("indexOfFirst", "indexOfLast")

  private val SequenceComparableElementNames: Set[String] =
    Set("maxOrNull", "minOrNull")

  private val SequencePlainReturnSequenceNames: Set[String] =
    Set(
      "constrainOnce",
      "distinct",
      "filterIsInstance",
      "filterNotNull",
      "flatten",
      "requireNoNulls",
      "sorted",
      "sortedDescending",
      "withIndex"
    )

  private val SequencePlainBooleanNames: Set[String] =
    Set("any", "none")

  private val SequencePredicateBooleanNames: Set[String] =
    Set("all", "any", "none")

  private val SequenceConversionReturnTypes: Map[String, String] =
    Map(
      "asIterable"    -> "java.lang.Iterable",
      "toHashSet"     -> "java.util.HashSet",
      "toList"        -> "java.util.List",
      "toMutableList" -> "java.util.List",
      "toMutableSet"  -> "java.util.Set",
      "toSet"         -> "java.util.Set"
    )

  private val ReceiverElementPreservingCallNames: Set[String] =
    Set("asIterable", "asSequence", "drop", "filter", "filterNot", "onEach", "take")

  private val JavaLangThrowableTypes: Map[String, String] = Map(
    "Throwable"                -> "java.lang.Throwable",
    "Exception"                -> "java.lang.Exception",
    "Error"                    -> "java.lang.Error",
    "RuntimeException"         -> "java.lang.RuntimeException",
    "IllegalArgumentException" -> "java.lang.IllegalArgumentException",
    "IllegalStateException"    -> "java.lang.IllegalStateException",
    "NullPointerException"     -> "java.lang.NullPointerException"
  )

  private val DefaultTypeFullNames: Map[String, String] =
    Map(
      "ArrayList"     -> "java.util.ArrayList",
      "Runtime"       -> "java.lang.Runtime",
      "StringBuilder" -> "java.lang.StringBuilder",
      "HashMap"       -> "java.util.HashMap",
      "HashSet"       -> "java.util.HashSet",
      "LinkedHashSet" -> "java.util.LinkedHashSet",
      "Sequence"      -> "kotlin.sequences.Sequence",
      "UUID"          -> "java.util.UUID"
    )

  private val CollectionTypeNames: Set[String] =
    Set(
      "Iterable",
      "Collection",
      "List",
      "ArrayList",
      "MutableIterable",
      "MutableCollection",
      "MutableList",
      "Set",
      "MutableSet",
      "Map",
      "MutableMap",
      "HashMap",
      "HashSet",
      "LinkedHashSet",
      "Sequence",
      "java.lang.Iterable",
      "java.util.ArrayList",
      "java.util.Collection",
      "java.util.HashSet",
      "java.util.LinkedHashSet",
      "java.util.List",
      "java.util.Set",
      "java.util.Map",
      "java.util.HashMap",
      "kotlin.sequences.Sequence",
      "kotlin.collections.Iterable",
      "kotlin.collections.Collection",
      "kotlin.collections.List",
      "kotlin.collections.MutableIterable",
      "kotlin.collections.MutableCollection",
      "kotlin.collections.MutableList",
      "kotlin.collections.Set",
      "kotlin.collections.MutableSet",
      "kotlin.collections.Map",
      "kotlin.collections.MutableMap"
    )

  private val ComparisonOperatorNames: Set[String] =
    Set(">", ">=", "<", "<=", "==", "!=")

  private val ClassLiteralOperatorName: String = "<operator>.class"
  private val KotlinReflectKClass: String      = "kotlin.reflect.KClass"

  private val PrimitiveKotlinTypeFullNames: Map[String, String] = Map(
    "boolean" -> "kotlin.Boolean",
    "byte"    -> "kotlin.Byte",
    "char"    -> "kotlin.Char",
    "double"  -> "kotlin.Double",
    "float"   -> "kotlin.Float",
    "int"     -> "kotlin.Int",
    "long"    -> "kotlin.Long",
    "short"   -> "kotlin.Short"
  )

  private val NumericPrimitiveTypeFullNames: Seq[String] =
    Seq("double", "float", "long", "int", "short", "byte")

  private val PrimitiveConversionReturnTypes: Map[String, String] = Map(
    "toByte"   -> "byte",
    "toShort"  -> "short",
    "toInt"    -> "int",
    "toLong"   -> "long",
    "toFloat"  -> "float",
    "toDouble" -> "double",
    "toChar"   -> "char"
  )

  private val StringNumericConversionReturnTypes: Map[String, String] = Map(
    "toByte"         -> "byte",
    "toShort"        -> "short",
    "toInt"          -> "int",
    "toLong"         -> "long",
    "toFloat"        -> "float",
    "toDouble"       -> "double",
    "toByteOrNull"   -> "byte",
    "toShortOrNull"  -> "short",
    "toIntOrNull"    -> "int",
    "toLongOrNull"   -> "long",
    "toFloatOrNull"  -> "float",
    "toDoubleOrNull" -> "double"
  )

  private val StringCaseConversionNames: Set[String] =
    Set("lowercase", "toLowerCase", "uppercase", "toUpperCase")

  private val StringPrefixSuffixNames: Set[String] =
    Set("startsWith", "endsWith")

  private val StringSearchNames: Set[String] =
    Set("indexOf", "lastIndexOf")

  private val StringDefaultingExtensionNames: Set[String] =
    Set("ifBlank", "ifEmpty")

  private val StringTrimEndNames: Set[String] =
    Set("trimStart", "trimEnd")

  private val StringPadNames: Set[String] =
    Set("padStart", "padEnd")

  private val StringRemoveAffixNames: Set[String] =
    Set("removePrefix", "removeSuffix")

  private val StringSubstringAroundNames: Set[String] =
    Set("substringBefore", "substringAfter", "substringBeforeLast", "substringAfterLast")

  private val StringTextPredicateExtensionNames: Set[String] =
    Set("isBlank", "isEmpty", "isNotBlank", "isNotEmpty")

  private val CharSequenceTypeFullNames: Set[String] =
    Set("java.lang.CharSequence", "java.lang.String")

  private val BuiltinTypeNames: Map[String, String] = Map(
    "Any"               -> TypeConstants.JavaLangObject,
    "ArrayList"         -> "java.util.ArrayList",
    "Boolean"           -> "boolean",
    "Byte"              -> "byte",
    "Char"              -> "char",
    "CharProgression"   -> CharProgressionTypeFullName,
    "CharRange"         -> CharRangeTypeFullName,
    "CharSequence"      -> "java.lang.CharSequence",
    "Collection"        -> "java.util.Collection",
    "Double"            -> "double",
    "Float"             -> "float",
    "HashSet"           -> "java.util.HashSet",
    "Iterable"          -> "java.lang.Iterable",
    "Int"               -> "int",
    "IntProgression"    -> IntProgressionTypeFullName,
    "IntRange"          -> IntRangeTypeFullName,
    "LinkedHashSet"     -> "java.util.LinkedHashSet",
    "List"              -> "java.util.List",
    "Long"              -> "long",
    "LongProgression"   -> LongProgressionTypeFullName,
    "LongRange"         -> LongRangeTypeFullName,
    "Map"               -> "java.util.Map",
    "MutableCollection" -> "java.util.Collection",
    "MutableIterable"   -> "java.lang.Iterable",
    "MutableList"       -> "java.util.List",
    "MutableMap"        -> "java.util.Map",
    "MutableSet"        -> "java.util.Set",
    "Nothing"           -> TypeConstants.Void,
    "Number"            -> "java.lang.Number",
    "Pair"              -> PairTypeFullName,
    "Set"               -> "java.util.Set",
    "Short"             -> "short",
    "String"            -> "java.lang.String",
    "Triple"            -> TripleTypeFullName,
    "Unit"              -> TypeConstants.Void
  )

  private val PrimitiveArrayTypeNames: Map[String, String] = Map(
    "BooleanArray" -> "boolean[]",
    "ByteArray"    -> "byte[]",
    "CharArray"    -> "char[]",
    "DoubleArray"  -> "double[]",
    "FloatArray"   -> "float[]",
    "IntArray"     -> "int[]",
    "LongArray"    -> "long[]",
    "ShortArray"   -> "short[]"
  )

  private val PrimitiveArrayOwnerTypeFullNames: Map[String, String] =
    PrimitiveArrayTypeNames.map { case (name, typeFullName) => typeFullName -> s"kotlin.$name" }

  private val PrimitiveArraySumReturnTypes: Map[String, String] =
    Map(
      "byte[]"   -> "int",
      "double[]" -> "double",
      "float[]"  -> "float",
      "int[]"    -> "int",
      "long[]"   -> "long",
      "short[]"  -> "int"
    )
}
