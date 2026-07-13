import com.typesafe.config.{Config, ConfigFactory}
import com.typesafe.sbt.packager.Keys.stagingDirectory

import scala.sys.process.Process

name := "abap2cpg"

dependsOn(
  Projects.dataflowengineoss % "compile->compile;test->test",
  Projects.x2cpg             % "compile->compile;test->test",
  Projects.linterRules       % ScalafixConfig
)

libraryDependencies ++= Seq(
  "io.shiftleft"  %% "codepropertygraph" % Versions.cpg,
  "com.lihaoyi"   %% "ujson"             % "4.1.0",
  "org.scalatest" %% "scalatest"         % Versions.scalatest % Test
)

enablePlugins(JavaAppPackaging, LauncherJarPlugin)

lazy val appProperties = settingKey[Config]("App Properties")
appProperties := {
  val path            = (Compile / resourceDirectory).value / "application.conf"
  val applicationConf = ConfigFactory.parseFile(path).resolve()
  applicationConf
}

lazy val abapgenVersion = settingKey[String]("abapgen version")
abapgenVersion := appProperties.value.getString("abap2cpg.abapgen_version")

// Released binary names (produced by the abap-astgen-release.yml workflow in
// joernio/astgen-monorepo after pkg output is renamed).
lazy val AbapgenWinX86   = "abapgen-win.exe"
lazy val AbapgenLinuxX86 = "abapgen-linux"
lazy val AbapgenLinuxArm = "abapgen-linux-arm"
lazy val AbapgenMacX86   = "abapgen-macos"
lazy val AbapgenMacArm   = "abapgen-macos-arm"

lazy val abapgenCurrentBinaryName = taskKey[String]("abapgen binary name for the current host")
abapgenCurrentBinaryName := {
  (Environment.operatingSystem, Environment.architecture) match {
    case (Environment.OperatingSystemType.Windows, _)                                => AbapgenWinX86
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)   => AbapgenLinuxX86
    case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8) => AbapgenLinuxArm
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)     => AbapgenMacX86
    case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)   => AbapgenMacArm
    case _                                                                           => AbapgenLinuxX86
  }
}

lazy val abapgenDlUrl = settingKey[String]("abapgen download url")
abapgenDlUrl := s"https://github.com/joernio/astgen-monorepo/releases/download/abap-astgen/v${abapgenVersion.value}/"

lazy val abapgenBinaryNames = taskKey[Seq[String]]("abapgen binary names for current or all platforms")
abapgenBinaryNames := {
  if (sys.props.get("ALL_PLATFORMS").contains("TRUE")) {
    Seq(AbapgenWinX86, AbapgenLinuxX86, AbapgenLinuxArm, AbapgenMacX86, AbapgenMacArm)
  } else {
    (Environment.operatingSystem, Environment.architecture) match {
      case (Environment.OperatingSystemType.Windows, _)                                => Seq(AbapgenWinX86)
      case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.X86)   => Seq(AbapgenLinuxX86)
      case (Environment.OperatingSystemType.Linux, Environment.ArchitectureType.ARMv8) => Seq(AbapgenLinuxArm)
      case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.X86)     => Seq(AbapgenMacX86)
      case (Environment.OperatingSystemType.Mac, Environment.ArchitectureType.ARMv8)   => Seq(AbapgenMacArm)
      case _ => Seq(AbapgenWinX86, AbapgenLinuxX86, AbapgenLinuxArm, AbapgenMacX86, AbapgenMacArm)
    }
  }
}

lazy val abapgenBuildRust = taskKey[File]("Build local Rust abapgen and install it under bin/astgen")
abapgenBuildRust := {
  val rustRoot = baseDirectory.value / "rust"
  val localBinaryName =
    if (Environment.operatingSystem == Environment.OperatingSystemType.Windows) "abapgen.exe" else "abapgen"
  val exitCode = Process(Seq("cargo", "build", "--release", "--bin", "abapgen"), rustRoot).!
  if (exitCode != 0) {
    sys.error(s"cargo build failed with exit code $exitCode")
  }

  val builtBinary = rustRoot / "target" / "release" / localBinaryName
  val astGenDir   = baseDirectory.value / "bin" / "astgen"
  val targetFile  = astGenDir / abapgenCurrentBinaryName.value
  astGenDir.mkdirs()
  IO.copyFile(builtBinary, targetFile, preserveLastModified = true)
  targetFile.setExecutable(true, false)
  streams.value.log.info(s"installed Rust abapgen to $targetFile")
  targetFile
}

lazy val abapgenDlTask = taskKey[Unit]("Download abapgen binaries from joernio/astgen-monorepo release")
abapgenDlTask := {
  val astGenDir = baseDirectory.value / "bin" / "astgen"

  abapgenBinaryNames.value.foreach { fileName =>
    val file = astGenDir / fileName
    DownloadHelper.ensureIsAvailable(s"${abapgenDlUrl.value}$fileName", file)
    // permissions are lost during the download; need to set them manually
    file.setExecutable(true, false)
  }

  val distDir = (Universal / stagingDirectory).value / "bin" / "astgen"
  distDir.mkdirs()
  IO.copyDirectory(astGenDir, distDir, preserveExecutable = true)
}

Compile / compile := ((Compile / compile) dependsOn abapgenBuildRust).value

lazy val abapgenSetAllPlatforms = taskKey[Unit]("Set ALL_PLATFORMS flag")
abapgenSetAllPlatforms := { System.setProperty("ALL_PLATFORMS", "TRUE") }

stage := Def
  .sequential(abapgenSetAllPlatforms, Universal / stage)
  .andFinally(System.setProperty("ALL_PLATFORMS", "FALSE"))
  .value

Universal / packageName       := name.value
Universal / topLevelDirectory := None

/** write the abapgen version to the manifest for downstream usage */
Compile / packageBin / packageOptions +=
  Package.ManifestAttributes(new java.util.jar.Attributes.Name("Abap-AstGen-Version") -> abapgenVersion.value)
