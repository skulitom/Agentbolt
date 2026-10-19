//! Release packaging, without publishing or modifying an installed Agentbolt.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
    if args.first().map(String::as_str) == Some("check-notices") && args.len() == 2 {
        let report = serde_json::from_slice(&fs::read(&args[1])?)?;
        check_notices(&report)?;
        println!("Every reported crate has nonempty notice text from a source file.");
        return Ok(());
    }
    if args.len() != 4 || args[0] != "package" {
        bail!(
            "Usage: cargo run -p xtask -- package <windows-x64|linux-x64> <notices.md> <new-output-folder>\n       cargo run -p xtask -- check-tag <tag>\n       cargo run -p xtask -- check-notices <cargo-about-report.json>"
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
    files.insert("NEXT.md".into(), fs::read(root.join("NEXT.md"))?);
    files.insert("AGENTS.md".into(), fs::read(root.join("AGENTS.md"))?);
    for path in git(&root, &["ls-files", "--", "docs"])?
        .lines()
        .filter(|p| p.ends_with(".md") || p.ends_with(".json"))
    {
        files.insert(path.to_owned(), fs::read(root.join(path))?);
    }
    files.insert("THIRD_PARTY_NOTICES.md".into(), notices);
    files.insert(
        "release.json".into(),
        pretty(&json!({
            "name": "Agentbolt", "version": VERSION, "target": target,
            "revision": revision, "codeSigned": false, "binarySha256": binary_hashes,
            "linuxSupport": "CLI/MCP discovery, tools and SSH client; the installed Desk requires Windows"
        }))?,
    );
    let zip_bytes = archive(&files, programs)?;
    let archive_name = format!("agentbolt-{VERSION}-{target}.zip");
    let digest = hash(&zip_bytes);
    let mut artifacts = BTreeMap::new();
    artifacts.insert(archive_name.clone(), zip_bytes);
    artifacts.insert("server.json".into(), pretty(&registry_manifest())?);
    if target == "windows-x64" {
        let plugin_manifest: Value = serde_json::from_slice(&fs::read(
            root.join("plugins/agentbolt/.claude-plugin/plugin.json"),
        )?)?;
        ensure!(
            plugin_manifest["version"] == VERSION,
            "plugin version must match the release"
        );
        let mut plugin_files = BTreeMap::new();
        for path in git(&root, &["ls-files", "--", "plugins/agentbolt"])?.lines() {
            let name = path
                .strip_prefix("plugins/agentbolt/")
                .context("unexpected plugin path")?;
            let mut bytes = fs::read(root.join(path))?;
            if name == "README.md" {
                // The standalone plugin ZIP has no repository parent. Keep its documentation links
                // useful and tied to the same source revision as the release binaries.
                bytes = String::from_utf8(bytes)?
                    .replace("](../../docs/", &format!("]({REPO}/blob/{revision}/docs/"))
                    .into_bytes();
            }
            plugin_files.insert(name.to_owned(), bytes);
        }
        ensure!(
            plugin_files.contains_key(".claude-plugin/plugin.json"),
            "plugin sources must be committed before packaging"
        );
        plugin_files.insert("LICENSE".into(), fs::read(root.join("LICENSE"))?);
        artifacts.insert(
            format!("agentbolt-{VERSION}-claude-plugin.zip"),
            archive(&plugin_files, &[])?,
        );
        let url = format!("{REPO}/releases/download/v{VERSION}/{archive_name}");
        artifacts.insert(
            "scoop/agentbolt.json".into(),
            pretty(&scoop_manifest(&url, &digest))?,
        );
        // JSON is a subset of YAML; winget accepts this and the generator needs no YAML serializer.
        artifacts.insert(
            "winget/Skulitom.Agentbolt.yaml".into(),
            [b"# yaml-language-server: $schema=https://aka.ms/winget-manifest.singleton.1.12.0.schema.json\n".as_slice(), &pretty(&winget_manifest(&url, &digest))?].concat(),
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

/// cargo-about can succeed with generic SPDX text when a package omits its license file.
/// Release notices must use real source text and cover every crate in its selected graph.
fn check_notices(report: &Value) -> Result<()> {
    let crates = report["crates"]
        .as_array()
        .context("notice report has no crates")?;
    let licenses = report["licenses"]
        .as_array()
        .context("notice report has no licenses")?;
    ensure!(
        !crates.is_empty() && !licenses.is_empty(),
        "notice report is empty"
    );
    let mut required = BTreeSet::new();
    for entry in crates {
        let id = entry["package"]["id"]
            .as_str()
            .filter(|id| !id.trim().is_empty())
            .context("notice report has an unidentified crate")?;
        required.insert(id);
    }
    let mut covered = BTreeSet::new();
    for entry in licenses {
        let name = entry["id"].as_str().unwrap_or("unknown license");
        ensure!(
            entry["text"].as_str().is_some_and(|s| !s.trim().is_empty()),
            "{name}: notice text is empty"
        );
        ensure!(
            entry["source_path"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty()),
            "{name}: no source file; replace generic SPDX fallback with verified source text"
        );
        let users = entry["used_by"]
            .as_array()
            .context("notice has no crate references")?;
        ensure!(!users.is_empty(), "{name}: notice has no crate references");
        for user in users {
            let id = user["crate"]["id"]
                .as_str()
                .context("notice has an unidentified crate reference")?;
            ensure!(
                required.contains(id),
                "notice refers to an unreported crate: {id}"
            );
            covered.insert(id);
        }
    }
    ensure!(
        required == covered,
        "some reported crates have no source-backed notice: {:?}",
        required.difference(&covered).collect::<Vec<_>>()
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
            !name.contains(['\\', ':'])
                && name
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
            "archive entries must be safe relative paths"
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
        "description": "A Windows desk for human answers, agent handoffs and work holds, with phone and SSH access.",
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
        "ShortDescription": "A local desk for human answers, agent handoffs and work-overlap checks.",
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
        "version": VERSION, "description": "A local desk for human answers, agent handoffs and work-overlap checks.",
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
    fn notices_refuse_generic_text_empty_sources_and_uncovered_crates() {
        let valid = json!({
            "crates": [{"package": {"id": "crate-a"}}, {"package": {"id": "crate-b"}}],
            "licenses": [{"id": "MIT", "text": "Upstream license text", "source_path": "LICENSE",
                "used_by": [{"crate": {"id": "crate-a"}}, {"crate": {"id": "crate-b"}}]}]
        });
        check_notices(&valid).unwrap();
        for missing in [Value::Null, json!(""), json!(" \n ")] {
            for field in ["source_path", "text"] {
                let mut invalid = valid.clone();
                invalid["licenses"][0][field] = missing.clone();
                assert!(check_notices(&invalid).is_err(), "{field}: {missing}");
            }
        }
        let mut uncovered = valid.clone();
        uncovered["licenses"][0]["used_by"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(check_notices(&uncovered).is_err());
        let mut unknown = valid.clone();
        unknown["licenses"][0]["used_by"][0]["crate"]["id"] = json!("crate-c");
        assert!(check_notices(&unknown).is_err());
        assert!(check_notices(&json!({"crates": [], "licenses": []})).is_err());
    }

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
        for name in [
            "/absolute",
            "C:/absolute",
            "a/../outside",
            "a//file",
            "a\\file",
            ".",
            "a/.",
        ] {
            assert!(
                archive(&BTreeMap::from([(name.into(), vec![])]), &[]).is_err(),
                "{name}"
            );
        }
        let nested = archive(
            &BTreeMap::from([("skills/ask/SKILL.md".into(), b"skill".to_vec())]),
            &[],
        )
        .unwrap();
        assert_eq!(
            zip::ZipArchive::new(std::io::Cursor::new(nested))
                .unwrap()
                .by_name("skills/ask/SKILL.md")
                .unwrap()
                .size(),
            5
        );
    }

    #[test]
    fn manifests_reference_the_actual_archive_digest_and_preserve_data_on_removal() {
        let registry = registry_manifest();
        assert!(
            registry["description"].as_str().unwrap().chars().count() <= 100,
            "the MCP Registry schema limits descriptions to 100 characters"
        );
        assert_eq!(registry["version"], VERSION);
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
