//! Ids, tokens and hashes.

use rand::Rng;
use sha2::{Digest, Sha256};

/// Lowercase Crockford-style base32 (no i, l, o, u) so ids are easy to read aloud and type.
const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// A new 16-character random id (80 bits). Clients can create ids offline without clashes.
pub fn new_id() -> String {
    let mut rng = rand::rng();
    (0..16)
        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
        .collect()
}

/// A long-lived device token: `aos_` plus 64 hex characters (256 bits).
pub fn new_token() -> String {
    let bytes: [u8; 32] = rand::random();
    format!("aos_{}", hex(&bytes))
}

/// A 6-digit pairing code, zero padded.
pub fn new_pairing_code() -> String {
    format!("{:06}", rand::rng().random_range(0..1_000_000u32))
}

/// SHA-256 of `input` as lowercase hex. Tokens and codes are stored only as hashes.
pub fn sha256_hex(input: &str) -> String {
    hex(&Sha256::digest(input.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_have_expected_shape() {
        let id = new_id();
        assert_eq!(id.len(), 16);
        assert!(id.bytes().all(|b| ALPHABET.contains(&b)));
        assert_ne!(new_id(), new_id());
    }

    #[test]
    fn tokens_and_codes() {
        let t = new_token();
        assert!(t.starts_with("aos_") && t.len() == 68);
        let c = new_pairing_code();
        assert_eq!(c.len(), 6);
        assert!(c.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(sha256_hex("abc").len(), 64);
    }
}
