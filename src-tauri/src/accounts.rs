use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

use crate::crypto;
use crate::roblox::UserData;

static ACCOUNTS_FILE: OnceLock<PathBuf> = OnceLock::new();

static FILE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub cookie: String,
    pub user_id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub added_at: Option<String>,

    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Account {
    pub fn new(cookie: String, user: UserData) -> Self {
        Self {
            cookie,
            user_id: user.id,
            name: Some(user.name),
            display_name: Some(user.display_name),
            thumbnail: Some(user.thumbnail),
            added_at: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            extra: Map::new(),
        }
    }

    pub fn refresh(&mut self, user: &UserData) -> bool {
        let fresh = (
            Some(user.name.clone()),
            Some(user.display_name.clone()),
            Some(user.thumbnail.clone()),
        );
        if (self.name.clone(), self.display_name.clone(), self.thumbnail.clone()) == fresh {
            return false;
        }
        (self.name, self.display_name, self.thumbnail) = fresh;
        true
    }

    pub fn from_json(mut value: Value) -> Option<Self> {
        let obj = value.as_object_mut()?;
        obj.remove("expired");
        if let Some(Value::String(id)) = obj.get("userId") {
            let id: u64 = id.parse().ok()?;
            obj.insert("userId".into(), id.into());
        }
        let account: Self = serde_json::from_value(value).ok()?;
        (!account.cookie.is_empty() && account.user_id != 0).then_some(account)
    }
}

pub fn init(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app.path().document_dir()?.join("AltDeck");
    fs::create_dir_all(&dir)?;
    let _ = ACCOUNTS_FILE.set(dir.join("accounts"));
    Ok(())
}

fn path() -> &'static PathBuf {
    ACCOUNTS_FILE.get().expect("accounts::init not called")
}

fn read_file() -> Result<Vec<Account>, String> {
    let content = match fs::read_to_string(path()) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read accounts: {e}")),
    };
    let content = content.trim();
    if content.is_empty() {
        return Ok(Vec::new());
    }

    if crypto::is_encrypted(content) {
        let json = crypto::decrypt(content)?;
        return parse(&json);
    }

    let accounts = parse(content)?;
    write_file(&accounts)?;
    Ok(accounts)
}

fn parse(json: &str) -> Result<Vec<Account>, String> {
    let values: Vec<Value> =
        serde_json::from_str(json).map_err(|e| format!("Accounts file is corrupted: {e}"))?;
    Ok(values.into_iter().filter_map(Account::from_json).collect())
}

fn write_file(accounts: &[Account]) -> Result<(), String> {
    let json = serde_json::to_string(accounts).map_err(|e| e.to_string())?;
    let sealed = crypto::encrypt(&json)?;
    let tmp = path().with_extension("tmp");
    fs::write(&tmp, sealed).map_err(|e| format!("Failed to save accounts: {e}"))?;
    fs::rename(&tmp, path()).map_err(|e| format!("Failed to save accounts: {e}"))
}

pub fn load() -> Result<Vec<Account>, String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_file()
}

pub fn update<T>(f: impl FnOnce(&mut Vec<Account>) -> T) -> Result<(T, Vec<Account>), String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut accounts = read_file()?;
    let out = f(&mut accounts);
    write_file(&accounts)?;
    Ok((out, accounts))
}

pub fn find(user_id: u64) -> Result<Account, String> {
    load()?
        .into_iter()
        .find(|a| a.user_id == user_id)
        .ok_or_else(|| "Account not found".to_string())
}
