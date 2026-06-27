package io.joern.x2cpg.frontendspecific.rubysrc2cpg

import io.joern.x2cpg.Defines as XDefines
import io.joern.x2cpg.frontendspecific.rubysrc2cpg.Constants.*
import io.joern.x2cpg.passes.frontend.*
import io.joern.x2cpg.passes.frontend.XTypeRecovery.AllNodeTypesFromNodeExt
import io.shiftleft.codepropertygraph.generated.nodes.*
import io.shiftleft.codepropertygraph.generated.{Cpg, DiffGraphBuilder, Operators, PropertyNames}
import io.shiftleft.semanticcpg.language.*
import io.shiftleft.semanticcpg.language.operatorextension.OpNodes.{Assignment, FieldAccess}

class RubyTypeRecoveryPassGenerator(cpg: Cpg, config: XTypeRecoveryConfig = XTypeRecoveryConfig())
    extends XTypeRecoveryPassGenerator[File](cpg, config) {
  override protected def generateRecoveryPass(state: XTypeRecoveryState, iteration: Int): XTypeRecovery[File] =
    new RubyTypeRecovery(cpg, state, iteration)
}

private class RubyTypeRecovery(cpg: Cpg, state: XTypeRecoveryState, iteration: Int)
    extends XTypeRecovery[File](cpg, state, iteration) {

  override def compilationUnits: Iterator[File] = cpg.file.iterator

  override def isParallel: Boolean = false

  override def generateRecoveryForCompilationUnitTask(
    unit: File,
    builder: DiffGraphBuilder
  ): RecoverForXCompilationUnit[File] = {
    new RecoverForRubyFile(cpg, unit, builder, state)
  }
}

