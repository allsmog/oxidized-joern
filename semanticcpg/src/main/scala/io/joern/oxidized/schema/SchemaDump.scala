package io.joern.oxidized.schema

import io.shiftleft.codepropertygraph.generated.GraphSchema
import io.shiftleft.codepropertygraph.generated.nodes.NewNode
import org.json4s.JsonAST.*
import org.json4s.native.JsonMethods.{pretty, render}

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path, Paths}
import scala.util.matching.Regex

object SchemaDump {

  private val SchemaVersion              = 1
  private val DefaultOutput              = Paths.get("schema", "cpg-schema.json")
  private val GeneratedNodesPackage      = "io.shiftleft.codepropertygraph.generated.nodes"
  private val CpgVersionPattern: Regex   = "(?m)^\\s*val\\s+cpgVersion\\s*=\\s*\"([^\"]+)\"".r
  private val JsonPropertyTypeSuffix     = "Type"
  private val JsonPropertyDefaultSuffix  = "WithDefault"
  private val JsonPropertyQuantityPrefix = "Qty"

  def main(args: Array[String]): Unit = {
    val repoRoot   = Paths.get("").toAbsolutePath.normalize()
    val outputPath = outputPathFrom(args, repoRoot)
    Files.createDirectories(outputPath.getParent)

    val cpgVersion = readCpgVersion(repoRoot)
    val json       = schemaJson(cpgVersion)
    Files.writeString(outputPath, renderJson(json) + System.lineSeparator(), StandardCharsets.UTF_8)
    println(s"Wrote CPG schema $cpgVersion to $outputPath")
  }

  private def renderJson(json: JObject): String = {
    pretty(render(json)).linesIterator.map(_.stripTrailing).mkString(System.lineSeparator())
  }

  private def outputPathFrom(args: Array[String], repoRoot: Path): Path = {
    val requested = args.headOption.map(Paths.get(_)).getOrElse(DefaultOutput)
    val resolved  = if (requested.isAbsolute) requested else repoRoot.resolve(requested)
    resolved.normalize()
  }

  private def readCpgVersion(repoRoot: Path): String = {
    val buildFile = repoRoot.resolve("build.sbt")
    val content   = Files.readString(buildFile, StandardCharsets.UTF_8)
    CpgVersionPattern
      .findFirstMatchIn(content)
      .map(_.group(1))
      .getOrElse(throw new IllegalStateException(s"Unable to find cpgVersion in $buildFile"))
  }

  private def schemaJson(cpgVersion: String): JObject = {
    val nodesByLabel = instantiateNodes()
    val edgeLabels   = GraphSchema.edgeLabels.toSeq.sorted
    val endpointsByEdge =
      endpointPairs(nodesByLabel, edgeLabels).groupBy(_.edge).view.mapValues(_.sortBy(e => (e.src, e.dst))).toMap
    val allowedOutByNode = allowedOutEdges(nodesByLabel.keys.toSeq.sorted, endpointsByEdge)
    val allowedInByNode  = allowedInEdges(nodesByLabel.keys.toSeq.sorted, endpointsByEdge)
    val nodeObjectsByName = nodesByLabel.keys.toSeq.sorted.map { label =>
      label -> nodeJson(
        label,
        allowedOutByNode.getOrElse(label, Map.empty),
        allowedInByNode.getOrElse(label, Map.empty)
      )
    }

    obj(
      "schemaVersion" -> JInt(SchemaVersion),
      "cpgVersion"    -> JString(cpgVersion),
      "metadata" -> obj(
        "nodeCount"               -> JInt(nodesByLabel.size),
        "edgeCount"               -> JInt(edgeLabels.size),
        "propertyKindCount"       -> JInt(GraphSchema.getNumberOfPropertyKinds),
        "normalNodePropertyCount" -> JInt(GraphSchema.normalNodePropertyNames.length),
        "source"                  -> JString("io.shiftleft.codepropertygraph.generated.GraphSchema"),
        "allowedEndpointSource"   -> JString("generated New* node validators"),
        "generatedBy"             -> JString("io.joern.oxidized.schema.SchemaDump")
      ),
      "nodes" -> obj(nodeObjectsByName*),
      "edges" -> obj(edgeLabels.map { edgeLabel =>
        edgeLabel -> edgeJson(endpointsByEdge.getOrElse(edgeLabel, Seq.empty))
      }*)
    )
  }

  private def nodeJson(
    label: String,
    allowedOut: Map[String, Seq[String]],
    allowedIn: Map[String, Seq[String]]
  ): JObject = {
    obj(
      "properties"      -> JArray(propertyJson(label)),
      "allowedOutEdges" -> edgeTargetMapJson(allowedOut),
      "allowedInEdges"  -> edgeTargetMapJson(allowedIn)
    )
  }

