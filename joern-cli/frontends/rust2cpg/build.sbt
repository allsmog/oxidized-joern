import com.typesafe.config.{Config, ConfigFactory}
import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.{Process, stringToProcess}
import scala.util.Try

name := "rust2cpg"

dependsOn(
  Projects.dataflowengineoss % "test->test",
  Projects.x2cpg             % "compile->compile;test->test",
  Projects.linterRules       % ScalafixConfig
)

lazy val appProperties = settingKey[Config]("App Properties")
appProperties := {
  val path            = (Compile / resourceDirectory).value / "application.conf"
  val applicationConf = ConfigFactory.parseFile(path).resolve()
  applicationConf
}

lazy val astGenVersion = settingKey[String]("rust_ast_gen version")
astGenVersion := appProperties.value.getString("rust2cpg.rust_ast_gen_version")

libraryDependencies ++= Seq(
  "io.shiftleft"  %% "codepropertygraph" % Versions.cpg,
  "com.lihaoyi"   %% "upickle"           % Versions.upickle,
  "org.scalatest" %% "scalatest"         % Versions.scalatest % Test
)

enablePlugins(JavaAppPackaging, LauncherJarPlugin)

lazy val AstgenWin      = "rust_ast_gen-win.exe"
lazy val AstgenWinArm   = "rust_ast_gen-win-arm.exe"
lazy val AstgenLinux    = "rust_ast_gen-linux"
lazy val AstgenLinuxArm = "rust_ast_gen-linux-arm"
lazy val AstgenMac      = "rust_ast_gen-macos"
lazy val AstgenMacArm   = "rust_ast_gen-macos-arm"

lazy val astGenDlUrl = settingKey[String]("rust_ast_gen download url")
astGenDlUrl := s"https://github.com/joernio/astgen-monorepo/releases/download/rust-astgen/v${astGenVersion.value}/"

def isCompatibleAstGen(command: Seq[String], astGenVersion: String): Boolean = {
  Try(Process(command).!!).toOption.map(_.strip().stripPrefix("rust_ast_gen ")) match {
    case Some(installedVersion) if installedVersion.nonEmpty =>
      versionsort.VersionHelper.compare(installedVersion, astGenVersion) >= 0
    case _ => false
  }
}

def hasCompatibleAstGenVersion(astGenVersion: String): Boolean =
  isCompatibleAstGen(Seq("rust_ast_gen", "--version"), astGenVersion)

lazy val astGenBinaryNames = taskKey[Seq[String]]("rust_ast_gen binary names")
astGenBinaryNames := {
  val bundledCurrentAstgen = baseDirectory.value / "bin" / "astgen" / astGenCurrentBinaryName.value
  if (
    hasCompatibleAstGenVersion(astGenVersion.value) ||
    isCompatibleAstGen(Seq(bundledCurrentAstgen.getAbsolutePath, "--version"), astGenVersion.value)
  ) {
    Seq.empty
  } else if (sys.props.get("ALL_PLATFORMS").contains("TRUE")) {
    Seq(AstgenWin, AstgenWinArm, AstgenLinux, AstgenLinuxArm, AstgenMac, AstgenMacArm)
  } else {
    Environment.operatingSystem match {
      case Environment.OperatingSystemType.Windows =>
        Environment.architecture match {
          case Environment.ArchitectureType.X86   => Seq(AstgenWin)
          case Environment.ArchitectureType.ARMv8 => Seq(AstgenWinArm)
        }
      case Environment.OperatingSystemType.Linux =>
        Environment.architecture match {
          case Environment.ArchitectureType.X86   => Seq(AstgenLinux)
          case Environment.ArchitectureType.ARMv8 => Seq(AstgenLinuxArm)
        }
      case Environment.OperatingSystemType.Mac =>
        Environment.architecture match {
          case Environment.ArchitectureType.X86   => Seq(AstgenMac)
          case Environment.ArchitectureType.ARMv8 => Seq(AstgenMacArm)
        }
      case Environment.OperatingSystemType.Unknown =>
        Seq(AstgenWin, AstgenWinArm, AstgenLinux, AstgenLinuxArm, AstgenMac, AstgenMacArm)
    }
  }
}

lazy val astGenCurrentBinaryName = taskKey[String]("rust_ast_gen binary name for the current host")
astGenCurrentBinaryName := {
  Environment.operatingSystem match {
    case Environment.OperatingSystemType.Windows =>
      Environment.architecture match {
        case Environment.ArchitectureType.X86   => AstgenWin
        case Environment.ArchitectureType.ARMv8 => AstgenWinArm
      }
    case Environment.OperatingSystemType.Linux =>
      Environment.architecture match {
        case Environment.ArchitectureType.X86   => AstgenLinux
        case Environment.ArchitectureType.ARMv8 => AstgenLinuxArm
      }
    case Environment.OperatingSystemType.Mac =>
      Environment.architecture match {
        case Environment.ArchitectureType.X86   => AstgenMac
        case Environment.ArchitectureType.ARMv8 => AstgenMacArm
      }
    case Environment.OperatingSystemType.Unknown =>
      AstgenLinux
  }
}

lazy val rustAstGenBuildRust = taskKey[File]("Build local Rust rust_ast_gen and install it under bin/astgen")
rustAstGenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "rust_ast_gen.exe" else "rust_ast_gen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "rust_ast_gen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }
  val built      = rustRoot / "target" / "release" / localBinaryName
  val astGenDir  = baseDirectory.value / "bin" / "astgen"
  val targetFile = astGenDir / astGenCurrentBinaryName.value
  astGenDir.mkdirs()
  IO.copyFile(built, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)
  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyFile(targetFile, distDir / astGenCurrentBinaryName.value, preserveLastModified = true)
  streams.value.log.info(s"installed Rust rust_ast_gen to $targetFile")
  targetFile
}

lazy val astGenDlTask = taskKey[Unit](s"Download rust_ast_gen binaries")
astGenDlTask := {
  val astGenDir = baseDirectory.value / "bin" / "astgen"

  astGenBinaryNames.value.foreach { fileName =>
    val file = astGenDir / fileName
    DownloadHelper.ensureIsAvailable(s"${astGenDlUrl.value}$fileName", file)
    // permissions are lost during the download; need to set them manually
    file.setExecutable(true, false)
  }

  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyDirectory(astGenDir, distDir, preserveExecutable = true)
}

Compile / compile := ((Compile / compile) dependsOn rustAstGenBuildRust).value

lazy val astGenSetAllPlatforms = taskKey[Unit](s"Set ALL_PLATFORMS")
astGenSetAllPlatforms := { System.setProperty("ALL_PLATFORMS", "TRUE") }

stage := Def
  .sequential(astGenSetAllPlatforms, Universal / stage)
  .andFinally(System.setProperty("ALL_PLATFORMS", "FALSE"))
  .value

Universal / packageName       := name.value
Universal / topLevelDirectory := None

/** write the astgen version to the manifest for downstream usage */
Compile / packageBin / packageOptions +=
  Package.ManifestAttributes(new java.util.jar.Attributes.Name("Rust-AstGen-Version") -> astGenVersion.value)
