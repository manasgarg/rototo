use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};

use crate::error::{Result, RototoError};

const TOKEN_FORMAT: &str = "rototo-console-token-v1";
pub const KEY_ENV: &str = "ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY";
const TAG_LEN: usize = 16;

/// Encrypts stored GitHub tokens at rest with AES-256-GCM.
///
/// Hosted mode loads the key from `ROTOTO_CONSOLE_TOKEN_ENCRYPTION_KEY`; local
/// mode may generate and persist one under the console data directory. The
/// crypto handle lives inside `Store`, and ciphertext rows use
/// `rototo-console-token-v1.<nonce>.<tag>.<ciphertext>` with base64url parts.
#[derive(Clone)]
pub struct TokenCrypto {
    key: [u8; 32],
}

impl TokenCrypto {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub fn from_env_value(raw: &str) -> Result<Self> {
        let key = decode_key(raw.trim())?;
        Ok(Self::new(key))
    }

    pub fn generate() -> Result<Self> {
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| RototoError::new("failed to generate a token encryption key"))?;
        Ok(Self::new(key))
    }

    pub fn key_base64(&self) -> String {
        format!(
            "base64:{}",
            base64::engine::general_purpose::STANDARD.encode(self.key)
        )
    }

    pub fn encrypt(&self, token: &str) -> Result<String> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| RototoError::new("failed to generate an encryption nonce"))?;
        let key = LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, &self.key)
                .map_err(|_| RototoError::new("token encryption key is invalid"))?,
        );
        let mut data = token.as_bytes().to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(TOKEN_FORMAT.as_bytes()),
            &mut data,
        )
        .map_err(|_| RototoError::new("token encryption failed"))?;
        let tag_start = data.len() - TAG_LEN;
        Ok([
            TOKEN_FORMAT,
            &URL_SAFE_NO_PAD.encode(nonce_bytes),
            &URL_SAFE_NO_PAD.encode(&data[tag_start..]),
            &URL_SAFE_NO_PAD.encode(&data[..tag_start]),
        ]
        .join("."))
    }

    pub fn decrypt(&self, encrypted: &str) -> Result<String> {
        let mut parts = encrypted.split('.');
        let (format, nonce, tag, ciphertext) = match (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) {
            (Some(format), Some(nonce), Some(tag), Some(ciphertext), None) => {
                (format, nonce, tag, ciphertext)
            }
            _ => {
                return Err(RototoError::new(
                    "GitHub token is not stored in the supported encrypted format",
                ));
            }
        };
        if format != TOKEN_FORMAT || nonce.is_empty() || tag.is_empty() || ciphertext.is_empty() {
            return Err(RototoError::new(
                "GitHub token is not stored in the supported encrypted format",
            ));
        }
        let nonce = URL_SAFE_NO_PAD
            .decode(nonce)
            .map_err(|_| RototoError::new("stored token nonce is not valid base64url"))?;
        let nonce: [u8; NONCE_LEN] = nonce
            .try_into()
            .map_err(|_| RototoError::new("stored token nonce has the wrong length"))?;
        let mut data = URL_SAFE_NO_PAD
            .decode(ciphertext)
            .map_err(|_| RototoError::new("stored token ciphertext is not valid base64url"))?;
        data.extend(
            URL_SAFE_NO_PAD
                .decode(tag)
                .map_err(|_| RototoError::new("stored token tag is not valid base64url"))?,
        );
        let key = LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, &self.key)
                .map_err(|_| RototoError::new("token encryption key is invalid"))?,
        );
        let plain = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(TOKEN_FORMAT.as_bytes()),
                &mut data,
            )
            .map_err(|_| RototoError::new("stored token failed to decrypt"))?;
        String::from_utf8(plain.to_vec())
            .map_err(|_| RototoError::new("decrypted token is not valid UTF-8"))
    }
}

fn decode_key(raw: &str) -> Result<[u8; 32]> {
    if raw.is_empty() {
        return Err(RototoError::new(format!(
            "{KEY_ENV} is required before GitHub sign-in"
        )));
    }
    let bytes = if let Some(rest) = raw.strip_prefix("base64:") {
        base64::engine::general_purpose::STANDARD
            .decode(rest)
            .map_err(|_| RototoError::new(format!("{KEY_ENV} is not valid base64")))?
    } else if let Some(rest) = raw.strip_prefix("hex:") {
        decode_hex(rest)?
    } else if raw.len() == 64 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
        decode_hex(raw)?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|_| RototoError::new(format!("{KEY_ENV} is not valid base64")))?
    };
    bytes
        .try_into()
        .map_err(|_| RototoError::new(format!("{KEY_ENV} must decode to exactly 32 bytes")))
}

fn decode_hex(raw: &str) -> Result<Vec<u8>> {
    if !raw.len().is_multiple_of(2) || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(RototoError::new(format!("{KEY_ENV} is not valid hex")));
    }
    Ok((0..raw.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&raw[at..at + 2], 16).expect("validated hex digits"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_round_trips() {
        let crypto = TokenCrypto::generate().unwrap();
        let encrypted = crypto.encrypt("gho_example_token").unwrap();
        assert!(encrypted.starts_with("rototo-console-token-v1."));
        assert_eq!(crypto.decrypt(&encrypted).unwrap(), "gho_example_token");
    }

    #[test]
    fn decrypt_rejects_other_keys_and_garbage() {
        let crypto = TokenCrypto::generate().unwrap();
        let other = TokenCrypto::generate().unwrap();
        let encrypted = crypto.encrypt("gho_example_token").unwrap();
        assert!(other.decrypt(&encrypted).is_err());
        assert!(crypto.decrypt("not-an-encrypted-token").is_err());
    }

    #[test]
    fn key_forms_decode() {
        let crypto = TokenCrypto::generate().unwrap();
        let round = TokenCrypto::from_env_value(&crypto.key_base64()).unwrap();
        let encrypted = crypto.encrypt("tok").unwrap();
        assert_eq!(round.decrypt(&encrypted).unwrap(), "tok");
        assert!(TokenCrypto::from_env_value("hex:00ff").is_err());
        let hex64 = "a".repeat(64);
        assert!(TokenCrypto::from_env_value(&hex64).is_ok());
    }
}
