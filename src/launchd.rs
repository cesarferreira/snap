//! Installs/removes the launchd user agent that runs `snap daemon run` in
//! the background, so `snap last` has continuous focus history to read
//! (design doc docs/superpowers/specs/2026-09-02-snap-last-design.md).
//! Opt-in only — every other snap command remains daemon-free.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Result, anyhow};

pub const LABEL: &str = "com.cesarferreira.snap.focuswatch";

pub fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LABEL}.plist")),
    )
}

fn log_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("snap")
            .join("focuswatch.log"),
    )
}

fn plist_contents(exe: &std::path::Path, log: &std::path::Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>daemon</string>
        <string>run</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        label = LABEL,
        exe = exe.display(),
        log = log.display(),
    )
}

fn current_uid() -> Result<String> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| anyhow!("failed to run 'id -u': {e}"))?;
    if !output.status.success() {
        return Err(anyhow!("'id -u' exited with an error"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Writes the launch agent plist and (re)loads it. Idempotent: safe to run
/// again if already installed.
pub fn install() -> Result<()> {
    let plist_path = plist_path().ok_or_else(|| anyhow!("$HOME not set"))?;
    let log_path = log_path().ok_or_else(|| anyhow!("$HOME not set"))?;
    let exe = std::env::current_exe().map_err(|e| anyhow!("failed to locate snap binary: {e}"))?;

    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&plist_path, plist_contents(&exe, &log_path))?;

    let uid = current_uid()?;
    // Ignore failure: bootout errors if nothing is loaded yet, which is the
    // common case on first install.
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/{LABEL}")])
        .output();
    let status = Command::new("launchctl")
        .args([
            "bootstrap",
            &format!("gui/{uid}"),
            &plist_path.to_string_lossy(),
        ])
        .status()
        .map_err(|e| anyhow!("failed to run launchctl: {e}"))?;
    if !status.success() {
        return Err(anyhow!("launchctl bootstrap failed"));
    }
    Ok(())
}

/// Stops and removes the launch agent.
pub fn uninstall() -> Result<()> {
    let plist_path = plist_path().ok_or_else(|| anyhow!("$HOME not set"))?;
    let uid = current_uid()?;
    let _ = Command::new("launchctl")
        .args(["bootout", &format!("gui/{uid}/{LABEL}")])
        .output();
    if plist_path.exists() {
        std::fs::remove_file(&plist_path)?;
    }
    Ok(())
}

/// `(installed, loaded)` — installed means the plist file exists; loaded
/// means launchd currently has the agent running. Used by `snap doctor`.
pub fn status() -> (bool, bool) {
    let installed = plist_path().is_some_and(|p| p.exists());
    let loaded = current_uid()
        .ok()
        .map(|uid| {
            Command::new("launchctl")
                .args(["print", &format!("gui/{uid}/{LABEL}")])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
        .unwrap_or(false);
    (installed, loaded)
}
