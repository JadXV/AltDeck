use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

const RELEASES_LATEST: &str = "https://github.com/JadXV/AltDeck/releases/latest";
const INSTALL_COMMAND: &str =
    "curl -fsSL https://raw.githubusercontent.com/JadXV/AltDeck/refs/heads/main/install.sh | bash";

async fn latest_version() -> Option<String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("AltDeck-Updater")
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let res = client.head(RELEASES_LATEST).send().await.ok()?;
    let location = res.headers().get(reqwest::header::LOCATION)?.to_str().ok()?;
    let (_, tag) = location.split_once("/tag/")?;
    Some(tag.to_string())
}

fn parse_version(v: &str) -> Vec<u64> {
    v.trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .map_while(|part| part.parse().ok())
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(latest), parse_version(current));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

pub async fn check_for_updates(app: &AppHandle) {
    let Some(latest) = latest_version().await else {
        return;
    };
    let current = app.package_info().version.to_string();
    if !is_newer(&latest, &current) {
        return;
    }

    let handle = app.clone();
    app.dialog()
        .message(format!(
            "AltDeck {latest} is available (you have v{current}).\n\nUpdating will reinstall AltDeck automatically using the install script."
        ))
        .title("Update Available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom("Update Now".into(), "Skip".into()))
        .show(move |update| {
            if !update {
                return;
            }
            let spawned = Command::new("bash")
                .args(["-c", INSTALL_COMMAND])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn();
            if spawned.is_ok() {
                handle.exit(0);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v2.10.0", "2.9.0"));
        assert!(is_newer("v2.0.1", "2.0.0"));
        assert!(!is_newer("v2.0", "2.0.0"));
        assert!(!is_newer("v1.9.9", "2.0.0"));
    }
}
