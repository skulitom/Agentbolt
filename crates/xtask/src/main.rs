//! Release packaging, without publishing or modifying an installed Agentbolt.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::write::SimpleFileOptions;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "https://github.com/skulitom/Agentbolt";
const SCHEMA: &str = "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json";

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("check-tag") && args.len() == 2 {
        ensure!(args[1] == format!("v{VERSION}"), "tag must be v{VERSION}");
        return Ok(());
    }
    if args.len() != 4 || args[0] != "package" {
        bail!(
            "Usage: cargo run -p xtask -- package <windows-x64|linux-x64> <notices.md> <new-output-folder>\n       cargo run -p xtask -- check-tag <tag>"
        );
    }
    let target = args[1].as_str();
    ensure!(
        matches!(target, "windows-x64" | "linux-x64"),
        "unsupported target"
    );
    ensure!(
        cfg!(target_arch = "x86_64")
            && ((cfg!(windows) && target == "windows-x64")
                || (cfg!(target_os = "linux") && target == "linux-x64")),
        "package on the matching x64 host, where the CLI version can be checked"
    );
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    ensure!(
        git(&root, &["status", "--porcelain"])?.is_empty(),
        "commit the release sources before packaging"
    );
    let revision = git(&root, &["rev-parse", "HEAD"])?;
    let output = Path::new(&args[3]);
    ensure!(!output.exists(), "output folder must not already exist");
    let notices = fs::read(&args[2]).context("read cargo-about notices")?;
    ensure!(
        notices.len() > 100 && String::from_utf8_lossy(&notices).contains("License"),
        "license notices are missing or empty"
    );
    let programs: &[&str] = if target == "windows-x64" {
        &[
            "agentbolt.exe",
            "agentbolt-daemon.exe",
            "agentbolt-desk.exe",
        ]
    } else {
        &["agentbolt"]
    };
    let bins = build_programs(&root, target, programs)?;
    ensure!(
        git(&root, &["status", "--porcelain"])?.is_empty()
            && git(&root, &["rev-parse", "HEAD"])? == revision,
        "sources changed during the build; retry from a clean checkout"
    );
    let cli_version = Command::new(&bins[programs[0]]).arg("--version").output()?;
    ensure!(
        cli_version.status.success()
            && String::from_utf8_lossy(&cli_version.stdout).trim()
                == format!("agentbolt {VERSION}"),
        "CLI does not match release version {VERSION}; rebuild first"
    );
    let mut files = BTreeMap::new();
    let mut binary_hashes = BTreeMap::new();
    for name in programs {
        let bytes = fs::read(&bins[*name]).with_context(|| format!("read {name}"))?;
        binary_hashes.insert(*name, hash(&bytes));
        files.insert((*name).into(), bytes);
    }
    files.insert("LICENSE".into(), fs::read(root.join("LICENSE"))?);
    files.insert("README.md".into(), fs::read(root.join("README.md"))?);
    files.insert("THIRD_PARTY_NOTICES.md".into(), notices);
    files.insert(
        "release.json".into(),
        pretty(&json!({
            "name": "Agentbolt", "version": VERSION, "target": target,
            "revision": revision, "codeSigned": false, "binarySha256": binary_hashes,
            "linuxSupport": "CLI/MCP discovery and tests; the installed Desk requires Windows"
        }))?,
    );
    let zip_bytes = archive(&files, programs)?;
    let archive_name = format!("agentbolt-{VERSION}-{target}.zip");
    let digest = hash(&zip_bytes);
    let mut artifacts = BTreeMap::new();
    artifacts.insert(archive_name.clone(), zip_bytes);
    artifacts.insert("server.json".into(), pretty(&registry_manifest())?);
    if target == "windows-x64" {
        let url = format!("{REPO}/releases/download/v{VERSION}/{archive_name}");
        artifacts.insert(
            "scoop/agentbolt.json".into(),
            pretty(&scoop_manifest(&url, &digest))?,
        );
        // JSON is a subset of YAML; winget accepts this and the generator needs no YAML serializer.
        artifacts.insert(
            "winget/Skulitom.Agentbolt.yaml".into(),
            pretty(&winget_manifest(&url, &digest))?,
        );
    }
    artifacts.insert("SHA256SUMS".into(), checksums(&artifacts).into_bytes());
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    // Never replace a candidate somebody may already be reviewing.
    fs::create_dir(output).context("output folder must not already exist")?;
    for (name, bytes) in artifacts {
        let path = output.join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, bytes)?;
    }
    println!(
        "Prepared unsigned {target} candidate {VERSION} from {revision} in {}",
        output.display()
    );
    Ok(())
}

