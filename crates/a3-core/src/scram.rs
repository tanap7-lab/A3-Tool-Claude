//! Builds a SCRAM-SHA-256 verifier on the client, so a plain password never appears in a
//! CREATE ROLE or ALTER ROLE statement that could reach a server log.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

const ITERATIONS: u32 = 4096;

pub fn verifier(password: &str) -> String {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    verifier_with_salt(password, &salt, ITERATIONS)
}

fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(msg);
    m.finalize().into_bytes().into()
}

fn verifier_with_salt(password: &str, salt: &[u8], iterations: u32) -> String {
    let mut salted = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut salted);
    let client_key = hmac(&salted, b"Client Key");
    let stored_key: [u8; 32] = Sha256::digest(client_key).into();
    let server_key = hmac(&salted, b"Server Key");
    format!(
        "SCRAM-SHA-256${iterations}:{}${}:{}",
        B64.encode(salt),
        B64.encode(stored_key),
        B64.encode(server_key)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_has_server_format() {
        let v = verifier("correct horse");
        let re = regex::Regex::new(r"^SCRAM-SHA-256\$4096:[A-Za-z0-9+/=]+\$[A-Za-z0-9+/=]+:[A-Za-z0-9+/=]+$").unwrap();
        assert!(re.is_match(&v), "{v}");
    }

    #[test]
    fn salts_differ() {
        assert_ne!(verifier("x"), verifier("x"));
    }
}
