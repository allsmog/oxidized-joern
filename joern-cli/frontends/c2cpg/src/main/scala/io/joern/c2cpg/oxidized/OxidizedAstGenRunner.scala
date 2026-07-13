package io.joern.c2cpg.oxidized

import io.joern.c2cpg.Config
import io.shiftleft.semanticcpg.utils.FileUtil
import io.shiftleft.semanticcpg.utils.ExternalCommand
import io.joern.x2cpg.utils.Environment

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Path, Paths}
import scala.jdk.CollectionConverters.*
import scala.sys.process.{Process, ProcessLogger}

object OxidizedAstGenRunner {

  private val CxxAstgenBinEnvVar = "CXXASTGEN_BIN"

  def run(config: Config): Seq[OxDocument] = {
    val outDir = Files.createTempDirectory("oxidized-cxxastgen")
    try {
      val runner = runnerCommandFor(config, outDir)
      val stdout = new StringBuilder
      val stderr = new StringBuilder
      val exitCode = Process(runner.command, runner.workingDirectory.map(_.toFile).orNull).!(
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

  private def runnerCommandFor(config: Config, outDir: Path): RunnerCommand = {
    val args = astgenArgs(config, outDir)
    configuredBinary()
      .orElse(localBundledBinary())
      .orElse(packagedBinary())
      .map(binary => RunnerCommand(binary.toString +: args, None))
      .getOrElse(RunnerCommand(cargoCommandPrefix ++ args, Some(findRustRoot())))
  }

  private def astgenArgs(config: Config, outDir: Path): Seq[String] = {
    val includeArgs = config.includePaths.toSeq.sorted.flatMap(path => Seq("-include", path))
    val defineArgs  = config.defines.toSeq.sorted.flatMap(define => Seq("-define", define))
    val compilationDbArgs =
      config.compilationDatabaseFilename.toSeq.flatMap(path => Seq("-compilation-database", path))
    val skipBodyArgs = Option.when(config.skipFunctionBodies)("-skip-function-bodies").toSeq
    val excludeArgs =
      Option(config.ignoredFilesRegex).map(_.regex).filter(_.nonEmpty).toSeq.flatMap(regex => Seq("-exclude", regex))

    includeArgs ++ defineArgs ++ compilationDbArgs ++ skipBodyArgs ++ excludeArgs ++
      Seq("-out", outDir.toString, config.inputPath)
  }

  private def cargoCommandPrefix: Seq[String] =
    Seq("cargo", "run", "--quiet", "-p", "cxxastgen", "--")

  private def configuredBinary(): Option[Path] =
    Option(System.getenv(CxxAstgenBinEnvVar)).filter(_.nonEmpty).map(Paths.get(_))

  private def localBundledBinary(): Option[Path] = {
    val cwd = Paths.get("").toAbsolutePath.normalize()
    Seq(
      cwd.resolve("joern-cli/frontends/c2cpg/bin/astgen").resolve(cxxAstgenBinaryName),
      cwd.resolve("bin/astgen").resolve(cxxAstgenBinaryName)
    ).find(Files.isRegularFile(_))
  }

  private def packagedBinary(): Option[Path] = {
    val packagePath = Paths.get(getClass.getProtectionDomain.getCodeSource.getLocation.toURI)
    val binary      = ExternalCommand.executableDir(packagePath).resolve("astgen").resolve(cxxAstgenBinaryName)
    Option.when(Files.isRegularFile(binary))(binary)
  }

  private def cxxAstgenBinaryName: String =
    (Environment.operatingSystem, Environment.architecture) match {
      case (Environment.OperatingSystemType.Windows, _)                                => "cxxastgen-win.exe"
      case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)   => "cxxastgen-linux"
      case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8) => "cxxastgen-linux-arm64"
      case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)     => "cxxastgen-macos"
      case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)   => "cxxastgen-macos-arm64"
      case _                                                                           => "cxxastgen-linux"
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

  private case class RunnerCommand(command: Seq[String], workingDirectory: Option[Path])

}
