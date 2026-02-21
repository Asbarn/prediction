/// Configuration loading and validation errors.
///
/// Covers all config failure modes: file I/O, TOML parsing (with line/column
/// info from the `toml` 0.8 crate), semantic validation, and missing env vars.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {file}: {source}")]
    ReadFile {
        file: String,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse {file}: {source}")]
    ParseToml {
        file: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("validation error in {file}: {message}")]
    Validation {
        file: String,
        message: String,
    },

    #[error("missing environment variable: {var}")]
    MissingEnvVar {
        var: String,
    },
}
