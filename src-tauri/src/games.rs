use std::collections::HashMap;

use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::OsRng;
use reqwest::Url;
use serde_json::{json, Value};

use crate::roblox::{client, fetch_value};

fn session_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

fn tile(g: &Value) -> Option<Value> {
    if g["isSponsored"].as_bool() == Some(true) {
        return None;
    }
    let up = g["totalUpVotes"].as_u64().unwrap_or(0);
    let down = g["totalDownVotes"].as_u64().unwrap_or(0);
    Some(json!({
        "universeId": g["universeId"].as_u64()?,
        "placeId": g["rootPlaceId"].as_u64()?,
        "name": g["name"],
        "playing": g["playerCount"],
        "rating": (up + down > 0).then(|| up * 100 / (up + down)),
        "creator": g.get("creatorName"),
        "icon": Value::Null,
    }))
}

async fn icon_map(ids: Vec<u64>) -> HashMap<u64, String> {
    let requests = ids.chunks(100).map(|chunk| {
        let list: Vec<String> = chunk.iter().map(u64::to_string).collect();
        fetch_value(client().get(format!(
            "https://thumbnails.roblox.com/v1/games/icons?universeIds={}&returnPolicy=PlaceHolder&size=256x256&format=Png&isCircular=false",
            list.join(",")
        )))
    });
    let mut icons = HashMap::new();
    for res in futures::future::join_all(requests).await.into_iter().flatten() {
        for d in res["data"].as_array().into_iter().flatten() {
            if let (Some(id), Some(url)) = (d["targetId"].as_u64(), d["imageUrl"].as_str()) {
                icons.insert(id, url.to_string());
            }
        }
    }
    icons
}

async fn attach_icons<'a>(tiles: impl Iterator<Item = &'a mut Value>) {
    let tiles: Vec<&mut Value> = tiles.collect();
    let ids = tiles.iter().filter_map(|t| t["universeId"].as_u64()).collect();
    let icons = icon_map(ids).await;
    for t in tiles {
        if let Some(url) = t["universeId"].as_u64().and_then(|id| icons.get(&id)) {
            t["icon"] = json!(url);
        }
    }
}

#[tauri::command]
pub async fn discover_games() -> Result<Value, String> {
    let url = format!(
        "https://apis.roblox.com/explore-api/v1/get-sorts?sessionId={}&device=computer&country=all",
        session_id()
    );
    let res = fetch_value(client().get(url))
        .await
        .ok_or("Couldn't load games from Roblox.")?;

    let mut shelves: Vec<Value> = res["sorts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["contentType"] == "Games")
        .map(|s| {
            let games: Vec<Value> = s["games"].as_array().into_iter().flatten().filter_map(tile).take(30).collect();
            json!({ "id": s["sortId"], "name": s["sortDisplayName"], "games": games })
        })
        .filter(|s| s["games"].as_array().is_some_and(|g| !g.is_empty()))
        .collect();

    attach_icons(
        shelves
            .iter_mut()
            .flat_map(|s| s["games"].as_array_mut().into_iter().flatten()),
    )
    .await;
    Ok(json!(shelves))
}

#[tauri::command]
pub async fn search_games(query: String) -> Result<Vec<Value>, String> {
    let url = Url::parse_with_params(
        "https://apis.roblox.com/search-api/omni-search",
        &[("searchQuery", query.as_str()), ("sessionId", &session_id()), ("pageType", "all")],
    )
    .map_err(|e| e.to_string())?;
    let res = fetch_value(client().get(url))
        .await
        .ok_or("Couldn't search Roblox.")?;

    let mut games: Vec<Value> = res["searchResults"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|group| group["contentGroupType"] == "Game")
        .flat_map(|group| group["contents"].as_array().into_iter().flatten())
        .filter_map(tile)
        .collect();
    attach_icons(games.iter_mut()).await;
    Ok(games)
}

#[tauri::command]
pub async fn game_details(universe_id: u64) -> Result<Value, String> {
    let info = fetch_value(client().get(format!("https://games.roblox.com/v1/games?universeIds={universe_id}")));
    let thumbs = fetch_value(client().get(format!(
        "https://thumbnails.roblox.com/v1/games/multiget/thumbnails?universeIds={universe_id}&countPerUniverse=6&defaults=true&size=768x432&format=Png&isCircular=false"
    )));
    let votes = fetch_value(client().get(format!("https://games.roblox.com/v1/games/votes?universeIds={universe_id}")));
    let (info, thumbs, votes, icons) = futures::join!(info, thumbs, votes, icon_map(vec![universe_id]));

    let game = info
        .and_then(|i| i["data"].get(0).cloned())
        .ok_or("Couldn't load this experience.")?;
    let screenshots: Vec<Value> = thumbs
        .map(|t| t["data"][0]["thumbnails"].clone())
        .and_then(|t| t.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t["state"] == "Completed")
        .map(|t| t["imageUrl"].clone())
        .collect();
    let (up, down) = votes
        .map(|v| (v["data"][0]["upVotes"].as_u64().unwrap_or(0), v["data"][0]["downVotes"].as_u64().unwrap_or(0)))
        .unwrap_or((0, 0));

    Ok(json!({
        "game": game,
        "screenshots": screenshots,
        "icon": icons.get(&universe_id),
        "rating": (up + down > 0).then(|| up * 100 / (up + down)),
    }))
}
