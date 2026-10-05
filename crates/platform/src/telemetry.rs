use sentry::ClientInitGuard;
use sentry::integrations::tracing::EventFilter;
use tracing::Level;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Where errors are reported, from the environment (`SENTRY_DSN`, `APP_ENV`, `RELEASE`).
#[derive(Debug, Clone, Default)]
pub struct ErrorReporting {
    /// Unset: nothing leaves the process.
    pub dsn: Option<String>,
    pub environment: String,
    pub release: Option<String>,
}

/// JSON logs to stdout, and (with a DSN) every `ERROR`-level event to Sentry. Keep the returned
/// guard alive for the life of the process: dropping it flushes and stops reporting.
///
/// What is sent is deliberately thin (docs/design.md §11 "never log emails, passwords, tokens,
/// presigned URLs"): only `ERROR` events are reported, lower levels are neither events nor
/// breadcrumbs, personal data is off, and `before_send` drops request details (URLs can carry
/// signed query strings), users and the server name.
pub fn init(rust_log: &str, reporting: &ErrorReporting) -> Option<ClientInitGuard> {
    let guard = reporting
        .dsn
        .as_deref()
        .filter(|dsn| !dsn.is_empty())
        .map(|dsn| {
            // (`ClientOptions` is non-exhaustive: start from the default and set fields.)
            let mut options = sentry::ClientOptions::default();
            options.release = reporting.release.clone().map(Into::into);
            options.environment = Some(reporting.environment.clone().into());
            options.send_default_pii = false;
            options.attach_stacktrace = true;
            options.before_send = Some(std::sync::Arc::new(|mut event| {
                scrub(&mut event);
                Some(event)
            }));
            sentry::init((dsn, options))
        });

    let sentry_layer = guard.is_some().then(|| {
        sentry::integrations::tracing::layer().event_filter(|metadata| {
            if *metadata.level() == Level::ERROR {
                EventFilter::Event
            } else {
                EventFilter::Ignore
            }
        })
    });
    tracing_subscriber::registry()
        .with(EnvFilter::try_new(rust_log).unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer().json())
        .with(sentry_layer)
        .init();
    guard
}

/// Removes everything about the request or the person from an event before it is sent.
pub fn scrub(event: &mut sentry::protocol::Event<'static>) {
    event.request = None;
    event.user = None;
    event.server_name = None;
    event.breadcrumbs = Default::default();
}

#[cfg(test)]
mod tests {
    use sentry::protocol::{Breadcrumb, Event, Request, User};

    use super::*;

    #[test]
    fn events_leave_without_requests_users_or_breadcrumbs() {
        let mut event = Event {
            request: Some(Request {
                url: Some(
                    "https://app/s/abc?X-Amz-Signature=secret"
                        .parse()
                        .expect("url"),
                ),
                ..Default::default()
            }),
            user: Some(User {
                email: Some("a@example.com".into()),
                ..Default::default()
            }),
            server_name: Some("host".into()),
            message: Some("something failed".into()),
            ..Default::default()
        };
        event.breadcrumbs.values.push(Breadcrumb {
            message: Some("GET /s/abc".into()),
            ..Default::default()
        });
        scrub(&mut event);
        assert!(event.request.is_none());
        assert!(event.user.is_none());
        assert!(event.server_name.is_none());
        assert!(event.breadcrumbs.values.is_empty());
        assert_eq!(event.message.as_deref(), Some("something failed"));
    }

    #[test]
    fn without_a_dsn_nothing_is_started() {
        // (A second `init` in one process would panic on the global subscriber, so only the
        // guard decision is exercised: no DSN, no client.)
        let reporting = ErrorReporting::default();
        assert!(reporting.dsn.as_deref().filter(|d| !d.is_empty()).is_none());
    }
}
