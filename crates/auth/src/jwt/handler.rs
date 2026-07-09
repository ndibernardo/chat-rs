use jsonwebtoken::Algorithm;
use jsonwebtoken::DecodingKey;
use jsonwebtoken::EncodingKey;
use jsonwebtoken::Header;
use jsonwebtoken::Validation;
use jsonwebtoken::decode;
use jsonwebtoken::encode;
use jsonwebtoken::errors::ErrorKind;
use serde::Deserialize;
use serde::Serialize;

use super::errors::JwtError;

/// JWT token handler using Ed25519 (EdDSA) signatures.
///
/// A handler built from [`JwtHandler::verifier`] holds only a public key: it
/// can decode and validate tokens issued elsewhere, but `encode` returns
/// `Err(JwtError::SigningKeyUnavailable)`. Use [`JwtHandler::signer`] where
/// the service also issues tokens.
pub struct JwtHandler {
    encoding_key: Option<EncodingKey>,
    decoding_key: DecodingKey,
}

impl JwtHandler {
    /// Builds a handler that can both sign and verify tokens.
    ///
    /// # Errors
    /// `InvalidKey` — either PEM fails to parse as an Ed25519 key.
    pub fn signer(private_key_pem: &[u8], public_key_pem: &[u8]) -> Result<Self, JwtError> {
        let encoding_key = EncodingKey::from_ed_pem(private_key_pem)
            .map_err(|e| JwtError::InvalidKey(e.to_string()))?;
        let decoding_key = DecodingKey::from_ed_pem(public_key_pem)
            .map_err(|e| JwtError::InvalidKey(e.to_string()))?;

        Ok(Self {
            encoding_key: Some(encoding_key),
            decoding_key,
        })
    }

    /// Builds a handler that can only verify tokens signed elsewhere.
    ///
    /// # Errors
    /// `InvalidKey` — the PEM fails to parse as an Ed25519 public key.
    pub fn verifier(public_key_pem: &[u8]) -> Result<Self, JwtError> {
        let decoding_key = DecodingKey::from_ed_pem(public_key_pem)
            .map_err(|e| JwtError::InvalidKey(e.to_string()))?;

        Ok(Self {
            encoding_key: None,
            decoding_key,
        })
    }

    /// Encode claims into a JWT token.
    ///
    /// # Errors
    /// `SigningKeyUnavailable` — this handler holds only a public key.
    /// `EncodingFailed` — token encoding failed.
    pub fn encode<T: Serialize>(&self, claims: &T) -> Result<String, JwtError> {
        let encoding_key = self
            .encoding_key
            .as_ref()
            .ok_or(JwtError::SigningKeyUnavailable)?;
        let header = Header::new(Algorithm::EdDSA);

        encode(&header, claims, encoding_key).map_err(|e| JwtError::EncodingFailed(e.to_string()))
    }