private class RecoverForRubyFile(cpg: Cpg, cu: File, builder: DiffGraphBuilder, state: XTypeRecoveryState)
    extends RecoverForXCompilationUnit[File](cpg, cu, builder, state) {

  private val EmptyTypeFullName           = "<empty>"
  private val SyntheticReceiverAssignment = "^\\(<tmp-\\d+>\\s*=\\s*([A-Za-z_][\\w:]*?)\\)\\..*".r
  private val RubyClassSuffix             = "<class>"
  private val RubySelf                    = "self"
  private val RubyTypeDeclBody            = "<body>"

  private def isRubyUnknownType(typ: String): Boolean =
    typ.isBlank ||
      typ == EmptyTypeFullName ||
      typ == XDefines.DynamicCallUnknownFullName ||
      typ.startsWith(s"${XDefines.DynamicCallUnknownFullName}$pathSep") ||
      XTypeRecovery.unknownTypePattern.matches(typ)

  private def rubyTypeSeq(types: IterableOnce[String]): Seq[String] =
    types.iterator.filterNot(isRubyUnknownType).toSeq.distinct

  private def rubyTypeSet(types: IterableOnce[String]): Set[String] =
    rubyTypeSeq(types).toSet

  override protected def fromNodeToLocalKey(node: AstNode): Option[LocalKey] = node match {
    case call: Call if call.name == Operators.fieldAccess => Option(LocalVar(s"<fieldAccess:${call.id()}>"))
    case _                                                => super.fromNodeToLocalKey(node)
  }

  /** A heuristic method to determine if a call is a constructor or not.
    */
  override protected def isConstructor(c: Call): Boolean = {
    isConstructor(c.name) && c.code.charAt(0).isUpper
  }

  /** A heuristic method to determine if a call name is a constructor or not.
    */
  override protected def isConstructor(name: String): Boolean =
    !name.isBlank && (name == "new" || name == Initialize)

  override protected def hasTypes(node: AstNode): Boolean = node match {
    case x: Call if !x.methodFullName.startsWith("<operator>") =>
      rubyTypeSet(x.getKnownTypes ++ Seq(x.methodFullName)).nonEmpty
    case x: Call if x.methodFullName.startsWith("<operator>") =>
      x.typeFullName != EmptyTypeFullName && super.hasTypes(node)
    case x =>
      rubyTypeSet(x.getKnownTypes).nonEmpty
  }

  private def importedReceiverCallTypes(call: Call): Set[String] = {
    val receiverNames = call.code match {
      case SyntheticReceiverAssignment(receiver) =>
        Seq(receiver, receiver.split("::").lastOption.getOrElse(receiver)).distinct
      case _ => Seq.empty
    }
    rubyTypeSet(receiverNames.flatMap(receiver => symbolTable.get(CallAlias(call.name, Option(receiver)))))
  }

  override protected def prepopulateSymbolTable(): Unit = {
    super.prepopulateSymbolTable()
    cu.ast.isCall.foreach { call =>
      val importedTypes = importedReceiverCallTypes(call)
      if (importedTypes.nonEmpty) {
        symbolTable.put(call, importedTypes)
      }
    }
  }

  override def prepopulateSymbolTableEntry(x: AstNode): Unit = x match {
    case x @ (_: Identifier | _: Local | _: MethodParameterIn) => symbolTable.append(x, rubyTypeSet(x.getKnownTypes))
    case call: Call =>
      val importedTypes = importedReceiverCallTypes(call)
      if (importedTypes.nonEmpty) {
        symbolTable.put(call, importedTypes)
      } else {
        val tnfs =
          if (
            call.name != "initialize" && (call.methodFullName == XDefines.DynamicCallUnknownFullName || call.methodFullName
              .startsWith("<operator>"))
          ) {
            (call.dynamicTypeHintFullName ++ call.possibleTypes).distinct
          } else {
            (call.methodFullName +: (call.dynamicTypeHintFullName ++ call.possibleTypes)).distinct
          }

        symbolTable.append(call, rubyTypeSet(tnfs))
      }
    case _ =>
  }

  override protected def assignments: Iterator[Assignment] =
    cu.ast.isCall.nameExact(Operators.assignment).cast[Assignment]

  override def visitImport(i: Import): Unit = for {
    resolvedImport <- i.call.tag
    alias          <- i.importedAs
  } {
    import io.shiftleft.semanticcpg.language.importresolver.*
    EvaluatedImport.tagToEvaluatedImport(resolvedImport).foreach {
      case ResolvedTypeDecl(fullName, _) =>
        symbolTable.append(LocalVar(fullName.split("\\.").lastOption.getOrElse(alias)), fullName)
      case _ => super.visitImport(i)
    }
  }

  override def visitIdentifierAssignedToConstructor(i: Identifier, c: Call): Set[String] = {
    associateTypes(i, rubyTypeSet(Set(i.typeFullName)))
  }

  override protected def associateTypes(i: Identifier, types: Set[String]): Set[String] =
    super.associateTypes(i, rubyTypeSet(types))

  override protected def associateTypes(symbol: LocalVar, fa: FieldAccess, types: Set[String]): Set[String] =
    if (persistRubyFieldMemberTypes(fa, rubyTypeSet(types))) {
      symbolTable.append(symbol, rubyTypeSet(types))
    } else {
      super.associateTypes(symbol, fa, rubyTypeSet(types))
    }

  private def rubyPathName(name: String): String =
    if (name == "_") "anonymous" else name

  private lazy val temporaryAssignmentRhs: Map[String, AstNode] = assignments.flatMap { assignment =>
    assignment.argumentOut.l match {
      case (identifier: Identifier) :: rhs :: _ if identifier.name.startsWith("<tmp-") =>
        Option(identifier.name -> rhs)
      case _ => None
    }
  }.toMap

  private def rubyCallMethodTypes(call: Call): Set[String] = {
    val receiverTypes = call.argumentOut
      .collectFirst { case expression: Expression if expression.argumentIndex == 0 => expressionValueTypes(expression) }
      .getOrElse(Set.empty)
    rubyTypeSet(receiverTypes.map(receiverType => s"$receiverType$pathSep${rubyPathName(call.name)}"))
  }

  private def rubyMethodTypesForCall(call: Call): Set[String] = {
    val directTypes = rubyTypeSet(symbolTable.get(call) ++ call.getKnownTypes ++ Seq(call.methodFullName))
    if (directTypes.nonEmpty) directTypes else rubyCallMethodTypes(call)
  }

  private def expressionValueTypes(node: AstNode, seenTemporaries: Set[String] = Set.empty): Set[String] = node match {
    case identifier: Identifier =>
      val knownTypes = rubyTypeSet(symbolTable.get(identifier) ++ identifier.getKnownTypes)
      if (knownTypes.nonEmpty) {
        knownTypes
      } else if (identifier.name.startsWith("<tmp-") && !seenTemporaries.contains(identifier.name)) {
        temporaryAssignmentRhs
          .get(identifier.name)
          .map(expressionValueTypes(_, seenTemporaries + identifier.name))
          .getOrElse(Set.empty)
      } else {
        Set.empty
      }
    case assignment: Call if assignment.name == Operators.assignment =>
      assignment.argumentOut.l match {
        case (identifier: Identifier) :: rhs :: _ =>
          rubyTypeSet(symbolTable.get(identifier) ++ identifier.getKnownTypes ++ expressionValueTypes(rhs))
        case _ => rubyTypeSet(symbolTable.get(assignment) ++ assignment.getKnownTypes)
      }
    case call: Call if call.name == Operators.fieldAccess =>
      val fa          = call.asInstanceOf[FieldAccess]
      val memberTypes = rubyFieldAccessMemberTypes(fa)
      val methodTypes = rubyTypeSet(symbolTable.get(fa) ++ rubyFieldAccessMethodTypes(fa))
      if (memberTypes.nonEmpty) {
        memberTypes
      } else {
        methodReturnValues(methodTypes.toSeq)
      }
    case call: Call if !call.name.startsWith("<operator>") =>
      val methodTypes = rubyMethodTypesForCall(call)
      methodReturnValues(methodTypes.toSeq)
    case call: Call =>
      rubyTypeSet(symbolTable.get(call) ++ call.getKnownTypes)
    case node: StoredNode =>
      rubyTypeSet(node.getKnownTypes)
  }

  private def typedReceiverForFieldAccess(fa: FieldAccess): Set[String] = {
    fa.argumentOut.headOption.map(arg => expressionValueTypes(arg)).getOrElse(Set.empty)
  }

  private def rubyFieldAccessMethodTypes(fa: FieldAccess): Set[String] = {
    val receiverTypes = typedReceiverForFieldAccess(fa)
    val fieldNames = fa.argumentOut.collect { case fieldIdentifier: FieldIdentifier => fieldIdentifier.canonicalName }
    rubyTypeSet(for {
      receiverType <- receiverTypes
      fieldName    <- fieldNames
    } yield s"$receiverType$pathSep$fieldName")
  }

  override protected def getTypesFromCall(c: Call): Set[String] = c.name match {
    case Operators.fieldAccess =>
      val memberTypes = rubyFieldAccessMemberTypes(c.asInstanceOf[FieldAccess])
      if (memberTypes.nonEmpty) memberTypes else super.getTypesFromCall(c)
    case _ if !c.name.startsWith("<operator>") =>
      val methodTypes = rubyMethodTypesForCall(c)
      if (methodTypes.nonEmpty) methodReturnValues(methodTypes.toSeq) else super.getTypesFromCall(c)
    case _ =>
      super.getTypesFromCall(c)
  }

  private def withRubySingletonType(typeFullName: String): String =
    if (typeFullName.endsWith(RubyClassSuffix)) typeFullName else s"$typeFullName$RubyClassSuffix"

  private def existingTypeDeclFullNames(candidates: Iterable[String]): Set[String] = {
    val fullNames = candidates.filterNot(isRubyUnknownType).toSeq.distinct
    if (fullNames.isEmpty) Set.empty else cpg.typeDecl.fullNameExact(fullNames*).fullName.toSet
  }

  private def rubyFieldMemberNameCandidates(fa: FieldAccess): Seq[String] =
    fa.argumentOut
      .collect { case fieldIdentifier: FieldIdentifier => fieldIdentifier.canonicalName }
      .flatMap(name => Seq(name, name.stripPrefix("@")))
      .filter(_.nonEmpty)
      .toSeq
      .distinct

  private def typeDeclFullNamesForConstantPath(path: String): Set[String] = {
    val normalizedPath = path.replace("::", pathSep).stripPrefix(pathSep).stripSuffix(pathSep)
    if (normalizedPath.isBlank) {
      Set.empty
    } else {
      val singletonPath = withRubySingletonType(normalizedPath)
      cpg.typeDecl
        .filter { typeDecl =>
          val fullName = typeDecl.fullName
          fullName == normalizedPath ||
          fullName == singletonPath ||
          fullName.endsWith(s"$pathSep$normalizedPath") ||
          fullName.endsWith(s"$pathSep$singletonPath")
        }
        .fullName
        .toSet
    }
  }

  private def rubyConstantPathFromFieldAccess(fa: FieldAccess): Seq[String] = fa.argumentOut.l match {
    case (base: Identifier) :: (fieldIdentifier: FieldIdentifier) :: Nil if base.name == RubySelf =>
      Seq(fieldIdentifier.canonicalName.stripPrefix("@"))
    case (base: Identifier) :: (fieldIdentifier: FieldIdentifier) :: Nil =>
      Seq(base.name, fieldIdentifier.canonicalName.stripPrefix("@"))
    case (base: TypeRef) :: (fieldIdentifier: FieldIdentifier) :: Nil =>
      Seq(base.typeFullName.stripSuffix(RubyClassSuffix), fieldIdentifier.canonicalName.stripPrefix("@"))
    case (base: Call) :: (fieldIdentifier: FieldIdentifier) :: Nil if base.name == Operators.fieldAccess =>
      rubyConstantPathFromFieldAccess(base.asInstanceOf[FieldAccess]) :+ fieldIdentifier.canonicalName.stripPrefix("@")
    case _ => Seq.empty
  }

  private def rubySelfAccessParentCandidates(fa: FieldAccess): Set[String] = fa.argumentOut.l match {
    case (base: Identifier) :: (_: FieldIdentifier) :: Nil if base.name == RubySelf =>
      val baseTypeCandidates = base.getKnownTypes.flatMap(t => Seq(t, withRubySingletonType(t)))
      val bodyTypeCandidate =
        Option(fa.method.fullName)
          .filter(_.endsWith(s"$pathSep$RubyTypeDeclBody"))
          .map(_.stripSuffix(s"$pathSep$RubyTypeDeclBody"))
          .map(withRubySingletonType)
      existingTypeDeclFullNames(baseTypeCandidates ++ bodyTypeCandidate)
    case _ => Set.empty
  }

  private def rubyConstantBaseParentCandidates(fa: FieldAccess): Set[String] = fa.argumentOut.headOption match {
    case Some(typeRef: TypeRef) =>
      existingTypeDeclFullNames(Seq(typeRef.typeFullName, withRubySingletonType(typeRef.typeFullName)))
    case Some(identifier: Identifier) if identifier.name.headOption.exists(_.isUpper) =>
      typeDeclFullNamesForConstantPath(identifier.name)
    case Some(call: Call) if call.name == Operators.fieldAccess =>
      typeDeclFullNamesForConstantPath(
        rubyConstantPathFromFieldAccess(call.asInstanceOf[FieldAccess]).mkString(pathSep)
      )
    case _ => Set.empty
  }

  private def existingRubyMemberNames(parentTypes: Iterable[String], memberNames: Iterable[String]): Set[String] = {
    val parents = parentTypes.toSeq.distinct
    val names   = memberNames.toSeq.distinct
    if (parents.isEmpty || names.isEmpty) {
      Set.empty
    } else {
      cpg.typeDecl.fullNameExact(parents*).member.nameExact(names*).name.toSet
    }
  }

  private def rubyFieldParentTypes(fa: FieldAccess): Set[String] = {
    val memberNames        = rubyFieldMemberNameCandidates(fa)
    val candidates         = rubySelfAccessParentCandidates(fa) ++ rubyConstantBaseParentCandidates(fa)
    val parentsWithMembers = candidates.filter(parent => existingRubyMemberNames(Seq(parent), memberNames).nonEmpty)
    parentsWithMembers
  }

  private def rubyFieldAccessMemberTypes(fa: FieldAccess): Set[String] = {
    val parentTypes = rubyFieldParentTypes(fa)
    val memberNames = rubyFieldMemberNameCandidates(fa)
    if (parentTypes.isEmpty || memberNames.isEmpty) {
      Set.empty
    } else {
      rubyTypeSet(
        cpg.typeDecl
          .fullNameExact(parentTypes.toSeq*)
          .member
          .nameExact(memberNames*)
          .flatMap(member => member.typeFullName +: (member.dynamicTypeHintFullName ++ member.possibleTypes))
      )
    }
  }

  private def persistRubyFieldMemberTypes(fa: FieldAccess, types: Set[String]): Boolean = {
    val parentTypes = rubyFieldParentTypes(fa)
    val memberNames = rubyFieldMemberNameCandidates(fa)
    val existingMembersByParent = parentTypes
      .map { parent =>
        parent -> existingRubyMemberNames(Seq(parent), memberNames)
      }
      .filter(_._2.nonEmpty)
    existingMembersByParent.foreach { case (parent, names) =>
      names.foreach(name => persistMemberWithTypeDecl(parent, name, types))
    }
    existingMembersByParent.nonEmpty
  }

  override protected def getFieldParents(fa: FieldAccess): Set[String] = {
    val rubyParents = rubyFieldParentTypes(fa)
    if (rubyParents.nonEmpty) rubyParents else super.getFieldParents(fa)
  }

  override protected def assignTypesToCall(x: Call, types: Set[String]): Set[String] = {
    val rubyTypes = rubyTypeSet(types)
    if (x.name == Operators.fieldAccess && rubyTypes.nonEmpty) {
      val fa = x.asInstanceOf[FieldAccess]
      if (persistRubyFieldMemberTypes(fa, rubyTypes)) {
        val memberNames = rubyFieldMemberNameCandidates(fa)
        val symbolName = rubyFieldParentTypes(fa)
          .flatMap(parent => existingRubyMemberNames(Seq(parent), memberNames))
          .headOption
          .orElse(memberNames.headOption)
          .getOrElse(getFieldName(fa))
        symbolTable.append(LocalVar(symbolName), rubyTypes)
      } else {
        super.assignTypesToCall(x, rubyTypes)
      }
    } else {
      super.assignTypesToCall(x, rubyTypes)
    }
  }

  override protected def visitIdentifierAssignedToFieldLoad(i: Identifier, fa: FieldAccess): Set[String] = {
    val memberTypes = rubyFieldAccessMemberTypes(fa)
    if (memberTypes.nonEmpty) {
      symbolTable.append(fa, memberTypes)
      associateTypes(i, memberTypes)
    } else {
      val methodTypes = rubyFieldAccessMethodTypes(fa)
      if (methodTypes.nonEmpty) {
        symbolTable.append(fa, methodTypes)
        associateTypes(i, methodReturnValues(methodTypes.toSeq))
      } else {
        super.visitIdentifierAssignedToFieldLoad(i, fa)
      }
    }
  }

  override protected def visitIdentifierAssignedToCall(i: Identifier, c: Call): Set[String] = {
    if (!c.name.startsWith("<operator>") && !isConstructor(c)) {
      val methodTypes = rubyMethodTypesForCall(c)
      if (methodTypes.nonEmpty) {
        symbolTable.append(c, methodTypes)
        associateTypes(i, methodReturnValues(methodTypes.toSeq))
      } else {
        super.visitIdentifierAssignedToCall(i, c)
      }
    } else {
      super.visitIdentifierAssignedToCall(i, c)
    }
  }

  override protected def setTypeFromTypeHints(n: StoredNode): Unit = {
    val types = rubyTypeSeq(n.getKnownTypes.filterNot(XTypeRecovery.isDummyType))
    if (types.nonEmpty) {
      setTypes(n, types)
    }
  }

  override protected def setTypes(n: StoredNode, types: Seq[String]): Unit = {
    val filteredTypes = rubyTypeSeq(types)
    if (filteredTypes.sizeIs == 1) {
      builder.setNodeProperty(n, PropertyNames.TypeFullName, filteredTypes.head)
    } else if (filteredTypes.nonEmpty) {
      builder.setNodeProperty(n, PropertyNames.DynamicTypeHintFullName, filteredTypes)
    }
  }

  override def storeCallTypeInfo(c: Call, types: Seq[String]): Unit =
    if (types.nonEmpty) {

      // Only necessary if we have more than 1 type and want to try and resolve to a single type
      val finalTypes = if (types.size > 1 && c.receiver.nonEmpty) {
        c.receiver.l.isCall.headOption match {
          case Some(recCall) =>
            if (recCall.methodFullName == Operators.fieldAccess) {
              val fieldAccessCall = recCall.asInstanceOf[FieldAccess]
              val fieldAccessName = getFieldName(fieldAccessCall) // Returns Module1.foo for ex when it can be resolved
              val fieldAccessParents = getFieldParents(fieldAccessCall)
              // Some FieldAccess return unknown (ex regex: 'x' =~ /y/) so we return types since we cannot resolve further
              if (fieldAccessName == "<unknown>")
                types
              else
                fieldAccessParents
                  .filter(_.endsWith(fieldAccessName.stripSuffix(s".${c.name}")))
                  .map(x => s"$x.${c.name}")
            } else {
              types
            }
          case None =>
            types
        }
      } else {
        types
      }

      val filteredFinalTypes = rubyTypeSeq(c.dynamicTypeHintFullName ++ finalTypes)
      builder.setNodeProperty(c, PropertyNames.DynamicTypeHintFullName, filteredFinalTypes)
    }
}
