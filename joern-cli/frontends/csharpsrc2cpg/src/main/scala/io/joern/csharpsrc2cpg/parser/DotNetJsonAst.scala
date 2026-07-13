package io.joern.csharpsrc2cpg.parser

import org.slf4j.LoggerFactory

object DotNetJsonAst {

  private val logger                     = LoggerFactory.getLogger(getClass)
  private val QualifiedClassName: String = DotNetJsonAst.getClass.getName

  def fromString(nodeName: String, fileName: Option[String] = None): DotNetParserNode = {
    try {
      val clazz = Class.forName(s"$QualifiedClassName${nodeName.stripPrefix("ast.")}$$")
      clazz.getField("MODULE$").get(clazz).asInstanceOf[DotNetParserNode]
    } catch {
      case _: Throwable =>
        logger.warn(
          s"`$nodeName` AST type is not handled.${fileName.map(x => s" We found this inside '$x'").getOrElse("")}"
        )
        NotHandledType
    }
  }

  sealed trait DotNetParserNode {
    override def toString: String = this.getClass.getSimpleName.stripSuffix("$")
  }

  sealed trait BaseExpr extends DotNetParserNode
  sealed trait BaseStmt extends DotNetParserNode

  sealed trait BasePattern extends DotNetParserNode

  sealed trait BaseLabel extends DotNetParserNode

  sealed trait JumpStatement extends BaseStmt

  sealed trait BaseLambdaExpression extends BaseExpr

  object GlobalStatement extends BaseStmt

  object ExpressionStatement extends BaseStmt

  object EmptyStatement extends BaseStmt

  object LabeledStatement extends BaseStmt

  object LockStatement extends BaseStmt

  object CheckedStatement extends BaseStmt

  object UnsafeStatement extends BaseStmt

  object FixedStatement extends BaseStmt

  object NotHandledType extends DotNetParserNode

  object CompilationUnit extends BaseExpr

  object NamespaceDeclaration extends DeclarationExpr

  object FileScopedNamespaceDeclaration extends DeclarationExpr

  sealed trait DeclarationExpr extends BaseExpr

  sealed trait TypeDeclaration extends DeclarationExpr

  object ClassDeclaration extends TypeDeclaration

  object StructDeclaration extends TypeDeclaration

  object RecordDeclaration extends TypeDeclaration

  object EnumDeclaration extends TypeDeclaration

  object AnonymousObjectCreationExpression extends TypeDeclaration

  object EnumMemberDeclaration extends DeclarationExpr

  object InterfaceDeclaration extends TypeDeclaration

  object DelegateDeclaration extends TypeDeclaration

  object MethodDeclaration extends DeclarationExpr

  object ConstructorDeclaration extends DeclarationExpr

  sealed trait ConstructorInitializer extends BaseExpr

  object BaseConstructorInitializer extends ConstructorInitializer

  object ThisConstructorInitializer extends ConstructorInitializer

  object FieldDeclaration extends DeclarationExpr

  object EventFieldDeclaration extends DeclarationExpr

  object EventDeclaration extends DeclarationExpr

  object IndexerDeclaration extends DeclarationExpr

  object OperatorDeclaration extends DeclarationExpr

  object ConversionOperatorDeclaration extends DeclarationExpr

  object DestructorDeclaration extends DeclarationExpr

  object VariableDeclaration extends DeclarationExpr

  object LocalDeclarationStatement extends DeclarationExpr

  object VariableDeclarator extends DeclarationExpr

  object SimpleLambdaExpression extends BaseLambdaExpression

  object ParenthesizedLambdaExpression extends BaseLambdaExpression

  object AnonymousMethodExpression extends BaseLambdaExpression

  sealed trait PatternExpr extends BaseExpr

  object IsPatternExpression extends PatternExpr

  object DeclarationPattern extends PatternExpr

  object SingleVariableDesignation extends PatternExpr

  object Designation extends PatternExpr

  sealed trait ClauseExpr extends BaseExpr

  object EqualsValueClause extends ClauseExpr

  object WhenClause extends ClauseExpr

  object FromClause extends ClauseExpr

  object JoinClause extends ClauseExpr

  object JoinIntoClause extends ClauseExpr

  object LetClause extends ClauseExpr

  object OrderByClause extends ClauseExpr

  object WhereClause extends ClauseExpr

  object SelectClause extends ClauseExpr

  object GroupClause extends ClauseExpr

  sealed trait LiteralExpr extends BaseExpr

