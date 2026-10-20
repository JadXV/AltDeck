use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use tokio::sync::Mutex;

use crate::binary_cookies;

const DEFAULT_BUNDLE_ID: &str = "com.roblox.RobloxPlayer";

static LAUNCH_LOCK: Mutex<()> = Mutex::const_new(());

fn bundle_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"<key>CFBundleIdentifier</key>\s*<string>(com\.roblox\.RobloxPlayer(?:\.\w+)?)</string>")
            .unwrap()
    })
}

fn plist_path(roblox: &Path) -> PathBuf {
    roblox.join("Contents/Info.plist")
}

fn current_bundle_id(roblox: &Path) -> Option<String> {
    let content = fs::read_to_string(plist_path(roblox)).ok()?;
    let caps = bundle_id_regex().captures(&content)?;
    Some(caps[1].to_string())
}

fn set_bundle_id(roblox: &Path, bundle_id: &str) -> Result<(), String> {
    let plist = plist_path(roblox);
    let content = fs::read_to_string(&plist)
        .map_err(|_| format!("Info.plist not found at: {}", plist.display()))?;
    if !bundle_id_regex().is_match(&content) {
        return Err("Could not find CFBundleIdentifier in Info.plist".into());
    }
    let replacement = format!("<key>CFBundleIdentifier</key>\n\t<string>{bundle_id}</string>");
    let updated = bundle_id_regex().replace(&content, regex::NoExpand(&replacement));
    fs::write(&plist, updated.as_bytes()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            open_app_management_settings();
            APP_MANAGEMENT_HINT.to_string()
        } else {
            e.to_string()
        }
    })
}

const APP_MANAGEMENT_HINT: &str = "macOS blocked AltDeck from modifying Roblox. \
Turn on AltDeck (or the app you launched it from, e.g. your terminal or VS Code) under \
System Settings > Privacy & Security > App Management, then restart it and try again.";

fn open_app_management_settings() {
    let _ = quiet(Command::new("open").arg(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_AppBundles",
    ))
    .spawn();
}

fn find_roblox() -> Option<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    [
        PathBuf::from("/Applications/Roblox.app"),
        PathBuf::from(home).join("Applications/Roblox.app"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

fn quiet(cmd: &mut Command) -> &mut Command {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
}

pub async fn launch(cookie: &str, user_id: u64, place_id: Option<u64>) -> Result<u32, String> {
    let roblox = find_roblox().ok_or("Roblox not found. Please install Roblox first.")?;
    binary_cookies::write_roblox_cookie(cookie, &user_id.to_string())?;

    let bundle_id = format!("{DEFAULT_BUNDLE_ID}.{user_id}");
    let _guard = LAUNCH_LOCK.lock().await;

    set_bundle_id(&roblox, &bundle_id).map_err(|e| match e.as_str() {
        APP_MANAGEMENT_HINT => e,
        _ => format!("Failed to modify bundle identifier: {e}"),
    })?;

    let app = roblox.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        let cleared = quiet(Command::new("xattr").arg("-cr").arg(&app)).status();
        if cleared.is_ok_and(|s| s.success()) {
            let _ = quiet(Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&app))
                .status();
        }
    })
    .await;

    let exe = roblox.join("Contents/MacOS/RobloxPlayer");
    if !exe.exists() {
        return Err("RobloxPlayer executable not found".into());
    }
    let mut cmd = Command::new(&exe);
    if let Some(place_id) = place_id {
        cmd.arg("-protocolString")
            .arg(format!("roblox://experiences/start?placeId={place_id}"));
    }
    let mut child = quiet(&mut cmd)
        .process_group(0)
        .spawn()
        .map_err(|e| format!("Failed to launch Roblox: {e}"))?;
    let pid = child.id();

    std::thread::spawn(move || {
        let _ = child.wait();
    });

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let _guard = LAUNCH_LOCK.lock().await;
        if current_bundle_id(&roblox).as_deref() == Some(bundle_id.as_str()) {
            let _ = set_bundle_id(&roblox, DEFAULT_BUNDLE_ID);
        }
    });

    Ok(pid)
}

pub fn kill_all() -> usize {
    let count = Command::new("pgrep")
        .args(["-x", "RobloxPlayer"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    if count > 0 {
        for name in ["RobloxPlayer", "Roblox"] {
            let _ = quiet(Command::new("killall").args(["-9", name])).status();
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_bundle_id() {
        let plist = "<dict>\n\t<key>CFBundleIdentifier</key>\n\t<string>com.roblox.RobloxPlayer.123</string>\n</dict>";
        let caps = bundle_id_regex().captures(plist).unwrap();
        assert_eq!(&caps[1], "com.roblox.RobloxPlayer.123");
        let out = bundle_id_regex().replace(plist, "<key>CFBundleIdentifier</key>\n\t<string>com.roblox.RobloxPlayer</string>");
        assert!(out.contains("<string>com.roblox.RobloxPlayer</string>"));
    }
}