/// Cargo verifies freshness against the locked source graph and reports each real output path.
/// Never stamp a caller-supplied folder of old binaries with the current source revision.
fn build_programs(
    root: &Path,
    target: &str,
    programs: &[&str],
) -> Result<BTreeMap<String, PathBuf>> {
    let triple = if target == "windows-x64" {
        "x86_64-pc-windows-msvc"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    let mut command = Command::new("cargo");
    command.current_dir(root).args([
        "build",
        "--release",
        "--locked",
        "--bins",
        "--target",
        triple,
        "--message-format=json",
    ]);
    for name in programs {
        command.args(["-p", name.trim_end_matches(".exe")]);
    }
    let output = command.stderr(std::process::Stdio::inherit()).output()?;
    ensure!(output.status.success(), "release build failed");
    let mut bins = BTreeMap::new();
    for line in String::from_utf8(output.stdout)?.lines() {
        let message: Value = serde_json::from_str(line)?;
        if message["reason"] == "compiler-artifact"
            && message["target"]["kind"] == json!(["bin"])
            && let Some(path) = message["executable"].as_str()
        {
            let path = PathBuf::from(path);
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && programs.contains(&name)
            {
                ensure!(
                    message["target"]["name"] == name.trim_end_matches(".exe"),
                    "unexpected Cargo binary target"
                );
                ensure!(
                    bins.insert(name.to_owned(), path).is_none(),
                    "duplicate Cargo binary output"
                );
            }
        }
    }
    ensure!(
        bins.len() == programs.len(),
        "Cargo did not report every expected release program"
    );
    Ok(bins)
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn pretty(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn archive(files: &BTreeMap<String, Vec<u8>>, executables: &[&str]) -> Result<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        ensure!(
            !name.contains(['/', '\\', ':']) && name != "." && name != "..",
            "archive entries must be flat file names"
        );
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default())
            .unix_permissions(if executables.contains(&name.as_str()) {
                0o755
            } else {
                0o644
            });
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn checksums(files: &BTreeMap<String, Vec<u8>>) -> String {
    files
        .iter()
        .map(|(name, bytes)| format!("{}  {name}\n", hash(bytes)))
        .collect()
}

fn registry_manifest() -> Value {
    // The Registry supports custom installation guides for desktop-embedded servers. This does
    // not claim a package is published to Cargo, or that the Windows Desk runs on Linux.
    json!({
        "$schema": SCHEMA, "name": "io.github.skulitom/agentbolt", "title": "Agentbolt",
        "description": "A local Windows desk where agents ask questions and read durable human answers through MCP.",
        "version": VERSION, "repository": {"url": REPO, "source": "github"},
        "websiteUrl": format!("{REPO}#install")
    })
}

fn winget_manifest(url: &str, digest: &str) -> Value {
    json!({
        "PackageIdentifier": "Skulitom.Agentbolt", "PackageVersion": VERSION,
        "PackageLocale": "en-US", "Publisher": "Artem Skulimovskiy",
        "PublisherUrl": "https://github.com/skulitom", "PackageName": "Agentbolt",
        "PackageUrl": REPO, "License": "MIT", "LicenseUrl": format!("{REPO}/blob/v{VERSION}/LICENSE"),
        "ShortDescription": "A local desk where agents ask questions and receive durable human answers.",
        "Tags": ["mcp", "ai", "agents"], "Moniker": "agentbolt",
        "InstallerType": "zip", "NestedInstallerType": "exe",
        "NestedInstallerFiles": [{"RelativeFilePath": "agentbolt.exe"}],
        "Scope": "user", "InstallModes": ["interactive", "silent", "silentWithProgress"],
        "InstallerSwitches": {"Silent": "install", "SilentWithProgress": "install", "Interactive": "install"},
        "UpgradeBehavior": "install", "ElevationRequirement": "elevationProhibited",
        "AppsAndFeaturesEntries": [{"DisplayName": "Agentbolt", "Publisher": "Artem Skulimovskiy", "DisplayVersion": VERSION, "ProductCode": "Skulitom.Agentbolt", "InstallerType": "exe"}],
        "Installers": [{"Architecture": "x64", "InstallerUrl": url, "InstallerSha256": digest.to_ascii_uppercase()}],
        "ManifestType": "singleton", "ManifestVersion": "1.12.0"
    })
}

fn scoop_manifest(url: &str, digest: &str) -> Value {
    json!({
        "version": VERSION, "description": "A local desk where agents ask questions and receive durable human answers.",
        "homepage": REPO, "license": "MIT",
        "architecture": {"64bit": {"url": url, "hash": digest}},
        "pre_install": "if ($global) { throw 'Agentbolt installs for the current user; omit --global.' }",
        "post_install": [
            "& \"$dir\\agentbolt.exe\" install",
            "if ($LASTEXITCODE -ne 0) { throw 'Agentbolt installation failed. The previous installation was preserved where possible; see the installer output.' }"
        ],
        "pre_uninstall": [
            "if ($cmd -ne 'update') {",
            "    & \"$dir\\agentbolt.exe\" uninstall",
            "    if ($LASTEXITCODE -ne 0) { throw 'Agentbolt uninstall failed; see the installer output.' }",
            "}"
        ],
        "notes": [
            "Open Agentbolt from the Start menu. Questions and answers are retained on uninstall.",
            "To view MCP setup commands: & \"$env:LOCALAPPDATA\\Programs\\Agentbolt\\agentbolt.exe\" clients",
            "Use that installed executable for MCP and status; the extracted Scoop copy is only the installer.",
            "Version 0.2 is unsigned."
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn archive_round_trip_is_reproducible_and_keeps_linux_executable_mode() {
        let files = BTreeMap::from([
            ("agentbolt".into(), b"binary".to_vec()),
            ("LICENSE".into(), b"license".to_vec()),
        ]);
        let first = archive(&files, &["agentbolt"]).unwrap();
        assert_eq!(first, archive(&files, &["agentbolt"]).unwrap());
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(first)).unwrap();
        assert_eq!(zip.len(), 2);
        for (name, expected) in files {
            let mut file = zip.by_name(&name).unwrap();
            assert_eq!(
                file.unix_mode().unwrap() & 0o777,
                if name == "agentbolt" { 0o755 } else { 0o644 }
            );
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, expected);
        }
        assert!(archive(&BTreeMap::from([("../outside".into(), vec![])]), &[]).is_err());
    }

    #[test]
    fn manifests_reference_the_actual_archive_digest_and_preserve_data_on_removal() {
        let digest = hash(b"real archive bytes");
        let url =
            format!("{REPO}/releases/download/v{VERSION}/agentbolt-{VERSION}-windows-x64.zip");
        let winget = winget_manifest(&url, &digest);
        let scoop = scoop_manifest(&url, &digest);
        assert_eq!(
            winget["Installers"][0]["InstallerSha256"],
            digest.to_ascii_uppercase()
        );
        assert_eq!(scoop["architecture"]["64bit"]["hash"], digest);
        assert_eq!(
            winget["Installers"][0]["InstallerUrl"],
            scoop["architecture"]["64bit"]["url"]
        );
        assert!(
            scoop.get("bin").is_none(),
            "an extracted MCP binary fails sibling-daemon verification"
        );
        assert!(!scoop.to_string().contains("--purge"));
        assert!(
            scoop["pre_uninstall"][0]
                .as_str()
                .unwrap()
                .contains("$cmd -ne 'update'")
        );
        assert!(
            winget["InstallerSwitches"].get("Upgrade").is_none(),
            "upgrade appends switches, so it must not duplicate install"
        );
        assert_eq!(
            checksums(&BTreeMap::from([("x".into(), b"abc".to_vec())])),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  x\n"
        );
    }
}
