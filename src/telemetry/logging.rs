use tracing_subscriber::{fmt, EnvFilter};

/// Initialize tracing with optional env-level and JSON formatting.
pub fn init_tracing(level: String, _json: bool) {
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = fmt().with_env_filter(filter).with_target(false);
    // `tracing_subscriber` exposes JSON formatting behind feature flags; to keep
    // this example robust across environments we currently always use the
    // human-readable formatter. The `json` parameter is accepted for API
    // compatibility and may be wired to `.json()` if the feature is enabled.
    subscriber.init();
}
