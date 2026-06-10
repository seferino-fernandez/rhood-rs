use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

/// Verify a PKCE S256 code challenge.
/// Returns true if `BASE64URL_NO_PAD(SHA256(verifier)) == challenge`.
pub fn verify_s256(verifier: &str, challenge: &str) -> bool {
    let hash = Sha256::digest(verifier.as_bytes());
    let computed = URL_SAFE_NO_PAD.encode(hash);
    computed == challenge
}

#[cfg(test)]
pub fn challenge_from_verifier(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_s256_verifier() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = challenge_from_verifier(verifier);
        assert!(verify_s256(verifier, &challenge));
    }

    #[test]
    fn wrong_verifier_fails() {
        let verifier = "correct-verifier";
        let challenge = challenge_from_verifier(verifier);
        assert!(!verify_s256("wrong-verifier", &challenge));
    }

    #[test]
    fn rfc7636_appendix_b_vector() {
        // RFC 7636 Appendix B test vector
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected_challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(verify_s256(verifier, expected_challenge));
    }
}
