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

pub async fn run_update(force: bool) -> Result<(), AppError> {
    let current = current_version();
    println!("Current version: v{current}");

    let client = reqwest::Client::builder()
        .user_agent("desktest-updater")
        .build()?;

    println!("Checking for latest release...");
    let release: Release = client
        .get(format!(
            "https://api.github.com/repos/{GITHUB_REPO}/releases/latest"
        ))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AppError::Infra(format!("failed to fetch latest release: {e}")))?
        .json()
        .await?;

    let latest = &release.tag_name;
    println!("Latest release:  {latest}");

    if !force && !is_newer(latest, current) {
        println!("Already up to date.");
        return Ok(());
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

    println!("Downloading {}...", asset.name);
    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AppError::Infra(format!("failed to download asset: {e}")))?
        .bytes()
        .await?;

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

    let tmp_path = current_exe.with_extension("update-tmp");

    // Write new binary to temp file
    std::fs::write(&tmp_path, &binary_data)?;

    // Set executable permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
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
}
