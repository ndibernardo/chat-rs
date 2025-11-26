//! Authentication utilities library
//!
//! Provides reusable authentication infrastructure for microservices:
//! - Password hashing (Argon2id)
//! - JWT token generation and validation
//! - Authentication coordination
//!
//! Each service defines its own authentication traits and adapts these implementations.
//! This avoids coupling services through shared domain logic while reducing code duplication.
//!
//! # Examples
//!
//! ## Password Hashing
//! ```
//! use auth::PasswordHasher;
//!
//! let hasher = PasswordHasher::new();
//! let hash = hasher.hash("my_password").unwrap();
//! let is_valid = hasher.verify("my_password", &hash).unwrap();
//! assert!(is_valid);
//! ```
//!
//! ## JWT Tokens
//! ```
//! use auth::{JwtHandler, Claims};
//!
//! let handler = JwtHandler::new(b"secret_key_at_least_32_bytes_long!");
//! let claims = Claims::new().with_subject("user123");
//! let token = handler.encode(&claims).unwrap();
//! let decoded: Claims = handler.decode(&token).unwrap();
//! ```
//!
//! ## Complete Authentication Flow
//! ```
//! use auth::{Authenticator, Claims};
//!
//! let auth = Authenticator::new(b"secret_key_at_least_32_bytes_long!");
//!
//! // Register: hash password
//! let hash = auth.hash_password("password123").unwrap();
//!
//! // Login: verify and generate token
//! let claims = Claims::for_user("user123", "alice".to_string(), 24);
//! let result = auth.authenticate("password123", &hash, &claims).unwrap();
//! println!("Token: {}", result.access_token);
//!
//! // Validate token
//! let decoded: Claims = auth.validate_token(&result.access_token).unwrap();
//! ```

pub mod authenticator;
pub mod jwt;
pub mod password;

// Re-export commonly used items
pub use authenticator::AuthenticationError;
pub use authenticator::AuthenticationResult;
pub use authenticator::Authenticator;
pub use jwt::Claims;
pub use jwt::JwtError;
pub use jwt::JwtHandler;
pub use password::PasswordError;
pub use password::PasswordHasher;
