use crate::error::AppError;
use std::path::PathBuf;

const GITHUB_REPO: &str = "Edison-Watch/desktest";

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn asset_suffix() -> Result<&'static str, AppError> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (arch, os) {
        ("aarch64", "macos") => Ok("aarch64-apple-darwin.tar.gz"),
        ("x86_64", "macos") => Ok("x86_64-apple-darwin.tar.gz"),
        ("aarch64", "linux") => Ok("aarch64-unknown-linux-gnu.tar.gz"),
        ("x86_64", "linux") => Ok("x86_64-unknown-linux-gnu.tar.gz"),
        _ => Err(AppError::Config(format!(
            "unsupported platform: {arch}-{os}"
        ))),
    }
}

fn parse_version(tag: &str) -> Option<(u32, u32, u32)> {
    let v = tag.strip_prefix('v').unwrap_or(tag);
    // Strip pre-release suffix (e.g. "0.9.3-dev" → "0.9.3")
    let v = v.split('-').next().unwrap_or(v);
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

fn has_prerelease(tag: &str) -> bool {
    let v = tag.strip_prefix('v').unwrap_or(tag);
    v.contains('-')
}

fn is_newer(latest_tag: &str, current: &str) -> bool {
    match (parse_version(latest_tag), parse_version(current)) {
        (Some(l), Some(c)) => {
            if l > c {
                return true;
            }
            // Pre-release current (e.g. "0.9.3-dev") is older than same stable version
            l == c && has_prerelease(current) && !has_prerelease(latest_tag)
        }
        // Cannot parse either version — allow update rather than silently suppressing
        (None, _) | (_, None) => true,
    }
}

/// Parse SHA256SUMS.txt content and find the expected hash for the given asset name.
fn find_expected_sha256(sums_text: &str, asset_name: &str) -> Option<String> {
    for line in sums_text.lines() {
        // Format: "<hash>  <filename>" or "<hash> <filename>"
        let mut parts = line.split_whitespace();
        if let (Some(hash), Some(name)) = (parts.next(), parts.next()) {
            if name == asset_name {
                return Some(hash.to_lowercase());
            }
        }
    }
    None
}

/// Compute SHA-256 hex digest of the given bytes.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub async fn run_update(force: bool) -> Result<(), AppError> {
    let current = current_version();
    println!("Current version: v{current}");

    let client = reqwest::Client::builder()
        .user_agent("desktest-updater")
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::Infra(format!("failed to build HTTP client: {e}")))?;

    // Optional: use GITHUB_TOKEN to avoid rate limits (60/hr unauthenticated → 5000/hr)
    let github_token = std::env::var("GITHUB_TOKEN").ok();

    println!("Checking for latest release...");
    let mut req = client.get(format!(
        "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
    ));
    if let Some(ref token) = github_token {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|e| AppError::Infra(format!("failed to fetch latest release: {e}")))?;
    if let Err(e) = response.error_for_status_ref() {
        let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
        if status == 403 || status == 429 {
            return Err(AppError::Infra(format!(
                "GitHub API rate limit exceeded. Set GITHUB_TOKEN env var to raise the limit (60/hr → 5000/hr): {e}"
            )));
        }
        return Err(AppError::Infra(format!("failed to fetch latest release: {e}")));
    }
    let release: Release = response
        .json()
        .await
        .map_err(|e| AppError::Infra(format!("failed to parse release response: {e}")))?;

    let latest = &release.tag_name;
    println!("Latest release:  {latest}");

    if !force && !is_newer(latest, current) {
        println!("Already up to date.");
        return Ok(());
    }

    if force && !is_newer(latest, current) {
        println!(
            "Warning: re-installing {latest} (current: v{current})"
        );
    }

    let suffix = asset_suffix()?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(suffix))
        .ok_or_else(|| {
            AppError::Infra(format!(
                "no release asset found for this platform ({suffix})"
            ))
        })?;

    // Download SHA256SUMS for integrity verification
    let checksums_asset = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt");

    // sums_available: true if SHA256SUMS.txt exists in the release
    let (expected_hash, sums_available) = if let Some(sums_asset) = checksums_asset {
        let mut req = client.get(&sums_asset.browser_download_url);
        if let Some(ref token) = github_token {
            req = req.bearer_auth(token);
        }
        let sums_text = req
            .send()
            .await
            .map_err(|e| AppError::Infra(format!("failed to download checksums: {e}")))?
            .error_for_status()
            .map_err(|e| AppError::Infra(format!("failed to download checksums: {e}")))?
            .text()
            .await
            .map_err(|e| AppError::Infra(format!("failed to read checksums: {e}")))?;
        let hash = find_expected_sha256(&sums_text, &asset.name);
        if hash.is_none() {
            return Err(AppError::Infra(format!(
                "SHA256SUMS.txt found but has no entry for '{}'; release may be incomplete or tampered",
                asset.name
            )));
        }
        (hash, true)
    } else {
        (None, false)
    };

    println!("Downloading {}...", asset.name);
    let mut req = client.get(&asset.browser_download_url);
    if let Some(ref token) = github_token {
        req = req.bearer_auth(token);
    }
    let bytes = req
        .send()
        .await
        .map_err(|e| AppError::Infra(format!("failed to download asset: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Infra(format!("failed to download asset: {e}")))?
        .bytes()
        .await
        .map_err(|e| AppError::Infra(format!("failed to read asset bytes: {e}")))?;

    // Verify checksum if available
    if let Some(expected) = &expected_hash {
        print!("Verifying SHA-256 checksum...");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let actual = sha256_hex(&bytes);
        if actual != *expected {
            println!(" FAILED");
            return Err(AppError::Infra(format!(
                "checksum mismatch: expected {expected}, got {actual}"
            )));
        }
        println!(" OK");
    } else if !sums_available {
        println!("Warning: SHA256SUMS.txt not found in release, skipping integrity check");
    }

    // Extract the binary from the tarball
    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(&bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut binary_data: Option<Vec<u8>> = None;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        if path.file_name().and_then(|n| n.to_str()) == Some("desktest") {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf)?;
            binary_data = Some(buf);
            break;
        }
    }

    let binary_data =
        binary_data.ok_or_else(|| AppError::Infra("'desktest' binary not found in archive".into()))?;

    // Replace the current executable
    let current_exe = std::env::current_exe()
        .map_err(|e| AppError::Infra(format!("cannot determine current executable path: {e}")))?;
    let current_exe = resolve_symlinks(&current_exe)?;

    let tmp_path = {
        let mut name = current_exe
            .file_name()
            .ok_or_else(|| AppError::Infra("cannot determine executable filename".into()))?
            .to_os_string();
        name.push(".update-tmp");
        current_exe.with_file_name(name)
    };

    // Write new binary to temp file
    std::fs::write(&tmp_path, &binary_data).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        AppError::Io(e)
    })?;

    // Set executable permissions (clean up temp on failure)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
        {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(AppError::Io(e));
        }
    }

    // Atomic rename
    std::fs::rename(&tmp_path, &current_exe).map_err(|e| {
        // Clean up temp file on failure
        let _ = std::fs::remove_file(&tmp_path);
        AppError::Infra(format!("failed to replace binary: {e}"))
    })?;

    println!("Updated to {latest} ({})", current_exe.display());
    Ok(())
}

