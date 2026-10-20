use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub enum RobloxError {
    Forbidden,

    Status(u16),

    Network(String),
}

impl RobloxError {
    pub fn message(&self) -> String {
        match self {
            Self::Forbidden => "Account access forbidden (403)".into(),
            Self::Status(code) => format!("Request failed: {code}"),
            Self::Network(msg) => msg.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserData {
    pub id: u64,
    pub name: String,
    pub display_name: String,
    pub thumbnail: String,
}

pub(crate) fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build HTTP client")
    })
}

async fn get_json<T: DeserializeOwned>(req: reqwest::RequestBuilder) -> Result<T, RobloxError> {
    let res = req
        .send()
        .await
        .map_err(|e| RobloxError::Network(e.to_string()))?;
    match res.status() {
        StatusCode::OK => res
            .json()
            .await
            .map_err(|_| RobloxError::Network("Failed to parse response".into())),
        StatusCode::FORBIDDEN => Err(RobloxError::Forbidden),
        status => Err(RobloxError::Status(status.as_u16())),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    id: u64,
    name: String,
    display_name: String,
}

#[derive(Deserialize)]
struct ThumbnailResponse {
    data: Vec<Thumbnail>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Thumbnail {
    state: String,
    image_url: Option<String>,
}

async fn get_profile(cookie: &str) -> Result<Profile, RobloxError> {
    get_json(
        client()
            .get("https://users.roblox.com/v1/users/authenticated")
            .header("Cookie", format!(".ROBLOSECURITY={cookie}")),
    )
    .await
}

async fn get_thumbnail(user_id: u64) -> Result<String, RobloxError> {
    let url = format!(
        "https://thumbnails.roblox.com/v1/users/avatar-bust?userIds={user_id}&size=150x150&format=Png&isCircular=true"
    );
    let fallback = format!(
        "https://www.roblox.com/headshot-thumbnail/image?userId={user_id}&width=150&height=150&format=png"
    );

    for attempt in 0..=3 {
        let res: ThumbnailResponse = get_json(client().get(&url)).await?;
        let thumb = res
            .data
            .into_iter()
            .next()
            .ok_or_else(|| RobloxError::Network("No thumbnail data found".into()))?;
        match (thumb.state.as_str(), thumb.image_url) {
            ("Completed", Some(url)) => return Ok(url),
            ("Pending", _) if attempt < 3 => tokio::time::sleep(Duration::from_millis(1500)).await,
            _ => break,
        }
    }
    Ok(fallback)
}

pub async fn get_user_data(cookie: &str) -> Result<UserData, RobloxError> {
    let profile = get_profile(cookie).await?;
    let thumbnail = get_thumbnail(profile.id).await?;
    Ok(UserData {
        id: profile.id,
        name: profile.name,
        display_name: profile.display_name,
        thumbnail,
    })
}

pub(crate) async fn fetch_value(req: reqwest::RequestBuilder) -> Option<serde_json::Value> {
    let retry = req.try_clone();
    let mut res = req.send().await.ok()?;

    if res.status() == StatusCode::TOO_MANY_REQUESTS {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        res = retry?.send().await.ok()?;
    }
    if !res.status().is_success() {
        return None;
    }
    res.json().await.ok()
}

async fn post_as_user(url: &str, cookie: &str, body: serde_json::Value) -> Option<serde_json::Value> {
    let send = |token: Option<String>| {
        let mut req = client()
            .post(url)
            .header("Cookie", format!(".ROBLOSECURITY={cookie}"))
            .json(&body);
        if let Some(token) = token {
            req = req.header("x-csrf-token", token);
        }
        req.send()
    };
    let mut res = send(None).await.ok()?;
    if res.status() == StatusCode::FORBIDDEN {
        let token = res.headers().get("x-csrf-token")?.to_str().ok()?.to_string();
        res = send(Some(token)).await.ok()?;
    }
    if !res.status().is_success() {
        return None;
    }
    res.json().await.ok()
}

pub async fn get_details(cookie: &str, id: u64) -> serde_json::Map<String, serde_json::Value> {
    let public = |url: String| client().get(url);
    let auth = |url: String| client().get(url).header("Cookie", format!(".ROBLOSECURITY={cookie}"));

    let requests = vec![

        ("profile", public(format!("https://users.roblox.com/v1/users/{id}"))),
        ("usernameHistory", public(format!("https://users.roblox.com/v1/users/{id}/username-history?limit=50&sortOrder=Desc"))),
        ("headshot", public(format!("https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={id}&size=420x420&format=Png&isCircular=false"))),
        ("friends", public(format!("https://friends.roblox.com/v1/users/{id}/friends/count"))),
        ("followers", public(format!("https://friends.roblox.com/v1/users/{id}/followers/count"))),
        ("followings", public(format!("https://friends.roblox.com/v1/users/{id}/followings/count"))),
        ("groups", public(format!("https://groups.roblox.com/v2/users/{id}/groups/roles"))),
        ("primaryGroup", public(format!("https://groups.roblox.com/v1/users/{id}/groups/primary/role"))),
        ("games", public(format!("https://games.roblox.com/v2/users/{id}/games?limit=50&sortOrder=Desc"))),

        ("currency", auth(format!("https://economy.roblox.com/v1/users/{id}/currency"))),
        ("premium", auth(format!("https://premiumfeatures.roblox.com/v1/users/{id}/validate-membership"))),
        ("email", auth("https://accountsettings.roblox.com/v1/email".into())),
        ("phone", auth("https://accountinformation.roblox.com/v1/phone".into())),
        ("voice", auth("https://voice.roblox.com/v1/settings".into())),
    ];

    let fetches = futures::future::join_all(
        requests
            .into_iter()
            .map(|(key, req)| async move { (key, fetch_value(req).await) }),
    );
    let presence = post_as_user(
        "https://presence.roblox.com/v1/presence/users",
        cookie,
        serde_json::json!({ "userIds": [id] }),
    );
    let (results, presence) = futures::join!(fetches, presence);

    let mut out: serde_json::Map<String, serde_json::Value> = results
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.unwrap_or_default()))
        .collect();
    out.insert("presence".into(), presence.unwrap_or_default());
    out
}
