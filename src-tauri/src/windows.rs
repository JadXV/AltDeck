use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

use crate::accounts::{self, Account};
use crate::roblox::{self, RobloxError};

const LOGIN_LABEL: &str = "login";
const ROBLOX_URL: &str = "https://www.roblox.com";
const CHROME_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

const CLOSE_HOST: &str = "altdeck.invalid";

const LOGIN_SCRIPT: &str = r#"
(() => {
  const brand = () => {
    document.getElementById('header')?.remove();
    const btn = document.getElementById('login-button');
    if (btn && btn.textContent !== 'Add Account') btn.textContent = 'Add Account';
    const h = document.querySelector('.login-header');
    if (h && h.textContent !== 'AltDeck') h.textContent = 'AltDeck';
  };
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') location.href = 'https://altdeck.invalid/close';
  });
  document.addEventListener('DOMContentLoaded', () => {
    brand();
    new MutationObserver(brand).observe(document.body, { childList: true, subtree: true, characterData: true });
  });
})();
"#;

fn roblox_url() -> Url {
    Url::parse(ROBLOX_URL).expect("valid URL")
}

#[tauri::command]
pub async fn open_login_window(app: AppHandle) -> Result<Option<Value>, String> {
    if let Some(existing) = app.get_webview_window(LOGIN_LABEL) {
        let _ = existing.set_focus();
        return Ok(None);
    }

    let closer = app.clone();
    let mut builder = WebviewWindowBuilder::new(
        &app,
        LOGIN_LABEL,
        WebviewUrl::External(Url::parse(&format!("{ROBLOX_URL}/login")).expect("valid URL")),
    )
    .title("Add Account")
    .inner_size(800.0, 700.0)
    .incognito(true)
    .user_agent(CHROME_UA)
    .initialization_script(LOGIN_SCRIPT)
    .on_navigation(move |url| {
        if url.host_str() == Some(CLOSE_HOST) {
            if let Some(w) = closer.get_webview_window(LOGIN_LABEL) {
                let _ = w.close();
            }
            return false;
        }
        true
    });
    if let Some(main) = app.get_webview_window("main") {
        builder = builder.parent(&main).map_err(|e| e.to_string())?;
    }
    builder.build().map_err(|e| e.to_string())?;

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        let Some(window) = app.get_webview_window(LOGIN_LABEL) else {
            return Ok(None);
        };

        let Ok(cookies) = window.cookies() else {
            continue;
        };
        let Some(cookie) = cookies
            .iter()
            .find(|c| {
                c.name() == ".ROBLOSECURITY"
                    && !c.value().is_empty()
                    && c.domain().is_some_and(|d| d.trim_start_matches('.').ends_with("roblox.com"))
            })
            .map(|c| c.value().to_string())
        else {
            continue;
        };
        let _ = window.close();

        let user = match roblox::get_user_data(&cookie).await {
            Ok(user) => user,
            Err(err) => {
                let kind = if matches!(err, RobloxError::Forbidden) { "forbidden" } else { "unknown" };
                return Ok(Some(json!({ "error": { "type": kind, "message": err.message() } })));
            }
        };

        let (account, _) = accounts::update(|list| {
            match list.iter_mut().find(|a| a.user_id == user.id) {
                Some(existing) => {
                    existing.cookie = cookie;
                    existing.refresh(&user);
                    existing.clone()
                }
                None => {
                    let account = Account::new(cookie, user);
                    list.push(account.clone());
                    account
                }
            }
        })?;
        return serde_json::to_value(account).map(Some).map_err(|e| e.to_string());
    }
}

fn data_store_id(user_id: u64) -> [u8; 16] {
    let digest = Sha256::digest(format!("altdeck-website-{user_id}"));
    digest[..16].try_into().expect("digest is 32 bytes")
}

#[tauri::command]
pub async fn open_website(app: AppHandle, user_id: u64, path: Option<String>) -> Result<(), String> {
    let account = accounts::find(user_id)?;
    let label = format!("website-{user_id}");
    let target = match path.as_deref() {
        Some(p) if p.starts_with('/') => roblox_url().join(p).map_err(|e| e.to_string())?,
        _ => roblox_url(),
    };

    if let Some(existing) = app.get_webview_window(&label) {
        if path.is_some() {
            let _ = existing.navigate(target);
        }
        let _ = existing.unminimize();
        let _ = existing.set_focus();
        return Ok(());
    }

    let title = match account.name.as_deref() {
        Some(name) => format!("Roblox · @{name}"),
        None => "Roblox".to_string(),
    };
    let window = WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::External(Url::parse("about:blank").expect("valid URL")),
    )
    .title(title)
    .inner_size(1200.0, 800.0)
    .data_store_identifier(data_store_id(user_id))
    .build()
    .map_err(|e| e.to_string())?;

    window
        .set_cookie(
            Cookie::build((".ROBLOSECURITY", account.cookie))
                .domain(".roblox.com")
                .path("/")
                .secure(true)
                .http_only(true)
                .build(),
        )
        .map_err(|e| format!("Failed to set session cookie: {e}"))?;
    window.navigate(target).map_err(|e| e.to_string())
}
