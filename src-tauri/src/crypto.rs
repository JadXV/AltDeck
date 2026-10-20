use aes_gcm::aead::consts::U16;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::aes::Aes256;
use aes_gcm::AesGcm;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use sha2::{Digest, Sha256};

type Cipher = AesGcm<Aes256, U16>;

const TAG_LEN: usize = 16;

fn key() -> [u8; 32] {
    let user = std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "user".into());
    Sha256::digest(format!("{user}-darwin-altdeck-v1")).into()
}

pub fn encrypt(plain: &str) -> Result<String, String> {
    let cipher = Cipher::new(&key().into());
    let iv = Cipher::generate_nonce(&mut OsRng);
    let mut sealed = cipher
        .encrypt(&iv, plain.as_bytes())
        .map_err(|_| "Failed to encrypt data".to_string())?;
    let tag = sealed.split_off(sealed.len() - TAG_LEN);
    Ok(format!(
        "{}:{}:{}",
        B64.encode(iv),
        B64.encode(tag),
        B64.encode(sealed)
    ))
}

pub fn decrypt(encoded: &str) -> Result<String, String> {
    let fail = |msg: &str| format!("Failed to decrypt data: {msg}");
    let parts: Vec<&str> = encoded.split(':').collect();
    let [iv, tag, data] = parts.as_slice() else {
        return Err(fail("Invalid encrypted data format"));
    };

    let iv = B64.decode(iv).map_err(|_| fail("bad IV"))?;
    let tag = B64.decode(tag).map_err(|_| fail("bad auth tag"))?;
    let mut sealed = B64.decode(data).map_err(|_| fail("bad ciphertext"))?;
    if iv.len() != 16 || tag.len() != TAG_LEN {
        return Err(fail("Invalid encrypted data format"));
    }
    sealed.extend_from_slice(&tag);

    let cipher = Cipher::new(&key().into());
    let plain = cipher
        .decrypt(GenericArray::from_slice(&iv), sealed.as_slice())
        .map_err(|_| fail("authentication failed"))?;
    String::from_utf8(plain).map_err(|_| fail("invalid UTF-8"))
}

pub fn is_encrypted(content: &str) -> bool {
    let parts: Vec<&str> = content.split(':').collect();
    parts.len() == 3 && parts.iter().all(|p| is_base64(p))
}

fn is_base64(s: &str) -> bool {
    let body = s.trim_end_matches('=');
    !body.is_empty()
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let sealed = encrypt(r#"[{"userId":1}]"#).unwrap();
        assert!(is_encrypted(&sealed));
        assert_eq!(decrypt(&sealed).unwrap(), r#"[{"userId":1}]"#);
    }

    #[test]
    fn plaintext_json_is_not_encrypted() {
        assert!(!is_encrypted(r#"[{"userId":1,"cookie":"a:b"}]"#));
    }
}
