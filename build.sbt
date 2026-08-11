name                     := "joern"
// Fork coordinates. NOT `io.joern` — that namespace belongs to upstream.
ThisBuild / organization := "io.github.allsmog"
ThisBuild / scalaVersion := "3.7.4"

val cpgVersion = "1.7.70"

lazy val joerncli          = Projects.joerncli
lazy val querydb           = Projects.querydb
lazy val console           = Projects.console
lazy val dataflowengineoss = Projects.dataflowengineoss
lazy val macros            = Projects.macros
lazy val semanticcpg       = Projects.semanticcpg
lazy val ghidra2cpg        = Projects.ghidra2cpg
lazy val x2cpg             = Projects.x2cpg
// library for kotlin2cpg interop only; standalone java-src scanning is in cpg-rs
lazy val javasrc2cpg       = Projects.javasrc2cpg
lazy val php2cpg           = Projects.php2cpg
lazy val jimple2cpg        = Projects.jimple2cpg
lazy val kotlin2cpg        = Projects.kotlin2cpg
lazy val swiftsrc2cpg      = Projects.swiftsrc2cpg
lazy val csharpsrc2cpg     = Projects.csharpsrc2cpg
lazy val abap2cpg          = Projects.abap2cpg
lazy val linterRules       = Projects.linterRules

lazy val root = project
  .in(file("."))
  .aggregate(
    joerncli,
    querydb,
    console,
    dataflowengineoss,
    macros,
    semanticcpg,
    ghidra2cpg,
    x2cpg,
    php2cpg,
    javasrc2cpg,
    jimple2cpg,
    kotlin2cpg,
    swiftsrc2cpg,
    csharpsrc2cpg,
    abap2cpg,
    linterRules
  )
  .dependsOn(linterRules % ScalafixConfig)

ThisBuild / libraryDependencies ++= Seq(
  "org.slf4j"                % "slf4j-api"         % Versions.slf4j,
  "org.apache.logging.log4j" % "log4j-slf4j2-impl" % Versions.log4j % Optional,
  "org.apache.logging.log4j" % "log4j-core"        % Versions.log4j % Optional
  // `Optional` means "not transitive", but still included in "stage/lib"
)

ThisBuild / compile / javacOptions ++= Seq(
  "-g", // debug symbols
  "-Xlint",
  "-proc:none",
  "--release=11"
) ++ {
  // Require Java 13+ due to FileSystems.newFileSystem(Path) API used in project/FileUtils.scala
  val javaVersion = sys.props("java.specification.version").toFloat
  assert(javaVersion.toInt >= 13, s"this build requires JDK13+ - you're using $javaVersion")
  Nil
}

ThisBuild / scalacOptions ++= Seq(
  "-deprecation", // Emit warning and location for usages of deprecated APIs.
  "--release",
  "11",
  "-Xfatal-warnings",
  "-feature",
  "-Wshadow:type-parameter-shadow",
  "-no-indent",
  "-old-syntax",
  "-Wconf:msg=Implicit parameters should be provided with a `using` clause:s",
)

lazy val createDistribution = taskKey[File]("Create a complete Joern distribution")
createDistribution := {
  val distributionFile = file("target/joern-cli.zip")
  val zip              = (joerncli / Universal / packageBin).value

  IO.copyFile(zip, distributionFile)
  val querydbDistribution = (querydb / createDistribution).value

  println(s"created distribution - resulting files: $distributionFile")
  distributionFile
}

ThisBuild / resolvers ++= Seq(
  Resolver.mavenLocal,
  "Sonatype OSS" at "https://oss.sonatype.org/content/repositories/public",
  "Atlassian" at "https://packages.atlassian.com/mvn/maven-atlassian-external",
  "Gradle Releases" at "https://repo.gradle.org/gradle/libs-releases/"
)

ThisBuild / Test / fork := true

Global / onChangedBuildSource := ReloadOnSourceChanges

// This fork does NOT publish to Maven Central. Upstream Joern owns the
// `io.joern` coordinates on Sonatype; publishing from here would push fork
// artifacts under upstream's identity. Distribution is via GitHub Releases
// only (see .github/workflows/release-github.yml).
//
// To re-enable publishing, claim your own Sonatype namespace first, then
// restore `publishTo`/`sonatypeCredentialHost` and set `organization` above
// to that namespace — do not reuse `io.joern`.
ThisBuild / publishArtifact := false
ThisBuild / publish         := {}
ThisBuild / publishLocal    := {}

ThisBuild / scmInfo := Some(
  ScmInfo(
    url("https://github.com/allsmog/oxidized-joern"),
    "scm:git@github.com:allsmog/oxidized-joern.git"
  )
)
ThisBuild / homepage := Some(url("https://github.com/allsmog/oxidized-joern"))
ThisBuild / licenses := List("Apache-2.0" -> url("http://www.apache.org/licenses/LICENSE-2.0"))

publish / skip := true // don't publish the root project

ThisBuild / Test / packageBin / publishArtifact := true

// trigger an sbt reload when any `application.conf` file changes
Global / checkBuildSources / fileInputs += (baseDirectory.value.toGlob / ** / "resources" / "application.conf")
