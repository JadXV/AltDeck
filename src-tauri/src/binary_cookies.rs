use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const COCOA_EPOCH_OFFSET: f64 = 978_307_200.0;
const THIRTY_DAYS: f64 = 30.0 * 24.0 * 60.0 * 60.0;

pub fn write_roblox_cookie(cookie: &str, profile_id: &str) -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let dir = PathBuf::from(home).join("Library/HTTPStorages");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create cookie directory: {e}"))?;

    let file = dir.join(format!("com.roblox.RobloxPlayer.{profile_id}.binarycookies"));
    fs::write(&file, build(cookie)).map_err(|e| format!("Failed to write cookie: {e}"))?;
    Ok(file)
}

fn build(value: &str) -> Vec<u8> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default();
    let created = now - COCOA_EPOCH_OFFSET;
    let expires = created + THIRTY_DAYS;

    let strings: [&str; 4] = [".roblox.com", ".ROBLOSECURITY", "/", value];
    let header_len = 56u32;
    let mut offsets = [0u32; 4];
    let mut next = header_len;
    for (i, s) in strings.iter().enumerate() {
        offsets[i] = next;
        next += s.len() as u32 + 1;
    }
    let cookie_size = next;
    let flags = 0x5u32;

    let mut cookie = Vec::with_capacity(cookie_size as usize);
    for word in [cookie_size, 1, flags, 0] {
        cookie.extend_from_slice(&word.to_le_bytes());
    }
    for off in offsets {
        cookie.extend_from_slice(&off.to_le_bytes());
    }
    cookie.extend_from_slice(&[0u8; 8]);
    cookie.extend_from_slice(&expires.to_le_bytes());
    cookie.extend_from_slice(&created.to_le_bytes());
    for s in strings {
        cookie.extend_from_slice(s.as_bytes());
        cookie.push(0);
    }

    let mut page = vec![0x00, 0x00, 0x01, 0x00];
    page.extend_from_slice(&1u32.to_le_bytes());
    page.extend_from_slice(&12u32.to_le_bytes());
    page.extend_from_slice(&cookie);

    let checksum: u32 = page.iter().step_by(4).map(|&b| b as u32).sum();

    let mut out = b"cook".to_vec();
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&(page.len() as u32).to_be_bytes());
    out.extend_from_slice(&page);
    out.extend_from_slice(&checksum.to_be_bytes());
    out.extend_from_slice(&[0x07, 0x17, 0x20, 0x05, 0x00, 0x00, 0x00, 0x4B]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let data = build("abc");
        assert_eq!(&data[..4], b"cook");
        let page_len = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;

        assert_eq!(data.len(), 12 + page_len + 4 + 8);
        let cookie = &data[12 + 12..12 + page_len];
        let size = u32::from_le_bytes(cookie[..4].try_into().unwrap()) as usize;
        assert_eq!(size, cookie.len());
        assert!(cookie.ends_with(b"abc\0"));
    }
}