  object NumericLiteralExpression extends LiteralExpr
  object StringLiteralExpression  extends LiteralExpr
  object TrueLiteralExpression    extends LiteralExpr
  object FalseLiteralExpression   extends LiteralExpr
  object NullLiteralExpression    extends LiteralExpr

  object UsingDirective extends BaseExpr

  object ExternAliasDirective extends BaseExpr

  object GlobalAttribute extends BaseExpr

  object ExplicitInterfaceSpecifier extends BaseExpr

  sealed trait PreprocessorBranch extends BaseExpr

  object PreprocessorDirective extends BaseExpr

  object ShebangDirective extends BaseExpr

  object PreprocessorIfDirective extends PreprocessorBranch

  object PreprocessorElifDirective extends PreprocessorBranch

  object PreprocessorElseDirective extends PreprocessorBranch

  object Parameter extends BaseExpr

  object FunctionPointerParameter extends BaseExpr

  sealed trait TypeExpr extends BaseExpr

  object ArrayType extends TypeExpr

  object ArrayRankSpecifier extends TypeExpr

  object TupleType extends TypeExpr

  object TupleElement extends TypeExpr

  object PredefinedType extends TypeExpr

  object PointerType extends TypeExpr

  object FunctionPointerType extends TypeExpr

  object RefType extends TypeExpr

  object ScopedType extends TypeExpr

  object SimpleBaseType extends TypeExpr

  object PrimaryConstructorBaseType extends TypeExpr

  object Block extends BaseExpr

  sealed trait IdentifierNode extends BaseExpr

  object IdentifierName extends IdentifierNode

  object QualifiedName extends IdentifierNode

  sealed trait UnaryExpr         extends BaseExpr
  object PostIncrementExpression extends UnaryExpr
  object PostDecrementExpression extends UnaryExpr
  object PreIncrementExpression  extends UnaryExpr
  object PreDecrementExpression  extends UnaryExpr
  object UnaryPlusExpression     extends UnaryExpr
  object UnaryMinusExpression    extends UnaryExpr
  object BitwiseNotExpression    extends UnaryExpr
  object LogicalNotExpression    extends UnaryExpr
  object AddressOfExpression     extends UnaryExpr
  object IndirectionExpression   extends UnaryExpr
  object IndexExpression         extends UnaryExpr

  sealed trait BinaryExpr     extends BaseExpr
  object AddExpression        extends BinaryExpr
  object SubtractExpression   extends BinaryExpr
  object MultiplyExpression   extends BinaryExpr
  object DivideExpression     extends BinaryExpr
  object ModuloExpression     extends BinaryExpr
  object EqualsExpression     extends BinaryExpr
  object NotEqualsExpression  extends BinaryExpr
  object LogicalAndExpression extends BinaryExpr
  object LogicalOrExpression  extends BinaryExpr
  object CoalesceExpression   extends BinaryExpr

  sealed trait AssignmentExpr                   extends BinaryExpr
  object AddAssignmentExpression                extends AssignmentExpr
  object SubtractAssignmentExpression           extends AssignmentExpr
  object MultiplyAssignmentExpression           extends AssignmentExpr
  object DivideAssignmentExpression             extends AssignmentExpr
  object ModuloAssignmentExpression             extends AssignmentExpr
  object AndAssignmentExpression                extends AssignmentExpr
  object OrAssignmentExpression                 extends AssignmentExpr
  object ExclusiveOrAssignmentExpression        extends AssignmentExpr
  object CoalesceAssignmentExpression           extends AssignmentExpr
  object RightShiftAssignmentExpression         extends AssignmentExpr
  object UnsignedRightShiftAssignmentExpression extends AssignmentExpr
  object LeftShiftAssignmentExpression          extends AssignmentExpr
  object SimpleAssignmentExpression             extends AssignmentExpr

  object GreaterThanExpression        extends BinaryExpr
  object LessThanExpression           extends BinaryExpr
  object GreaterThanOrEqualExpression extends BinaryExpr
  object LessThanOrEqualExpression    extends BinaryExpr
  object LeftShiftExpression          extends BinaryExpr
  object RightShiftExpression         extends BinaryExpr
  object UnsignedRightShiftExpression extends BinaryExpr
  object BitwiseAndExpression         extends BinaryExpr
  object BitwiseOrExpression          extends BinaryExpr
  object ExclusiveOrExpression        extends BinaryExpr
  object RangeExpression              extends BinaryExpr

  object QueryExpression extends BaseExpr

  object InvocationExpression extends BaseExpr

  object NameOfExpression extends BaseExpr

  object Argument extends BaseExpr

