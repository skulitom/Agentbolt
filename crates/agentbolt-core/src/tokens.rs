//! Per-button toast tokens (PLAN.md, "Toasts (v1)").
//!
//! A token is `<toast_id>.<option_index>.<hex hmac-sha256(secret, "ask_id|option_index|toast_id")>`. The secret
//! is 32 random bytes made at the database's first open and kept in it. The MAC binds the ask, the option and
//! the toast, so a token can't be moved to another option or ask. Single use, expiry and "answered another way"
//! are enforced by the store (`toasts` table) when the token is presented.

use hmac::{Hmac, KeyInit, Mac};
use rand::RngCore;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub const SECRET_LEN: usize = 32;
/// Tokens expire 7 days after minting (or as soon as the ask is answered any other way).
pub const TOKEN_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const TOAST_ID_HEX: usize = 32;
const MAC_HEX: usize = 64;

pub fn new_secret() -> [u8; SECRET_LEN] {
    let mut s = [0u8; SECRET_LEN];
    rand::rng().fill_bytes(&mut s);
    s
}

/// `bytes` random bytes as lowercase hex.
pub fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}

pub fn new_toast_id() -> String {
    random_hex(TOAST_ID_HEX / 2)
}

fn mac(secret: &[u8], ask_id: &str, option_index: usize, toast_id: &str) -> HmacSha256 {
    let mut m = HmacSha256::new_from_slice(secret).expect("HMAC takes a key of any length");
    m.update(format!("{ask_id}|{option_index}|{toast_id}").as_bytes());
    m
}

/// Mint the token for one button.
pub fn mint(secret: &[u8], ask_id: &str, option_index: usize, toast_id: &str) -> String {
    let tag = mac(secret, ask_id, option_index, toast_id)
        .finalize()
        .into_bytes();
    format!("{toast_id}.{option_index}.{}", hex::encode(tag))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedToken {
    pub toast_id: String,
    pub option_index: usize,
    tag: Vec<u8>,
}

/// Check the token's shape. It says nothing about validity; see `verify`.
pub fn parse(token: &str) -> Result<ParsedToken, String> {
    let malformed = || "malformed answer token".to_string();
    let mut parts = token.trim().split('.');
    let (Some(toast_id), Some(idx), Some(tag), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(malformed());
    };
    let is_hex = |s: &str| {
        s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if toast_id.len() != TOAST_ID_HEX || !is_hex(toast_id) || tag.len() != MAC_HEX || !is_hex(tag) {
        return Err(malformed());
    }
    if idx.is_empty() || idx.len() > 2 || !idx.bytes().all(|b| b.is_ascii_digit()) {
        return Err(malformed());
    }
    let option_index = idx.parse().map_err(|_| malformed())?;
    let tag = hex::decode(tag).map_err(|_| malformed())?;
    Ok(ParsedToken {
        toast_id: toast_id.to_string(),
        option_index,
        tag,
    })
}

/// True when the token's MAC matches this ask (constant-time comparison).
pub fn verify(secret: &[u8], ask_id: &str, token: &ParsedToken) -> bool {
    mac(secret, ask_id, token.option_index, &token.toast_id)
        .verify_slice(&token.tag)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_token_parses_and_verifies() {
        let secret = new_secret();
        let toast = new_toast_id();
        let t = mint(&secret, "ask_1", 2, &toast);
        let p = parse(&t).unwrap();
        assert_eq!(p.toast_id, toast);
        assert_eq!(p.option_index, 2);
        assert!(verify(&secret, "ask_1", &p));
    }

    #[test]
    fn a_token_can_not_move_to_another_option_ask_or_secret() {
        let secret = new_secret();
        let toast = new_toast_id();
        let t = mint(&secret, "ask_1", 0, &toast);
        let moved = t.replacen(&format!("{toast}.0."), &format!("{toast}.1."), 1);
        assert!(!verify(&secret, "ask_1", &parse(&moved).unwrap()));
        assert!(!verify(&secret, "ask_2", &parse(&t).unwrap()));
        assert!(!verify(&new_secret(), "ask_1", &parse(&t).unwrap()));
        let mut flipped = t.clone();
        let last = flipped.pop().unwrap();
        flipped.push(if last == '0' { '1' } else { '0' });
        assert!(!verify(&secret, "ask_1", &parse(&flipped).unwrap()));
    }

    #[test]
    fn malformed_tokens_are_refused() {
        let secret = new_secret();
        let good = mint(&secret, "a", 0, &new_toast_id());
        for bad in [
            "",
            "x",
            "a.b.c",
            &format!("{good}.0"),
            &good.to_uppercase(),
            &good.replacen(".0.", ".-1.", 1),
            &good.replacen(".0.", "..", 1),
            &good.replacen(".0.", ".123.", 1),
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be refused");
        }
    }
}
