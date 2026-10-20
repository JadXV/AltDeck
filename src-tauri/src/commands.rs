use std::collections::HashSet;
use std::fs;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use crate::accounts::{self, Account};
use crate::launcher;
use crate::roblox::{self, RobloxError};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    #[serde(flatten)]
    account: Account,
    expired: bool,
}

fn message_dialog(app: &AppHandle, kind: MessageDialogKind, title: &str, message: &str) -> tauri_plugin_dialog::MessageDialogBuilder<tauri::Wry> {
    let builder = app.dialog().message(message).title(title).kind(kind);
    match app.get_webview_window("main") {
        Some(main) => builder.parent(&main),
        None => builder,
    }
}

#[tauri::command]
pub fn list_accounts() -> Result<Vec<Account>, String> {
    accounts::load()
}

#[tauri::command]
pub async fn get_accounts() -> Result<Vec<AccountView>, String> {
    let accounts = accounts::load()?;
    let results =
        futures::future::join_all(accounts.iter().map(|a| roblox::get_user_data(&a.cookie))).await;

    let mut views = Vec::with_capacity(accounts.len());
    let mut refreshed = Vec::new();
    for (mut account, result) in accounts.into_iter().zip(results) {
        let expired = match result {
            Ok(user) => {
                if account.refresh(&user) {
                    refreshed.push(user);
                }
                false
            }

            Err(RobloxError::Network(_)) => false,
            Err(_) => true,
        };
        views.push(AccountView { account, expired });
    }

    if !refreshed.is_empty() {
        accounts::update(|list| {
            for user in &refreshed {
                if let Some(acc) = list.iter_mut().find(|a| a.user_id == user.id) {
                    acc.refresh(user);
                }
            }
        })?;
    }
    Ok(views)
}

#[tauri::command]
pub async fn get_account_details(user_id: u64) -> Result<Value, String> {
    let account = accounts::find(user_id)?;
    Ok(Value::Object(roblox::get_details(&account.cookie, user_id).await))
}

#[tauri::command]
pub fn delete_accounts(user_ids: Vec<u64>) -> Result<Vec<Account>, String> {
    let ids: HashSet<u64> = user_ids.into_iter().collect();
    let (_, remaining) = accounts::update(|list| list.retain(|a| !ids.contains(&a.user_id)))?;
    Ok(remaining)
}

#[tauri::command]
pub async fn export_accounts(app: AppHandle) -> Result<Value, String> {
    let list = accounts::load()?;
    if list.is_empty() {
        return Err("No accounts to export".into());
    }

    let proceed = message_dialog(
        &app,
        MessageDialogKind::Warning,
        "Exported accounts are NOT encrypted!",
        "The exported file will contain your account cookies in plain text. Anyone with this file can add and use ALL your accounts.\n\nOnly share this file with people you absolutely trust.",
    )
    .buttons(MessageDialogButtons::OkCancelCustom(
        "Export Anyway".into(),
        "Cancel".into(),
    ))
    .blocking_show();
    if !proceed {
        return Ok(json!({ "cancelled": true }));
    }

    let Some(target) = app
        .dialog()
        .file()
        .set_title("Export Accounts")
        .set_file_name("altdeck-accounts.json")
        .add_filter("JSON Files", &["json"])
        .blocking_save_file()
    else {
        return Ok(json!({ "cancelled": true }));
    };
    let target = target.into_path().map_err(|e| e.to_string())?;

    let json = serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?;
    fs::write(&target, json).map_err(|e| format!("Failed to write export: {e}"))?;
    Ok(json!({ "success": true, "count": list.len() }))
}

#[tauri::command]
pub async fn import_accounts(app: AppHandle) -> Result<Value, String> {
    let Some(source) = app
        .dialog()
        .file()
        .set_title("Import Accounts")
        .add_filter("JSON Files", &["json"])
        .blocking_pick_file()
    else {
        return Ok(json!({ "cancelled": true, "imported": 0 }));
    };
    let source = source.into_path().map_err(|e| e.to_string())?;

    let content = fs::read_to_string(&source).map_err(|e| format!("Failed to read file: {e}"))?;
    let values: Vec<Value> = serde_json::from_str(&content)
        .map_err(|_| "Invalid file format: expected an array of accounts".to_string())?;
    let incoming: Vec<Account> = values.into_iter().filter_map(Account::from_json).collect();

    let (imported, _) = accounts::update(|list| {
        let mut known: HashSet<u64> = list.iter().map(|a| a.user_id).collect();
        let before = list.len();
        list.extend(incoming.into_iter().filter(|a| known.insert(a.user_id)));
        list.len() - before
    })?;
    Ok(json!({ "imported": imported }))
}

#[tauri::command]
pub async fn launch_account(user_id: u64, place_id: Option<u64>) -> Result<Value, String> {
    let account = accounts::find(user_id)?;
    let pid = launcher::launch(&account.cookie, user_id, place_id).await?;
    Ok(json!({ "success": true, "pid": pid }))
}

#[tauri::command]
pub async fn kill_all_roblox() -> Value {
    let count = tauri::async_runtime::spawn_blocking(launcher::kill_all)
        .await
        .unwrap_or(0);
    json!({ "count": count })
}

#[tauri::command]
pub async fn show_alert(app: AppHandle, kind: Option<String>, title: Option<String>, message: String) {
    let kind = match kind.as_deref() {
        Some("error") => MessageDialogKind::Error,
        Some("warning") => MessageDialogKind::Warning,
        _ => MessageDialogKind::Info,
    };
    message_dialog(&app, kind, title.as_deref().unwrap_or("AltDeck"), &message).blocking_show();
}

#[tauri::command]
pub async fn confirm(app: AppHandle, title: String, message: String, ok_label: Option<String>) -> bool {
    message_dialog(&app, MessageDialogKind::Warning, &title, &message)
        .buttons(MessageDialogButtons::OkCancelCustom(
            ok_label.unwrap_or_else(|| "OK".into()),
            "Cancel".into(),
        ))
        .blocking_show()
}