/// Resolve symlinks to find the actual binary path.
fn resolve_symlinks(path: &std::path::Path) -> Result<PathBuf, AppError> {
    std::fs::canonicalize(path)
        .map_err(|e| AppError::Infra(format!("cannot resolve executable path: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("0.9.1"), Some((0, 9, 1)));
        assert_eq!(parse_version("v0.10.0"), Some((0, 10, 0)));
        assert_eq!(parse_version("bad"), None);
        // Pre-release suffixes are stripped
        assert_eq!(parse_version("0.9.3-dev"), Some((0, 9, 3)));
        assert_eq!(parse_version("v1.0.0-beta.1"), Some((1, 0, 0)));
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("v1.0.0", "0.9.1"));
        assert!(is_newer("v0.10.0", "0.9.1"));
        assert!(is_newer("v0.9.2", "0.9.1"));
        assert!(!is_newer("v0.9.1", "0.9.1"));
        assert!(!is_newer("v0.9.0", "0.9.1"));
        // Pre-release current version should detect newer stable release
        assert!(is_newer("v0.9.4", "0.9.3-dev"));
        // Pre-release current should update to same stable version
        assert!(is_newer("v0.9.3", "0.9.3-dev"));
        // Unparseable latest tag should allow update
        assert!(is_newer("nightly-2025-01-01", "0.9.1"));
        // Unparseable current version should allow update
        assert!(is_newer("v0.9.1", "custom-build"));
    }

    #[test]
    fn test_find_expected_sha256() {
        let sums = "abc123  desktest-v0.9.1-aarch64-apple-darwin.tar.gz\ndef456  desktest-v0.9.1-x86_64-unknown-linux-gnu.tar.gz\n";
        assert_eq!(
            find_expected_sha256(sums, "desktest-v0.9.1-aarch64-apple-darwin.tar.gz"),
            Some("abc123".to_string())
        );
        assert_eq!(
            find_expected_sha256(sums, "desktest-v0.9.1-x86_64-unknown-linux-gnu.tar.gz"),
            Some("def456".to_string())
        );
        assert_eq!(find_expected_sha256(sums, "nonexistent.tar.gz"), None);
    }
}
