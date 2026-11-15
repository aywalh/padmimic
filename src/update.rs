//! Updates from GitHub releases: read the latest release, download its padmimic.exe, check it
//! against the release's SHA-256 file and swap it in. Windows lets a running exe be renamed, so the
//! old one moves aside and the new one starts in its place. No extra dependencies: PowerShell does
//! the HTTPS part, like the ViGEmBus installer in winio.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;

pub const REPO: &str = "aywalh/padmimic";
const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String, // without the leading "v"
    pub exe_url: String,
    pub sha_url: String,
    pub page: String,
}

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    Latest,
    Available(Release),
    Installing,
    Restarting, // new version started: close this one
    Failed(String),
}

/// "0.10.2" > "0.9.9": compare the dotted numbers, missing parts count as 0.
pub fn newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| v.trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (parse(latest), parse(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y { return x > y }
    }
    false
}

fn powershell(script: &str) -> Result<(i32, String), String> {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(NO_WINDOW)
        .output()
        .map_err(|e| format!("PowerShell could not start: {e}"))?;
    Ok((out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// Latest release if it is newer than this build.
pub fn check() -> Result<Option<Release>, String> {
    let script = format!(
        "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol='Tls12'
try {{ $r = Invoke-RestMethod -UseBasicParsing -Headers @{{'User-Agent'='padmimic-updater'}} 'https://api.github.com/repos/{REPO}/releases/latest' }} catch {{ exit 10 }}
$r | ConvertTo-Json -Depth 4 -Compress"
    );
    let (code, out) = powershell(&script)?;
    if code != 0 { return Err("Update check failed – check the internet connection".into()) }
    let v: serde_json::Value = serde_json::from_str(out.trim()).map_err(|_| "Update check: unexpected answer from GitHub".to_string())?;
    let version = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    let asset = |name: &str| v["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"] == name)).and_then(|x| x["browser_download_url"].as_str()).map(String::from);
    let (Some(exe_url), Some(sha_url)) = (asset("padmimic.exe"), asset("padmimic.exe.sha256")) else {
        return Ok(None); // a release without the files can't be installed
    };
    let r = Release { version, exe_url, sha_url, page: v["html_url"].as_str().unwrap_or("").into() };
    Ok(newer(&r.version, env!("CARGO_PKG_VERSION")).then_some(r))
}

fn exe_paths() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok((exe.with_file_name("padmimic.new.exe"), exe.with_file_name("padmimic.old.exe"), exe))
}

/// Download, verify, swap and start the new version. On Ok the caller closes this instance.
pub fn install(r: &Release) -> Result<(), String> {
    let (new, old, exe) = exe_paths()?;
    let script = format!(
        "$ProgressPreference='SilentlyContinue'; [Net.ServicePointManager]::SecurityProtocol='Tls12'
$f = '{new}'
try {{ Invoke-WebRequest -UseBasicParsing -Headers @{{'User-Agent'='padmimic-updater'}} '{exe_url}' -OutFile $f
       $want = ((Invoke-WebRequest -UseBasicParsing -Headers @{{'User-Agent'='padmimic-updater'}} '{sha_url}').Content -split '\\s+')[0] }} catch {{ exit 10 }}
if ((Get-FileHash $f -Algorithm SHA256).Hash -ne $want.ToUpper()) {{ Remove-Item $f; exit 11 }}",
        new = new.display().to_string().replace('\'', "''"),
        exe_url = r.exe_url,
        sha_url = r.sha_url,
    );
    match powershell(&script)?.0 {
        0 => {}
        11 => return Err("Update: wrong checksum – nothing was changed".into()),
        _ => return Err("Update: download failed – check the internet connection".into()),
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("Update: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe); // put the working version back
        return Err(format!("Update: {e}"));
    }
    std::process::Command::new(&exe).spawn().map_err(|e| format!("Update: {e}"))?;
    Ok(())
}

/// Remove what an earlier update left behind (the old exe can't be deleted while it runs).
pub fn cleanup() {
    if let Ok((new, old, _)) = exe_paths() {
        let _ = std::fs::remove_file(old);
        let _ = std::fs::remove_file(new);
    }
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn version_order() {
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(!newer("0.1", "0.1.0"));
    }
}