  object ArgumentList extends BaseExpr

  object BracketedArgumentList extends BaseExpr

  trait MemberAccessExpr extends BaseExpr

  object SimpleMemberAccessExpression extends MemberAccessExpr

  object ThisExpression extends MemberAccessExpr

  object BaseExpression extends MemberAccessExpr

  object IfStatement extends BaseStmt

  object ElseClause extends ClauseExpr

  object ThrowStatement extends BaseStmt

  object ObjectCreationExpression extends BaseExpr

  object WithExpression extends BaseExpr

  object TryStatement extends BaseStmt

  object CatchDeclaration extends DeclarationExpr

  object CatchClause extends ClauseExpr

  object CatchFilterClause extends ClauseExpr

  object FinallyClause extends ClauseExpr

  object ForEachStatement extends BaseStmt

  object ForStatement extends BaseStmt

  object DoStatement extends BaseStmt

  object WhileStatement extends BaseStmt

  object SwitchStatement extends BaseStmt

  object SwitchSection extends BaseExpr

  object SwitchExpression extends BaseExpr

  object SwitchExpressionArm extends BaseExpr

  object UsingStatement extends BaseStmt

  object RelationalPattern extends BasePattern

  object ConstantPattern extends BasePattern

  object DiscardPattern extends BasePattern

  object NegatedPattern extends BasePattern

  object AndPattern extends BasePattern

  object OrPattern extends BasePattern

  object ParenthesizedPattern extends BasePattern

  object ListPattern extends BasePattern

  object RecursivePattern extends BasePattern

  object TypePattern extends BasePattern

  object VarPattern extends BasePattern

  object TuplePattern extends BasePattern

  object ParenthesizedVariableDesignation extends BasePattern

  object Subpattern extends BasePattern

  object CaseSwitchLabel extends BaseLabel

  object CasePatternSwitchLabel extends BaseLabel

  object DefaultSwitchLabel extends BaseLabel

  object BreakStatement extends JumpStatement

  object ContinueStatement extends JumpStatement

  object GotoStatement extends JumpStatement

  object ReturnStatement extends JumpStatement

  object YieldStatement extends JumpStatement

  object LocalFunctionStatement extends DeclarationExpr with BaseStmt

  object AwaitExpression extends BaseExpr

  object PropertyDeclaration extends DeclarationExpr

  object TypeArgumentList extends BaseStmt

  object TypeParameterList extends BaseStmt

  object TypeParameter extends BaseStmt

  object TypeParameterConstraintClause extends BaseStmt

  object TypeParameterConstraint extends BaseStmt

  object GenericName extends BaseStmt

  object NullableType extends BaseExpr

  object ArrayInitializerExpression extends BaseExpr

  object ObjectInitializerExpression extends BaseExpr

  object ElementAccessExpression extends BaseExpr

  object CollectionExpression extends BaseExpr

  object TupleExpression extends BaseExpr

  object ExpressionElement extends BaseExpr

  object CastExpression extends BaseExpr

  object AsExpression extends BaseExpr

  object IsExpression extends BaseExpr

  object TypeOfExpression extends BaseExpr

  object SizeOfExpression extends BaseExpr

  object DefaultExpression extends BaseExpr

  object ThrowExpression extends BaseExpr

  object RefExpression extends BaseExpr

  object MakeRefExpression extends BaseExpr

  object RefTypeExpression extends BaseExpr

  object RefValueExpression extends BaseExpr

  object SpreadElement extends BaseExpr

  object CheckedExpression extends BaseExpr

  object AnonymousObjectMemberDeclarator extends DeclarationExpr

  object ConditionalExpression extends BaseExpr

  object ImplicitArrayCreationExpression extends BaseExpr

  object StackAllocExpression extends BaseExpr

  object InterpolatedStringExpression extends BaseExpr

  object InterpolatedStringText extends BaseExpr

  object Interpolation extends BaseExpr

  object InterpolationAlignmentClause extends BaseExpr

  object InterpolationFormatClause extends BaseExpr

  object ConditionalAccessExpression extends MemberAccessExpr

  object MemberBindingExpression extends BaseExpr

  object SuppressNullableWarningExpression extends BaseExpr

  object AttributeList extends BaseExpr

  object Attribute extends BaseExpr

  object AttributeTargetSpecifier extends BaseExpr

  object AttributeArgumentList extends BaseExpr

  object AttributeArgument extends BaseExpr

  object ParenthesizedExpression extends BaseExpr

  object Unknown extends DotNetParserNode

  object AccessorList extends DotNetParserNode

