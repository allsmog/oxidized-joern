package io.joern.csharpsrc2cpg

object Constants {
  val This: String                   = "this"
  val Base: String                   = "base"
  val Global: String                 = "global"
  val TopLevelMainMethodName: String = "<Main>$"
}

object CSharpOperators {
  val throws: String           = "<operators>.throw"
  val unknown: String          = "<operators>.unknown"
  val await: String            = "<operator>.await"
  val indexFromEnd: String     = "<operator>.indexFromEnd"
  val queryExpression: String  = "<operator>.query"
  val ref: String              = "<operator>.ref"
  val makeRef: String          = "<operator>.makeRef"
  val refType: String          = "<operator>.refType"
  val refValue: String         = "<operator>.refValue"
  val spread: String           = "<operator>.spread"
  val stackAlloc: String       = "<operator>.stackalloc"
  val switchExpression: String = "<operator>.switch"
  val tuple: String            = "<operator>.tuple"
  val nameOf: String           = "<operator>.nameof"
  val typeOf: String           = "<operator>.typeOf"
  val defaultValue: String     = "<operator>.default"
  val withExpression: String   = "<operator>.with"
}

object CSharpModifiers {
  final val CONST: String    = "const"
  final val ASYNC: String    = "async"
  final val OVERRIDE: String = "override"
  final val FILE: String     = "file"
  final val IN: String       = "in"
  final val NEW: String      = "new"
  final val OUT: String      = "out"
  final val PARAMS: String   = "params"
  final val PARTIAL: String  = "partial"
  final val REF: String      = "ref"
  final val REQUIRED: String = "required"
  final val SCOPED: String   = "scoped"
  final val STRUCT: String   = "struct"
  final val THIS: String     = "this"
  final val UNSAFE: String   = "unsafe"
  final val VOLATILE: String = "volatile"
}

object CSharpDefines {
  final val AnonymousTypePrefix = "<anon>"
}
