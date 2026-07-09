use axum::http::HeaderValue;
use axum::http::Method;
use axum::http::header;
use tower_http::cors::CorsLayer;

/// A configured origin failed to parse as an HTTP header value.
#[derive(Debug, thiserror::Error)]
#[error("Invalid CORS origin {origin:?}: {source}")]
pub struct InvalidOrigin {
    origin: String,
    #[source]
    source: header::InvalidHeaderValue,
}

/// Builds a `CorsLayer` restricted to the given origins.
///
/// No wildcard support: every allowed origin must be listed explicitly, and
/// only the methods and headers this codebase's routers actually use are
/// permitted.
///
/// # Errors
/// `InvalidOrigin` — an origin string is not a valid HTTP header value.
pub fn cors_layer(allowed_origins: &[String]) -> Result<CorsLayer, InvalidOrigin> {
    let origins = allowed_origins
        .iter()
        .map(|origin| {
            HeaderValue::from_str(origin).map_err(|source| InvalidOrigin {
                origin: origin.clone(),
                source,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cors_layer_accepts_well_formed_origins() {
        let origins = vec![
            "https://chat.example.com".to_string(),
            "https://admin.example.com".to_string(),
        ];

        assert!(cors_layer(&origins).is_ok());
    }

    #[test]
    fn cors_layer_rejects_an_origin_with_invalid_header_bytes() {
        let origins = vec!["https://chat.example.com\n".to_string()];

        let result = cors_layer(&origins);

        assert!(matches!(result, Err(InvalidOrigin { .. })));
    }
}
