use tracing_subscriber::EnvFilter;

pub fn init(rust_log: &str) {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::try_new(rust_log).unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
}
