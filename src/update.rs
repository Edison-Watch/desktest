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

fn is_newer(latest_tag: &str, current: &str) -> bool {
    match (parse_version(latest_tag), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
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
    let mut child = std::process::Command::new("shasum")
        .args(["-a", "256"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .or_else(|_| {
            std::process::Command::new("sha256sum")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
        })
        .expect("neither shasum nor sha256sum found");

    {
        use std::io::Write as IoWrite;
        let stdin = child.stdin.as_mut().expect("open stdin");
        stdin.write_all(data).expect("write to shasum stdin");
    }

    let output = child.wait_with_output().expect("wait for shasum");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase()
}

pub async fn run_update(force: bool) -> Result<(), AppError> {
    let current = current_version();
    println!("Current version: v{current}");

    let client = reqwest::Client::builder()
        .user_agent("desktest-updater")
        .build()
        .map_err(|e| AppError::Infra(format!("failed to build HTTP client: {e}")))?;

    println!("Checking for latest release...");
    let release: Release = client
        .get(format!(
            "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
        ))
        .send()
        .await
        .map_err(|e| AppError::Infra(format!("failed to fetch latest release: {e}")))?
        .error_for_status()
        .map_err(|e| AppError::Infra(format!("failed to fetch latest release: {e}")))?
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
        let sums_text = client
            .get(&sums_asset.browser_download_url)
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
    let bytes = client
        .get(&asset.browser_download_url)
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
        let mut name = current_exe.file_name().unwrap_or_default().to_os_string();
        name.push(".update-tmp");
        current_exe.with_file_name(name)
    };

    // Write new binary to temp file
    std::fs::write(&tmp_path, &binary_data)?;

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
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("v1.0.0", "0.9.1"));
        assert!(is_newer("v0.10.0", "0.9.1"));
        assert!(is_newer("v0.9.2", "0.9.1"));
        assert!(!is_newer("v0.9.1", "0.9.1"));
        assert!(!is_newer("v0.9.0", "0.9.1"));
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
