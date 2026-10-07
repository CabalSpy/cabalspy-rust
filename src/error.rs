//! Error types.
//!
//! Every failure is an [`Error`]. Match on the variant to distinguish a wallet
//! that is simply not tracked from a rate limit or an exhausted credit balance.

use serde::Deserialize;
use thiserror::Error;

use crate::models::DemoUpgrade;

/// The `error` object the API returns on a failed request.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ApiErrorBody {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    /// Set on validation failures: which parameter was wrong.
    #[serde(default)]
    pub parameter: Option<String>,
    /// Set on validation failures: which values would have been accepted.
    #[serde(default)]
    pub allowed: Option<Vec<serde_json::Value>>,
    /// Set on `demo_limit_reached`: seconds until the daily demo budget resets.
    #[serde(default)]
    pub resets_in_seconds: Option<u64>,
}

/// Rate limit state parsed from the `X-RateLimit-*` response headers.
#[derive(Debug, Clone, Copy, Default)]
pub struct RateLimit {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    /// Unix seconds at which the current minute window resets.
    pub reset: Option<u64>,
}

#[derive(Debug, Error)]
pub enum Error {
    /// 400 — `missing_parameter`, `invalid_parameter` or `invalid_body`.
    #[error("bad request: {}{}", body.message, fmt_parameter(&body.parameter))]
    BadRequest { body: ApiErrorBody, rate_limit: RateLimit },

    /// 401 — `missing_api_key`.
    #[error("authentication failed: {}", body.message)]
    Authentication { body: ApiErrorBody, rate_limit: RateLimit },

    /// 403 — `invalid_api_key`.
    #[error("forbidden: {}", body.message)]
    Permission { body: ApiErrorBody, rate_limit: RateLimit },

    /// 403 — `insufficient_credits`. Separate so billing can be handled on its own.
    #[error("insufficient credits: {}", body.message)]
    InsufficientCredits { body: ApiErrorBody, rate_limit: RateLimit },

    /// 404 — `wallet_not_found` or `token_not_found`.
    #[error("not found: {}", body.message)]
    NotFound { body: ApiErrorBody, rate_limit: RateLimit },

    /// 429 — `rate_limit_exceeded`. `retry_after` carries the server's hint in seconds.
    #[error("rate limited: {} (retry after {:?}s)", body.message, retry_after)]
    RateLimited {
        body: ApiErrorBody,
        rate_limit: RateLimit,
        retry_after: Option<u64>,
    },

    /// 429 — `demo_limit_reached`. The public demo key's daily budget (20 requests
    /// per IP per UTC day) is spent. Retrying will not help before the reset; get a
    /// free test key at <https://apidashboard.cabalspy.xyz/> or pay per call with x402.
    #[error(
        "demo limit reached: {} (resets in {:?}s; free test key at https://apidashboard.cabalspy.xyz/)",
        body.message,
        resets_in_seconds
    )]
    DemoLimit {
        body: ApiErrorBody,
        rate_limit: RateLimit,
        resets_in_seconds: Option<u64>,
        /// Upgrade links the server sent with the error.
        upgrade: Option<DemoUpgrade>,
    },

    /// 5xx — `internal_error` or `service_unavailable`.
    #[error("server error {status}: {}", body.message)]
    Server {
        status: u16,
        body: ApiErrorBody,
        rate_limit: RateLimit,
    },

    /// Any other non-success status.
    #[error("unexpected status {status}: {}", body.message)]
    Status {
        status: u16,
        body: ApiErrorBody,
        rate_limit: RateLimit,
    },

    /// Rejected before the request was sent, for example a wallet type the chain
    /// does not have, or a batch larger than the server accepts.
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },

    /// Network failure, timeout or TLS problem.
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),

    /// The body was not valid JSON, or did not have the expected envelope.
    #[error("could not decode response: {0}")]
    Decode(String),

    /// No API key was supplied and `CABALSPY_API_KEY` was not set.
    #[error("missing API key: pass it to the builder or set CABALSPY_API_KEY")]
    MissingApiKey,
}

fn fmt_parameter(parameter: &Option<String>) -> String {
    match parameter {
        Some(name) => format!(" (parameter: {name})"),
        None => String::new(),
    }
}

impl Error {
    /// The API's machine readable error code, when the failure came from the API.
    pub fn code(&self) -> Option<&str> {
        self.body().map(|b| b.code.as_str())
    }

    /// The request id, useful when reporting a problem to support.
    pub fn request_id(&self) -> Option<&str> {
        self.body().and_then(|b| b.request_id.as_deref())
    }

    /// Which parameter the API rejected, on validation failures.
    pub fn parameter(&self) -> Option<&str> {
        self.body().and_then(|b| b.parameter.as_deref())
    }

    /// Which values the API would have accepted, on validation failures.
    pub fn allowed(&self) -> Option<&[serde_json::Value]> {
        self.body().and_then(|b| b.allowed.as_deref())
    }

    /// The rate limit state at the time of the failure.
    pub fn rate_limit(&self) -> Option<RateLimit> {
        match self {
            Error::BadRequest { rate_limit, .. }
            | Error::Authentication { rate_limit, .. }
            | Error::Permission { rate_limit, .. }
            | Error::InsufficientCredits { rate_limit, .. }
            | Error::NotFound { rate_limit, .. }
            | Error::RateLimited { rate_limit, .. }
            | Error::DemoLimit { rate_limit, .. }
            | Error::Server { rate_limit, .. }
            | Error::Status { rate_limit, .. } => Some(*rate_limit),
            _ => None,
        }
    }

