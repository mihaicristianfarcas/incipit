//! Checking an ID token from the sign-in: its RS256 signature against the issuer's published keys
//! (JWKS), then the claims that tie it to this sign-in (issuer, audience, expiry, nonce). Also the
//! small pieces of randomness and encoding the sign-in needs (state, nonce, PKCE, a host id).

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;

/// `n` random bytes; None only if the system has no randomness to give, which no supported
/// system lacks.
fn random(n: usize) -> Option<Vec<u8>> {
    let mut bytes = vec![0u8; n];
    SystemRandom::new().fill(&mut bytes).ok()?;
    Some(bytes)
}

/// A fresh random value for `state`, `nonce` or a PKCE verifier: 32 bytes, base64url.
pub fn fresh() -> Option<String> {
    random(32).map(|b| URL_SAFE_NO_PAD.encode(b))
}

/// The PKCE challenge of `verifier`: base64url of its SHA-256, without padding.
pub fn challenge(verifier: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.as_ref())
}

/// A new host id: an opaque, random UUID URN, tied to no one.
pub fn host_id() -> Option<String> {
    let mut b = random(16)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Some(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

fn decode(part: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .or_else(|_| STANDARD.decode(part))
        .map_err(|_| "a part of the token is not base64".to_string())
}

/// The claims of a token, unchecked: for reading a token the issuer just handed over.
pub fn claims(token: &str) -> Result<Value, String> {
    let payload = token.split('.').nth(1).ok_or("the token has no claims")?;
    serde_json::from_slice(&decode(payload)?).map_err(|_| "the token's claims are not JSON".into())
}

/// The claims of `token` if an RSA key in `jwks` signed it with RS256.
pub fn verify(token: &str, jwks: &Value) -> Result<Value, String> {
    let parts: Vec<&str> = token.split('.').collect();
    let [head, body, signature] = parts[..] else {
        return Err("the ID token is not a signed token".into());
    };
    let header: Value =
        serde_json::from_slice(&decode(head)?).map_err(|_| "the ID token's header is not JSON")?;
    if header["alg"] != "RS256" {
        return Err(format!(
            "the ID token is signed with {}, not RS256",
            header["alg"]
        ));
    }
    let keys = jwks["keys"]
        .as_array()
        .ok_or("the issuer published no keys")?;
    let key = keys
        .iter()
        .filter(|k| k["kty"] == "RSA")
        .find(|k| header["kid"].is_null() || k["kid"] == header["kid"])
        .ok_or("no published key matches the ID token")?;
    let n = decode(key["n"].as_str().ok_or("a published key has no modulus")?)?;
    let e = decode(key["e"].as_str().ok_or("a published key has no exponent")?)?;
    let public = ring::signature::RsaPublicKeyComponents { n: &n, e: &e };
    public
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{head}.{body}").as_bytes(),
            &decode(signature)?,
        )
        .map_err(|_| "the ID token's signature does not match the issuer's key")?;
    claims(token)
}

/// Whether `claims` belong to this sign-in: from `issuer`, for `client`, not expired at `now`
/// (seconds since 1970), and carrying `nonce`.
pub fn check(
    claims: &Value,
    issuer: &str,
    client: &str,
    nonce: &str,
    now: u64,
) -> Result<(), String> {
    if claims["iss"].as_str().map(|i| i.trim_end_matches('/')) != Some(issuer.trim_end_matches('/'))
    {
        return Err("the ID token is from another issuer".into());
    }
    let audience = match &claims["aud"] {
        Value::String(one) => one == client,
        Value::Array(many) => many.iter().any(|a| a == client),
        _ => false,
    };
    if !audience {
        return Err("the ID token is for another app".into());
    }
    if claims["exp"].as_u64().is_none_or(|exp| exp <= now) {
        return Err("the ID token has expired".into());
    }
    if claims["nonce"].as_str() != Some(nonce) {
        return Err("the ID token is from another sign-in".into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod testing {
    //! A key pair for signing test tokens. It was made for these tests alone and protects nothing.

    use super::*;
    use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

    const KEY: &[&str] = &include!("test_key.in");

    fn pair() -> RsaKeyPair {
        let der = STANDARD.decode(KEY.concat()).unwrap();
        RsaKeyPair::from_pkcs8(&der).unwrap()
    }

    /// `claims` as an RS256 token signed by the test key, under the key id "k1".
    pub fn sign(claims: &Value) -> String {
        let head = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","kid":"k1","typ":"JWT"}"#);
        let body = URL_SAFE_NO_PAD.encode(claims.to_string());
        let input = format!("{head}.{body}");
        let pair = pair();
        let mut signature = vec![0u8; pair.public().modulus_len()];
        pair.sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            input.as_bytes(),
            &mut signature,
        )
        .unwrap();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
    }

    /// The test key, published as a JWKS.
    pub fn jwks() -> Value {
        let parts: ring::rsa::PublicKeyComponents<Vec<u8>> = pair().public().into();
        serde_json::json!({ "keys": [{
            "kty": "RSA", "kid": "k1", "alg": "RS256", "use": "sig",
            "n": URL_SAFE_NO_PAD.encode(&parts.n),
            "e": URL_SAFE_NO_PAD.encode(&parts.e),
        }]})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn claims_for(nonce: &str) -> Value {
        json!({ "iss": "https://auth.example", "aud": ["oaiapp_1"], "exp": 2_000, "nonce": nonce, "sub": "u" })
    }

    #[test]
    fn a_token_signed_by_the_published_key_is_accepted() {
        let token = testing::sign(&claims_for("n1"));
        let claims = verify(&token, &testing::jwks()).unwrap();
        assert!(check(&claims, "https://auth.example/", "oaiapp_1", "n1", 1_000).is_ok());
    }

    #[test]
    fn a_token_whose_claims_were_changed_is_refused() {
        let token = testing::sign(&claims_for("n1"));
        let mut parts: Vec<String> = token.split('.').map(String::from).collect();
        parts[1] = URL_SAFE_NO_PAD.encode(claims_for("n2").to_string());
        assert!(verify(&parts.join("."), &testing::jwks()).is_err());
    }

    #[test]
    fn claims_from_another_sign_in_app_or_time_are_refused() {
        let claims = claims_for("n1");
        assert!(check(&claims, "https://other.example", "oaiapp_1", "n1", 1_000).is_err());
        assert!(check(&claims, "https://auth.example", "oaiapp_2", "n1", 1_000).is_err());
        assert!(check(&claims, "https://auth.example", "oaiapp_1", "n2", 1_000).is_err());
        assert!(check(&claims, "https://auth.example", "oaiapp_1", "n1", 3_000).is_err());
    }

    #[test]
    fn the_pkce_challenge_is_the_rfc_7636_example() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_host_id_is_a_random_version_4_uuid() {
        let id = host_id().unwrap();
        assert!(id.starts_with("urn:uuid:"));
        assert_eq!(id.len(), "urn:uuid:".len() + 36);
        assert_eq!(&id[23..24], "4");
        assert_ne!(id, host_id().unwrap());
    }
}
