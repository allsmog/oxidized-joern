package io.joern.c2cpg.oxidized

import io.joern.c2cpg.Config
import io.shiftleft.semanticcpg.utils.FileUtil

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path, Paths}
import scala.jdk.CollectionConverters.*
import scala.sys.process.{Process, ProcessLogger}

object OxidizedAstGenRunner {

  def run(config: Config): Seq[OxDocument] = {
    val outDir = Files.createTempDirectory("oxidized-cxxastgen")
    try {
      val rustRoot = findRustRoot()
      val command  = commandFor(config, outDir)
      val stdout   = new StringBuilder
      val stderr   = new StringBuilder
      val exitCode = Process(command, rustRoot.toFile).!(
        ProcessLogger(
          line => stdout.append(line).append(System.lineSeparator()),
          line => stderr.append(line).append(System.lineSeparator())
        )
      )
      if (exitCode != 0) {
        throw new RuntimeException(s"cxxastgen failed with exit code $exitCode\n$stderr$stdout")
      }
      Files
        .walk(outDir)
        .iterator()
        .asScala
        .filter(path => Files.isRegularFile(path) && path.toString.endsWith(".json"))
        .toSeq
        .sortBy(_.toString)
        .map(path => OxDocument.fromJson(Files.readString(path, StandardCharsets.UTF_8)))
    } finally {
      FileUtil.delete(outDir)
    }
  }

  private def commandFor(config: Config, outDir: Path): Seq[String] = {
    val includeArgs = config.includePaths.toSeq.sorted.flatMap(path => Seq("-include", path))
    val defineArgs  = config.defines.toSeq.sorted.flatMap(define => Seq("-define", define))
    val compilationDbArgs =
      config.compilationDatabaseFilename.toSeq.flatMap(path => Seq("-compilation-database", path))
    val skipBodyArgs = Option.when(config.skipFunctionBodies)("-skip-function-bodies").toSeq
    val excludeArgs =
      Option(config.ignoredFilesRegex).map(_.regex).filter(_.nonEmpty).toSeq.flatMap(regex => Seq("-exclude", regex))

    Seq("cargo", "run", "--quiet", "-p", "cxxastgen", "--") ++
      includeArgs ++ defineArgs ++ compilationDbArgs ++ skipBodyArgs ++ excludeArgs ++
      Seq("-out", outDir.toString, config.inputPath)
  }

  private def findRustRoot(): Path = {
    val cwd        = Paths.get("").toAbsolutePath.normalize()
    val candidates = Seq(cwd.resolve("joern-cli/frontends/c2cpg/rust"), cwd.resolve("rust"), cwd)
    candidates
      .find(path =>
        Files.isRegularFile(path.resolve("Cargo.toml")) && Files.isDirectory(path.resolve("crates/cxxastgen-cli"))
      )
      .getOrElse {
        throw new RuntimeException(
          "Could not find joern-cli/frontends/c2cpg/rust. Run the oxidized c2cpg backend from the repository root."
        )
      }
  }

}
