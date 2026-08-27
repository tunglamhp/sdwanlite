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
    // base64 of "ABCDEFGHIJ" -> 10 bytes when decoded -> not 32
    // Pad to 44 chars using a base64-shaped string that decodes short.
    // We construct "QUFB" repeated and pad to decode to fewer than 32 bytes.
    let short_payload = b"hello"; // 5 bytes -> base64 "aGVsbG8=" length 8
    // Force 44 chars by padding with valid base64 chars.
    let s = "aGVsbG8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"; // 44 chars
    // This decodes to 5 + (44-8) bytes depending on padding alignment.
    // Just check we get a decode-length OR decode error.
    let err = validate_public_key(s).unwrap_err();
    assert!(
        matches!(
            err,
            ValidationError::PublicKeyDecodedLength { .. } | ValidationError::PublicKeyDecode(_)
        ),
        "unexpected error: {err:?}"
    );
}

#[test]
fn public_key_newtype_round_trip() {
    let k = PublicKey::try_from_str(VALID_KEY).unwrap();
    assert_eq!(k.as_str(), VALID_KEY);
}