  private def propertyJson(label: String): List[JObject] = {
    val nodeKind = GraphSchema.getNodeKindByLabel(label)
    GraphSchema
      .getNodePropertyNames(label)
      .toSeq
      .sorted
      .map { propertyName =>
        val propertyKind = GraphSchema.getPropertyKindByName(propertyName)
        obj(
          "name" -> JString(propertyName),
          "type" -> JString(normalizeFormalType(GraphSchema.getNodePropertyFormalType(nodeKind, propertyKind))),
          "cardinality" -> JString(
            normalizeFormalQuantity(GraphSchema.getNodePropertyFormalQuantity(nodeKind, propertyKind))
          )
        )
      }
      .toList
  }

  private def edgeJson(endpoints: Seq[Endpoint]): JObject = {
    obj(
      "srcLabels" -> stringArray(endpoints.map(_.src).distinct.sorted),
      "dstLabels" -> stringArray(endpoints.map(_.dst).distinct.sorted),
      "allowedEndpoints" -> JArray(
        endpoints.map(endpoint => obj("src" -> JString(endpoint.src), "dst" -> JString(endpoint.dst))).toList
      )
    )
  }

  private def endpointPairs(nodesByLabel: Map[String, NewNode], edgeLabels: Seq[String]): Seq[Endpoint] = {
    for {
      edgeLabel <- edgeLabels
      src       <- nodesByLabel.toSeq.sortBy(_._1)
      dst       <- nodesByLabel.toSeq.sortBy(_._1)
      if src._2.isValidOutNeighbor(edgeLabel, dst._2) && dst._2.isValidInNeighbor(edgeLabel, src._2)
    } yield Endpoint(src._1, edgeLabel, dst._1)
  }

  private def allowedOutEdges(
    nodeLabels: Seq[String],
    endpointsByEdge: Map[String, Seq[Endpoint]]
  ): Map[String, Map[String, Seq[String]]] = {
    nodeLabels.map { label =>
      val allowed = endpointsByEdge.toSeq.flatMap { case (edgeLabel, endpoints) =>
        val targets = endpoints.collect { case Endpoint(`label`, _, dst) => dst }.distinct.sorted
        Option.when(targets.nonEmpty)(edgeLabel -> targets)
      }.toMap
      label -> allowed
    }.toMap
  }

  private def allowedInEdges(
    nodeLabels: Seq[String],
    endpointsByEdge: Map[String, Seq[Endpoint]]
  ): Map[String, Map[String, Seq[String]]] = {
    nodeLabels.map { label =>
      val allowed = endpointsByEdge.toSeq.flatMap { case (edgeLabel, endpoints) =>
        val sources = endpoints.collect { case Endpoint(src, _, `label`) => src }.distinct.sorted
        Option.when(sources.nonEmpty)(edgeLabel -> sources)
      }.toMap
      label -> allowed
    }.toMap
  }

  private def instantiateNodes(): Map[String, NewNode] = {
    GraphSchema.nodeLabels.toSeq.sorted.map { label =>
      label -> instantiateNode(label)
    }.toMap
  }

  private def instantiateNode(label: String): NewNode = {
    val className = s"$GeneratedNodesPackage.New${toPascalCase(label)}"
    Class.forName(className).getDeclaredConstructor().newInstance().asInstanceOf[NewNode]
  }

  private def toPascalCase(label: String): String = {
    label
      .split("_")
      .map(part => part.toLowerCase.capitalize)
      .mkString
  }

  private def normalizeFormalType(value: AnyRef): String = {
    val simpleName = singletonName(value)
      .stripSuffix(JsonPropertyDefaultSuffix)
      .stripSuffix(JsonPropertyTypeSuffix)
    simpleName match {
      case "Bool"    => "bool"
      case "String"  => "string"
      case "Byte"    => "byte"
      case "Short"   => "short"
      case "Int"     => "int"
      case "Long"    => "long"
      case "Float"   => "float"
      case "Double"  => "double"
      case "Ref"     => "ref"
      case "Nothing" => "nothing"
      case other     => other
    }
  }

  private def normalizeFormalQuantity(value: AnyRef): String = {
    singletonName(value).stripPrefix(JsonPropertyQuantityPrefix) match {
      case "One"    => "one"
      case "Option" => "optional"
      case "Multi"  => "multi"
      case "None"   => "none"
      case other    => other
    }
  }

  private def singletonName(value: AnyRef): String = {
    value.getClass.getSimpleName.stripSuffix("$")
  }

  private def edgeTargetMapJson(edges: Map[String, Seq[String]]): JObject = {
    obj(edges.toSeq.sortBy(_._1).map { case (edgeLabel, targets) =>
      edgeLabel -> stringArray(targets)
    }*)
  }

  private def stringArray(values: Seq[String]): JArray = {
    JArray(values.sorted.map(JString.apply).toList)
  }

  private def obj(fields: (String, JValue)*): JObject = {
    JObject(fields.map { case (name, value) => JField(name, value) }.toList)
  }

  private final case class Endpoint(src: String, edge: String, dst: String)

}