    /// True for failures where retrying with backoff is worthwhile.
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::RateLimited { .. } | Error::Server { .. } => true,
            Error::Transport(err) => err.is_timeout() || err.is_connect() || err.is_request(),
            _ => false,
        }
    }

    fn body(&self) -> Option<&ApiErrorBody> {
        match self {
            Error::BadRequest { body, .. }
            | Error::Authentication { body, .. }
            | Error::Permission { body, .. }
            | Error::InsufficientCredits { body, .. }
            | Error::NotFound { body, .. }
            | Error::RateLimited { body, .. }
            | Error::DemoLimit { body, .. }
            | Error::Server { body, .. }
            | Error::Status { body, .. } => Some(body),
            _ => None,
        }
    }
}

/// Maps an HTTP status and error body onto the right [`Error`] variant.
pub(crate) fn error_from_status(
    status: u16,
    body: ApiErrorBody,
    rate_limit: RateLimit,
    retry_after: Option<u64>,
) -> Error {
    match status {
        400 => Error::BadRequest { body, rate_limit },
        401 => Error::Authentication { body, rate_limit },
        403 => {
            if body.code == "insufficient_credits" {
                Error::InsufficientCredits { body, rate_limit }
            } else {
                Error::Permission { body, rate_limit }
            }
        }
        404 => Error::NotFound { body, rate_limit },
        429 if body.code == "demo_limit_reached" => Error::DemoLimit {
            resets_in_seconds: body.resets_in_seconds.or(retry_after),
            body,
            rate_limit,
            upgrade: None,
        },
        429 => Error::RateLimited {
            body,
            rate_limit,
            retry_after,
        },
        s if s >= 500 => Error::Server {
            status,
            body,
            rate_limit,
        },
        _ => Error::Status {
            status,
            body,
            rate_limit,
        },
    }
}

/// Maps a failed response body onto an [`Error`], falling back to a generic body
/// when the server did not send JSON.
pub(crate) fn error_from_body(
    status: u16,
    text: &str,
    fallback_message: String,
    rate_limit: RateLimit,
    retry_after: Option<u64>,
) -> Error {
    let (body, upgrade) = match serde_json::from_str::<crate::models::RawError>(text) {
        Ok(raw) => (raw.error, raw.upgrade),
        Err(_) => (
            ApiErrorBody {
                code: format!("http_{status}"),
                message: fallback_message,
                ..Default::default()
            },
            None,
        ),
    };
    let mut error = error_from_status(status, body, rate_limit, retry_after);
    if let Error::DemoLimit { upgrade: slot, .. } = &mut error {
        *slot = upgrade.and_then(|value| serde_json::from_value(value).ok());
    }
    error
}

/// Shorthand for results returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO_LIMIT_BODY: &str = r#"{
        "success": false,
        "error": {
            "code": "demo_limit_reached",
            "message": "The public demo allows 20 requests per IP per day.",
            "resets_in_seconds": 3600
        },
        "demo": true,
        "upgrade": {
            "test_key": "https://apidashboard.cabalspy.xyz/",
            "pay_per_call": "https://www.cabalspy.xyz/x402/",
            "docs": "https://docs.cabalspy.xyz"
        }
    }"#;

    #[test]
    fn demo_limit_maps_to_its_own_variant() {
        let error = error_from_body(429, DEMO_LIMIT_BODY, "x".into(), RateLimit::default(), None);
        match &error {
            Error::DemoLimit {
                body,
                resets_in_seconds,
                upgrade,
                ..
            } => {
                assert_eq!(body.code, "demo_limit_reached");
                assert_eq!(*resets_in_seconds, Some(3600));
                let upgrade = upgrade.as_ref().expect("upgrade links");
                assert_eq!(
                    upgrade.test_key.as_deref(),
                    Some("https://apidashboard.cabalspy.xyz/")
                );
                assert_eq!(
                    upgrade.pay_per_call.as_deref(),
                    Some("https://www.cabalspy.xyz/x402/")
                );
            }
            other => panic!("expected DemoLimit, got {other:?}"),
        }
        assert_eq!(error.code(), Some("demo_limit_reached"));
        assert!(!error.is_retryable());
        assert!(error.to_string().contains("demo limit reached"));
    }

    #[test]
    fn other_429s_stay_rate_limited() {
        let body = r#"{"success":false,"error":{"code":"rate_limit_exceeded","message":"slow down"}}"#;
        let error = error_from_body(429, body, "x".into(), RateLimit::default(), Some(7));
        assert!(matches!(
            error,
            Error::RateLimited {
                retry_after: Some(7),
                ..
            }
        ));
        assert!(error.is_retryable());
    }

    #[test]
    fn non_json_bodies_fall_back() {
        let error = error_from_body(
            502,
            "<html>bad gateway</html>",
            "HTTP 502 on GET /x".into(),
            RateLimit::default(),
            None,
        );
        assert!(matches!(error, Error::Server { status: 502, .. }));
        assert_eq!(error.code(), Some("http_502"));
    }

    #[test]
    fn insufficient_credits_is_unchanged() {
        let body = r#"{"error":{"code":"insufficient_credits","message":"top up"}}"#;
        let error = error_from_body(403, body, "x".into(), RateLimit::default(), None);
        assert!(matches!(error, Error::InsufficientCredits { .. }));
    }
}