  object GetAccessorDeclaration extends DotNetParserNode

  object SetAccessorDeclaration extends DotNetParserNode

  object AddAccessorDeclaration extends DotNetParserNode

  object RemoveAccessorDeclaration extends DotNetParserNode

}

/** The JSON key values, in alphabetical order.
  */
object ParserKeys {

  val AccessorList               = "AccessorList"
  val Accessors                  = "Accessors"
  val Arms                       = "Arms"
  val AstRoot                    = "AstRoot"
  val AlignmentClause            = "AlignmentClause"
  val Alias                      = "Alias"
  val Arguments                  = "Arguments"
  val ArgumentList               = "ArgumentList"
  val AttributeLists             = "AttributeLists"
  val Attributes                 = "Attributes"
  val Await                      = "Await"
  val BaseList                   = "BaseList"
  val Body                       = "Body"
  val ByExpression               = "ByExpression"
  val CallingConvention          = "CallingConvention"
  val Block                      = "Block"
  val Catches                    = "Catches"
  val Clauses                    = "Clauses"
  val Code                       = "Code"
  val ColumnStart                = "ColumnStart"
  val ColumnEnd                  = "ColumnEnd"
  val Condition                  = "Condition"
  val Contents                   = "Contents"
  val ConstraintClauses          = "ConstraintClauses"
  val Constraints                = "Constraints"
  val Declaration                = "Declaration"
  val Designation                = "Designation"
  val Directions                 = "Directions"
  val Elements                   = "Elements"
  val ElementType                = "ElementType"
  val Else                       = "Else"
  val Expression                 = "Expression"
  val ExpressionElement          = "ExpressionElement"
  val Expressions                = "Expressions"
  val ExpressionBody             = "ExpressionBody"
  val ExplicitInterfaceSpecifier = "ExplicitInterfaceSpecifier"
  val Finally                    = "Finally"
  val Filter                     = "Filter"
  val FileName                   = "FileName"
  val FormatClause               = "FormatClause"
  val FormatStringToken          = "FormatStringToken"
  val FromClause                 = "FromClause"
  val Global                     = "Global"
  val GetAccessorDeclaration     = "GetAccessorDeclaration"
  val GoverningExpression        = "GoverningExpression"
  val HasSlice                   = "HasSlice"
  val Identifier                 = "Identifier"
  val Incrementors               = "Incrementors"
  val Initializer                = "Initializer"
  val Initializers               = "Initializers"
  val InExpression               = "InExpression"
  val Into                       = "Into"
  val Keyword                    = "Keyword"
  val Kind                       = "Kind"
  val Labels                     = "Labels"
  val Left                       = "Left"
  val LeftExpression             = "LeftExpression"
  val LineStart                  = "LineStart"
  val LineEnd                    = "LineEnd"
  val MetaData                   = "MetaData"
  val Members                    = "Members"
  val Modifiers                  = "Modifiers"
  val Name                       = "Name"
  val NameColon                  = "NameColon"
  val NameEquals                 = "NameEquals"
  val Operand                    = "Operand"
  val OperatorToken              = "OperatorToken"
  val Parameter                  = "Parameter"
  val Parameters                 = "Parameters"
  val ParameterList              = "ParameterList"
  val Pattern                    = "Pattern"
  val Patterns                   = "Patterns"
  val PositionalPatterns         = "PositionalPatterns"
  val PropertyPatterns           = "PropertyPatterns"
  val RefKind                    = "RefKind"
  val Sections                   = "Sections"
  val SetAccessorDeclaration     = "SetAccessorDeclaration"
  val SingleVariableDesignation  = "SingleVariableDesignation"
  val Statement                  = "Statement"
  val Statements                 = "Statements"
  val Static                     = "Static"
  val Target                     = "Target"
  val ReturnType                 = "ReturnType"
  val Rank                       = "Rank"
  val Right                      = "Right"
  val RightExpression            = "RightExpression"
  val SliceIndex                 = "SliceIndex"
  val TextToken                  = "TextToken"
  val Type                       = "Type"
  val TypeArgumentList           = "TypeArgumentList"
  val TypeParameterList          = "TypeParameterList"
  val Types                      = "Types"
  val Unsafe                     = "Unsafe"
  val Using                      = "Using"
  val Usings                     = "Usings"
  val Value                      = "Value"
  val Variables                  = "Variables"
  val WhenFalse                  = "WhenFalse"
  val WhenClause                 = "WhenClause"
  val WhenNotNull                = "WhenNotNull"
  val WhenTrue                   = "WhenTrue"
}
