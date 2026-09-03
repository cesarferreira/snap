use std::path::Path;
use std::process::Command;

const LEGACY_LABEL: &str = "com.cesarferreira.snap.focuswatch";

/// Stops and removes the launch agent installed by snap versions that
/// supported `snap last`. Cleanup is best-effort so an old, unreadable plist
/// never prevents a current one-shot command from running.
pub fn remove_legacy_daemon() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let plist = Path::new(&home)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LEGACY_LABEL}.plist"));
    if !plist.exists() {
        return;
    }

    if let Ok(output) = Command::new("id").arg("-u").output()
        && output.status.success()
    {
        let uid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{LEGACY_LABEL}")])
            .output();
    }

    let _ = remove_legacy_plist(&plist);
}

fn remove_legacy_plist(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_an_existing_legacy_launch_agent_plist() {
        let path =
            std::env::temp_dir().join(format!("snap-legacy-cleanup-test-{}", std::process::id()));
        std::fs::write(&path, "legacy plist").unwrap();

        remove_legacy_plist(&path).unwrap();

        assert!(!path.exists());
    }
}