    /// Decode and validate a JWT token.
    ///
    /// # Errors
    /// `TokenExpired` — token has expired, or has no `exp` claim.
    /// `DecodingFailed` — signature is invalid or the token is malformed.
    pub fn decode<T: for<'de> Deserialize<'de>>(&self, token: &str) -> Result<T, JwtError> {
        // `Validation::new` defaults to requiring `exp`; keep that default so a
        // token without an expiration is rejected instead of granted a permanent
        // credential.
        let validation = Validation::new(Algorithm::EdDSA);

        let token_data =
            decode::<T>(token, &self.decoding_key, &validation).map_err(|e| match e.kind() {
                ErrorKind::ExpiredSignature => JwtError::TokenExpired,
                ErrorKind::MissingRequiredClaim(claim) if claim == "exp" => JwtError::TokenExpired,
                _ => JwtError::DecodingFailed(e.to_string()),
            })?;

        Ok(token_data.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestClaims {
        sub: String,
        role: String,
        exp: i64,
    }

    // Ed25519 test keypairs (PKCS8 private / SPKI public PEM), generated via:
    //   openssl genpkey -algorithm ed25519 -out priv.pem
    //   openssl pkey -in priv.pem -pubout -out pub.pem
    // Not used anywhere outside this test module.
    const PRIVATE_KEY_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----\n\
        MC4CAQAwBQYDK2VwBCIEIP6JnME9bwmwbdD47xCxd3Sopbc/1L8s0jLUq4ecKox8\n\
        -----END PRIVATE KEY-----\n";
    const PUBLIC_KEY_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
        MCowBQYDK2VwAyEAo2X2xe1SK4wTKPqRQk+27d5mkWyyxkcZAyRVbplPCmM=\n\
        -----END PUBLIC KEY-----\n";

    // A second, unrelated keypair used to prove signatures don't verify
    // across keys.
    const OTHER_PUBLIC_KEY_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----\n\
        MCowBQYDK2VwAyEALLCXsWshxvVjLoYNA8gEMNnX7ZyubKYNhUlWAabQbhw=\n\
        -----END PUBLIC KEY-----\n";

    fn future_exp() -> i64 {
        chrono::Utc::now().timestamp() + 3600
    }

    fn miles_davis_claims() -> TestClaims {
        TestClaims {
            sub: "miles-davis".to_string(),
            role: "platform-engineer".to_string(),
            exp: future_exp(),
        }
    }

    #[test]
    fn signer_encodes_and_decodes_its_own_token() {
        let handler =
            JwtHandler::signer(PRIVATE_KEY_PEM, PUBLIC_KEY_PEM).expect("Valid Ed25519 keypair");
        let claims = miles_davis_claims();

        let token = handler.encode(&claims).expect("Failed to encode token");
        let decoded: TestClaims = handler.decode(&token).expect("Failed to decode token");

        assert_eq!(decoded, claims);
    }

    #[test]
    fn verifier_decodes_a_token_signed_by_the_matching_signer() {
        let signer =
            JwtHandler::signer(PRIVATE_KEY_PEM, PUBLIC_KEY_PEM).expect("Valid Ed25519 keypair");
        let verifier = JwtHandler::verifier(PUBLIC_KEY_PEM).expect("Valid Ed25519 public key");
        let claims = miles_davis_claims();

        let token = signer.encode(&claims).expect("Failed to encode token");
        let decoded: TestClaims = verifier.decode(&token).expect("Failed to decode token");

        assert_eq!(decoded, claims);
    }

    #[test]
    fn verifier_encode_returns_signing_key_unavailable() {
        let verifier = JwtHandler::verifier(PUBLIC_KEY_PEM).expect("Valid Ed25519 public key");

        let result = verifier.encode(&miles_davis_claims());

        assert!(matches!(result, Err(JwtError::SigningKeyUnavailable)));
    }

    #[test]
    fn decode_rejects_token_signed_by_a_different_key() {
        let signer =
            JwtHandler::signer(PRIVATE_KEY_PEM, PUBLIC_KEY_PEM).expect("Valid Ed25519 keypair");
        let other_verifier =
            JwtHandler::verifier(OTHER_PUBLIC_KEY_PEM).expect("Valid Ed25519 public key");

        let token = signer
            .encode(&miles_davis_claims())
            .expect("Failed to encode token");
        let result = other_verifier.decode::<TestClaims>(&token);

        assert!(matches!(result, Err(JwtError::DecodingFailed(_))));
    }

    #[test]
    fn decode_rejects_malformed_token() {
        let handler =
            JwtHandler::signer(PRIVATE_KEY_PEM, PUBLIC_KEY_PEM).expect("Valid Ed25519 keypair");

        let result = handler.decode::<TestClaims>("invalid.token.here");

        assert!(matches!(result, Err(JwtError::DecodingFailed(_))));
    }

    #[test]
    fn signer_rejects_invalid_pem() {
        let result = JwtHandler::signer(b"not a pem", PUBLIC_KEY_PEM);

        assert!(matches!(result, Err(JwtError::InvalidKey(_))));
    }

    #[test]
    fn verifier_rejects_invalid_pem() {
        let result = JwtHandler::verifier(b"not a pem");

        assert!(matches!(result, Err(JwtError::InvalidKey(_))));
    }
}
