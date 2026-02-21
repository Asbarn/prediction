use std::fmt;

/// Venue API credentials loaded from environment variables.
///
/// All fields are optional in Phase 1 since no venue connections are
/// established yet. In Phase 2+, specific feeds will require their
/// credentials and return `ConfigError::MissingEnvVar` if absent.
///
/// Credentials are NEVER stored in config files -- only environment variables.
#[derive(Clone)]
pub struct Credentials {
    pub deribit_api_key: Option<String>,
    pub deribit_api_secret: Option<String>,
    pub polymarket_private_key: Option<String>,
    pub kalshi_email: Option<String>,
    pub kalshi_password: Option<String>,
}

/// Load credentials from environment variables.
///
/// All credentials are optional in Phase 1. The `ConfigError::MissingEnvVar`
/// variant exists for Phase 2+ when specific feeds require credentials.
pub fn load_credentials() -> Credentials {
    Credentials {
        deribit_api_key: std::env::var("DERIBIT_API_KEY").ok(),
        deribit_api_secret: std::env::var("DERIBIT_API_SECRET").ok(),
        polymarket_private_key: std::env::var("POLYMARKET_PRIVATE_KEY").ok(),
        kalshi_email: std::env::var("KALSHI_EMAIL").ok(),
        kalshi_password: std::env::var("KALSHI_PASSWORD").ok(),
    }
}

/// Custom Debug implementation that redacts secret values.
///
/// Shows "***" for present values and "None" for absent ones,
/// preventing accidental exposure of credentials in logs or error messages.
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn redact(opt: &Option<String>) -> &str {
            match opt {
                Some(_) => "***",
                None => "None",
            }
        }

        f.debug_struct("Credentials")
            .field("deribit_api_key", &redact(&self.deribit_api_key))
            .field("deribit_api_secret", &redact(&self.deribit_api_secret))
            .field("polymarket_private_key", &redact(&self.polymarket_private_key))
            .field("kalshi_email", &redact(&self.kalshi_email))
            .field("kalshi_password", &redact(&self.kalshi_password))
            .finish()
    }
}
