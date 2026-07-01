package io.joern.x2cpg.testfixtures

import flatgraph.Edge
import io.shiftleft.codepropertygraph.generated.Cpg
import io.shiftleft.codepropertygraph.generated.nodes.StoredNode

/** Test-scope CPG equivalence for frontend replacement gates.
  *
  * Runtime node ids are intentionally excluded. A node is identified by its label plus emitted properties; an edge is
  * identified by the canonical source node, edge label/property, and canonical destination node.
  */
object CpgEquivalence {

  final case class Comparison(
    actualOnlyNodes: Seq[String],
    expectedOnlyNodes: Seq[String],
    actualOnlyEdges: Seq[String],
    expectedOnlyEdges: Seq[String]
  ) {
    def isEquivalent: Boolean =
      actualOnlyNodes.isEmpty &&
        expectedOnlyNodes.isEmpty &&
        actualOnlyEdges.isEmpty &&
        expectedOnlyEdges.isEmpty

    def diff: String = {
      if (isEquivalent) "CPGs are equivalent"
      else {
        Seq(
          section("Nodes only in actual", actualOnlyNodes, "+"),
          section("Nodes only in expected", expectedOnlyNodes, "-"),
          section("Edges only in actual", actualOnlyEdges, "+"),
          section("Edges only in expected", expectedOnlyEdges, "-")
        ).filter(_.nonEmpty).mkString("\n")
      }
    }
  }

  def equivalent(actual: Cpg, expected: Cpg): Boolean =
    compare(actual, expected).isEquivalent

  def diff(actual: Cpg, expected: Cpg): String =
    compare(actual, expected).diff

  def compare(actual: Cpg, expected: Cpg): Comparison = {
    val actualSnapshot   = Snapshot.from(actual)
    val expectedSnapshot = Snapshot.from(expected)

    Comparison(
      actualOnlyNodes = multisetDiff(actualSnapshot.nodes, expectedSnapshot.nodes),
      expectedOnlyNodes = multisetDiff(expectedSnapshot.nodes, actualSnapshot.nodes),
      actualOnlyEdges = multisetDiff(actualSnapshot.edges, expectedSnapshot.edges),
      expectedOnlyEdges = multisetDiff(expectedSnapshot.edges, actualSnapshot.edges)
    )
  }

  private final case class Snapshot(nodes: Seq[String], edges: Seq[String])

  private object Snapshot {
    def from(cpg: Cpg): Snapshot = {
      val nodes = cpg.graph.allNodes.collect { case node: StoredNode => node }.toSeq
      val nodeSignaturesById = nodes.map { node =>
        node.id -> nodeSignature(node)
      }.toMap
      val edges = cpg.graph.allEdges.map(edgeSignature(_, nodeSignaturesById)).toSeq
      Snapshot(nodes.map(node => nodeSignature(node)).sorted, edges.sorted)
    }
  }

  private def nodeSignature(node: StoredNode, depth: Int = 0): String = {
    val properties = node.properties.toSeq
      .map { case (name, value) => s"${escape(name.toString)}=${normalizeValue(value, depth + 1)}" }
      .sortBy(identity)
      .mkString("|")

    if (properties.isEmpty) s"NODE|${node.label}"
    else s"NODE|${node.label}|$properties"
  }

  private def edgeSignature(edge: Edge, nodeSignaturesById: Map[Long, String]): String = {
    val src = nodeSignaturesById.getOrElse(edge.src.id, s"NODE_REF|${edge.src.label}")
    val dst = nodeSignaturesById.getOrElse(edge.dst.id, s"NODE_REF|${edge.dst.label}")
    val edgeProperty = edge.propertyMaybe match {
      case Some(value) =>
        val name = edge.propertyName.getOrElse("property")
        s"|${escape(name)}=${normalizeValue(value)}"
      case None => ""
    }
    s"EDGE|$src|${escape(edge.label)}|$dst$edgeProperty"
  }

  private def normalizeValue(value: Any, depth: Int = 0): String = value match {
    case null                 => "<null>"
    case None                 => "<none>"
    case Some(inner)          => normalizeValue(inner, depth)
    case node: StoredNode     => normalizeNodeRef(node, depth)
    case values: Array[?]     => values.iterator.map(normalizeValue(_, depth)).mkString("[", ",", "]")
    case values: Iterable[?]  => values.iterator.map(normalizeValue(_, depth)).mkString("[", ",", "]")
    case other                => escape(other.toString)
  }

  private def normalizeNodeRef(node: StoredNode, depth: Int): String = {
    if (depth >= 2) s"node(${node.label})"
    else s"node(${nodeSignature(node, depth)})"
  }

  private def multisetDiff(left: Seq[String], right: Seq[String]): Seq[String] = {
    val leftCounts  = counts(left)
    val rightCounts = counts(right)
    leftCounts.toSeq
      .flatMap { case (value, count) =>
        Seq.fill(count - rightCounts.getOrElse(value, 0))(value)
      }
      .sorted
  }

  private def counts(values: Seq[String]): Map[String, Int] =
    values.groupMapReduce(identity)(_ => 1)(_ + _)

  private def section(title: String, lines: Seq[String], marker: String): String = {
    if (lines.isEmpty) ""
    else lines.map(line => s"$marker $line").mkString(s"[$title]\n", "\n", "")
  }

  private def escape(value: String): String =
    value
      .replace("\\", "\\\\")
      .replace("\n", "\\n")
      .replace("\r", "\\r")
      .replace("|", "\\|")
}
