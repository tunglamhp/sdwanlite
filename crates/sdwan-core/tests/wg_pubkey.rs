//! WireGuard public_key validator tests (P0-6 fix).
//!
//! X25519 keys are 32 bytes; base64-encoded they are exactly 44 characters.
//! The validator must reject: wrong length, non-base64 chars, and base64
//! strings that decode to a non-32-byte payload.

use sdwan_core::{validate_public_key, PublicKey, ValidationError};

/// Real base64 of 32 zero bytes (32 * 4/3 = 42.67 -> padded to 44 chars).
const VALID_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

#[test]
fn accepts_real_32_byte_base64() {
    validate_public_key(VALID_KEY).expect("VALID_KEY should pass");
    let k = PublicKey::try_from_str(VALID_KEY).unwrap();
    assert_eq!(k.as_str(), VALID_KEY);
}

#[test]
fn rejects_short_key() {
    let s = "AAAA";
    match validate_public_key(s).unwrap_err() {
        ValidationError::PublicKeyLength { actual, expected } => {
            assert_eq!(actual, 4);
            assert_eq!(expected, 44);
        }
        e => panic!("expected length error, got {e:?}"),
    }
}

#[test]
fn rejects_long_key() {
    let s = "A".repeat(64);
    assert!(matches!(
        validate_public_key(&s).unwrap_err(),
        ValidationError::PublicKeyLength { actual: 64, expected: 44 }
    ));
}

#[test]
fn rejects_non_base64_charset() {
    // 44 chars but contains '!' which is outside the base64 alphabet.
    let mut s: String = "A".repeat(43);
    s.push('!'); // 44th byte invalid
    match validate_public_key(&s).unwrap_err() {
        ValidationError::PublicKeyCharset { position, byte } => {
            assert_eq!(position, 43);
            assert_eq!(byte, b'!');
        }
        e => panic!("expected charset error, got {e:?}"),
    }
}

#[test]
fn rejects_decoded_length_mismatch() {
    // 44 chars (length-valid, charset-valid) but decodes to 33 bytes, not 32.
    // 33 zero bytes → base64 "A" * 44, exactly 44 chars, no padding.
    let s = "A".repeat(44);
    match validate_public_key(&s).unwrap_err() {
        ValidationError::PublicKeyDecodedLength { len } => {
            assert_eq!(len, 33);
        }
        e => panic!("expected decoded-length error, got {e:?}"),
    }
}

#[test]
fn public_key_newtype_round_trip() {
    let k = PublicKey::try_from_str(VALID_KEY).unwrap();
    assert_eq!(k.as_str(), VALID_KEY);
}
