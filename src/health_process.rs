//! Process entrypoint that binds operator health probes from environment input.
//!
//! Operators start this process so a load balancer can keep calling GET `/live`
//! and GET `/ready`. Liveness never opens a database connection. Readiness
//! observes `DATABASE_URL` only after accept, and never echoes driver errors.

use crate::health::{
    BacklogHealth, CapabilityHealth, CapabilityState, DataIntegrityHealth, RuntimeHealthSnapshot,
};
use crate::health_http::{
    bind_health_http, handle_health_http_request, health_ready_response,
    health_request_required_capabilities, health_request_requires_readiness_snapshot,
    serve_health_http_with, HealthHttpResponse, HEALTH_HTTP_IO_TIMEOUT,
};
use crate::postgres_health::POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF;
use crate::postgres_health_http::handle_postgres_health_http_request;
use postgres::{Client, NoTls};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::str::FromStr;

/// Full listen-address environment variable. Wins over [`HEALTH_LISTEN_PORT_ENV`].
pub const HEALTH_LISTEN_ADDR_ENV: &str = "HEALTH_LISTEN_ADDR";
/// Platform TCP port environment variable. Binds `0.0.0.0:$PORT` when set alone.
pub const HEALTH_LISTEN_PORT_ENV: &str = "PORT";
/// Optional operational-store URL observed only for GET `/ready`.
pub const HEALTH_DATABASE_URL_ENV: &str = "DATABASE_URL";
/// Optional caller-measured backlog label. Missing means unknown and not ready.
pub const HEALTH_BACKLOG_HEALTH_ENV: &str = "HEALTH_BACKLOG_HEALTH";

/// Fail-closed configuration error for the health-probe process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HealthProcessConfigError {
    /// Neither `HEALTH_LISTEN_ADDR` nor `PORT` was set.
    MissingListenAddress,
    /// `HEALTH_LISTEN_ADDR` was blank, padded, or not a socket address.
    InvalidListenAddress,
    /// `PORT` was blank, padded, or not a TCP port.
    InvalidListenPort,
    /// `DATABASE_URL` was set but was not an unpadded postgres URL or libpq string.
    InvalidDatabaseUrl,
    /// `HEALTH_BACKLOG_HEALTH` was set to an unknown label.
    InvalidBacklogHealth,
}

impl Display for HealthProcessConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MissingListenAddress => {
                "set HEALTH_LISTEN_ADDR to a socket address, or set PORT to a TCP port"
            }
            Self::InvalidListenAddress => {
                "HEALTH_LISTEN_ADDR must be an unpadded host:port socket address"
            }
            Self::InvalidListenPort => "PORT must be an unpadded TCP port from 0 to 65535",
            Self::InvalidDatabaseUrl => {
                "DATABASE_URL must be an unpadded postgres URL or libpq keyword/value string when set"
            }
            Self::InvalidBacklogHealth => {
                "HEALTH_BACKLOG_HEALTH must be within_bounds, stalled, or unknown when set"
            }
        })
    }
}

impl Error for HealthProcessConfigError {}

/// Validated listen and optional store configuration for one health-probe process.
pub struct HealthProcessConfig {
    listen_addr: SocketAddr,
    database_url: Option<String>,
    connect_config: Option<postgres::Config>,
    backlog_health: BacklogHealth,
}

impl HealthProcessConfig {
    /// Return the address the process will bind.
    #[must_use]
    pub const fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// Return the configured store URL without printing it.
    #[must_use]
    pub fn database_url(&self) -> Option<&str> {
        self.database_url.as_deref()
    }

    /// Return the caller-supplied backlog evidence used for readiness.
    #[must_use]
    pub const fn backlog_health(&self) -> BacklogHealth {
        self.backlog_health
    }
}

impl std::fmt::Debug for HealthProcessConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HealthProcessConfig")
            .field("listen_addr", &self.listen_addr)
            .field("database_url_present", &self.database_url.is_some())
            .field("backlog_health", &self.backlog_health)
            .finish_non_exhaustive()
    }
}

/// Parse listen, store, and backlog environment values without starting I/O.
///
/// Unknown, blank, or whitespace-padded values fail closed so a load balancer
/// cannot point at a process that guessed its listen target or store URL.
///
/// # Errors
///
/// Returns [`HealthProcessConfigError`] when required listen input is missing
/// or any provided value is padded, empty, or semantically unknown.
pub fn parse_health_process_config<F>(
    getenv: F,
) -> Result<HealthProcessConfig, HealthProcessConfigError>
where
    F: Fn(&str) -> Option<String>,
{
    let listen_addr = parse_listen_addr(
        getenv(HEALTH_LISTEN_ADDR_ENV),
        getenv(HEALTH_LISTEN_PORT_ENV),
    )?;
    let (database_url, connect_config) = parse_database_url(getenv(HEALTH_DATABASE_URL_ENV))?;
    let backlog_health = parse_backlog_health(getenv(HEALTH_BACKLOG_HEALTH_ENV))?;
    Ok(HealthProcessConfig {
        listen_addr,
        database_url,
        connect_config,
        backlog_health,
    })
}

/// Bind the configured listen address for operator probes.
///
/// # Errors
///
/// Returns the I/O error if the operating system cannot bind the address.
pub fn bind_health_process(config: &HealthProcessConfig) -> io::Result<TcpListener> {
    bind_health_http(config.listen_addr())
}

/// Serve GET `/live` and GET `/ready` until `accept` fails.
///
/// GET `/live` uses a process-liveness snapshot and never opens a store
/// connection. GET `/ready` connects only when `DATABASE_URL` is configured.
/// Connect or probe failure becomes HTTP 503 without driver text.
///
/// # Errors
///
/// Returns the I/O error that stopped the accept loop.
pub fn serve_health_process(
    listener: &TcpListener,
    config: &HealthProcessConfig,
) -> io::Result<()> {
    serve_health_http_with(listener, |request| {
        answer_health_process_request(request, config)
    })
}

/// Run the process from environment values: parse, bind, then serve.
///
/// # Errors
///
/// Returns a configuration error or the listen/serve I/O error.
pub fn run_health_process<F>(getenv: F) -> Result<(), HealthProcessRunError>
where
    F: Fn(&str) -> Option<String>,
{
    let config =
        parse_health_process_config(getenv).map_err(HealthProcessRunError::InvalidConfig)?;
    let listener = bind_health_process(&config).map_err(HealthProcessRunError::Listen)?;
    serve_health_process(&listener, &config).map_err(HealthProcessRunError::Listen)
}

/// Runtime failure after configuration has been parsed.
#[derive(Debug)]
pub enum HealthProcessRunError {
    /// Environment values were missing or unknown.
    InvalidConfig(HealthProcessConfigError),
    /// Binding or serving the probe listener failed.
    Listen(io::Error),
}

impl Display for HealthProcessRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(error) => Display::fmt(error, formatter),
            Self::Listen(error) => write!(
                formatter,
                "bind HEALTH_LISTEN_ADDR or PORT and keep the process running: {error}"
            ),
        }
    }
}

impl Error for HealthProcessRunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfig(error) => Some(error),
            Self::Listen(error) => Some(error),
        }
    }
}

fn parse_listen_addr(
    listen_addr: Option<String>,
    port: Option<String>,
) -> Result<SocketAddr, HealthProcessConfigError> {
    match exact_env_value(listen_addr) {
        Err(()) => return Err(HealthProcessConfigError::InvalidListenAddress),
        Ok(Some(value)) => {
            return value
                .parse()
                .map_err(|_| HealthProcessConfigError::InvalidListenAddress);
        }
        Ok(None) => {}
    }
    match exact_env_value(port) {
        Err(()) => Err(HealthProcessConfigError::InvalidListenPort),
        Ok(None) => Err(HealthProcessConfigError::MissingListenAddress),
        Ok(Some(value)) => {
            let port = value
                .parse::<u16>()
                .map_err(|_| HealthProcessConfigError::InvalidListenPort)?;
            Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port))
        }
    }
}

fn parse_database_url(
    raw: Option<String>,
) -> Result<(Option<String>, Option<postgres::Config>), HealthProcessConfigError> {
    match exact_env_value(raw) {
        Err(()) => Err(HealthProcessConfigError::InvalidDatabaseUrl),
        Ok(None) => Ok((None, None)),
        Ok(Some(value)) => {
            let config = postgres::Config::from_str(&value)
                .map_err(|_| HealthProcessConfigError::InvalidDatabaseUrl)?;
            Ok((Some(value), Some(config)))
        }
    }
}

fn parse_backlog_health(raw: Option<String>) -> Result<BacklogHealth, HealthProcessConfigError> {
    match exact_env_value(raw) {
        Err(()) => Err(HealthProcessConfigError::InvalidBacklogHealth),
        Ok(None) => Ok(BacklogHealth::Unknown),
        Ok(Some(value)) => match value.as_str() {
            "within_bounds" => Ok(BacklogHealth::WithinBounds),
            "stalled" => Ok(BacklogHealth::Stalled),
            "unknown" => Ok(BacklogHealth::Unknown),
            _ => Err(HealthProcessConfigError::InvalidBacklogHealth),
        },
    }
}

fn exact_env_value(raw: Option<String>) -> Result<Option<String>, ()> {
    match raw {
        None => Ok(None),
        Some(value) if value.is_empty() || value.trim() != value.as_str() => Err(()),
        Some(value) => Ok(Some(value)),
    }
}

fn answer_health_process_request(
    request: &str,
    config: &HealthProcessConfig,
) -> HealthHttpResponse {
    if !health_request_requires_readiness_snapshot(request) {
        return handle_health_http_request(request, &process_liveness_snapshot());
    }
    match config.connect_config.as_ref() {
        None => handle_health_http_request(request, &process_liveness_snapshot()),
        Some(connect_config) => match connect_operational_store(connect_config) {
            Some(mut client) => handle_postgres_health_http_request(
                request,
                &mut client,
                &[],
                config.backlog_health(),
            ),
            None => health_ready_response(
                &unavailable_store_snapshot(config.backlog_health()),
                &ready_required_capabilities(request),
            ),
        },
    }
}

fn connect_operational_store(connect_config: &postgres::Config) -> Option<Client> {
    let mut connect_config = connect_config.clone();
    connect_config.connect_timeout(HEALTH_HTTP_IO_TIMEOUT);
    connect_config.connect(NoTls).ok()
}

fn ready_required_capabilities(request: &str) -> Vec<&str> {
    let named = health_request_required_capabilities(request);
    if named.is_empty() {
        vec![POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]
    } else {
        named
    }
}

fn process_liveness_snapshot() -> RuntimeHealthSnapshot {
    RuntimeHealthSnapshot::new(
        true,
        BacklogHealth::Unknown,
        DataIntegrityHealth::Unknown,
        Vec::new(),
    )
    .expect("empty process-liveness snapshot is valid")
}

fn unavailable_store_snapshot(backlog_health: BacklogHealth) -> RuntimeHealthSnapshot {
    let capability = CapabilityHealth::new(
        POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF,
        CapabilityState::Unknown,
        false,
    )
    .expect("repository-owned postgres capability reference must remain valid");
    RuntimeHealthSnapshot::new(
        true,
        backlog_health,
        DataIntegrityHealth::Unknown,
        vec![capability],
    )
    .expect("unavailable store snapshot contains one unique capability")
}

#[cfg(test)]
mod tests {
    use super::{
        exact_env_value, parse_health_process_config, ready_required_capabilities,
        run_health_process, HealthProcessConfigError, HealthProcessRunError,
        HEALTH_LISTEN_ADDR_ENV,
    };
    use crate::postgres_health::POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF;
    use std::error::Error;
    use std::io;

    #[test]
    fn config_debug_redacts_the_database_url() {
        let config = parse_health_process_config(|key| match key {
            HEALTH_LISTEN_ADDR_ENV => Some("127.0.0.1:0".to_owned()),
            "DATABASE_URL" => Some("postgres://operator:secret@db/product".to_owned()),
            _ => None,
        })
        .unwrap();
        let rendered = format!("{config:?}");
        assert!(rendered.contains("database_url_present: true"));
        assert!(!rendered.contains("secret"));
        assert!(!rendered.contains("operator"));
    }

    #[test]
    fn exact_env_value_treats_absence_as_optional() {
        assert_eq!(exact_env_value(None), Ok(None));
        assert_eq!(
            exact_env_value(Some("ready".to_owned())),
            Ok(Some("ready".to_owned()))
        );
        assert_eq!(exact_env_value(Some(String::new())), Err(()));
        assert_eq!(exact_env_value(Some(" padded".to_owned())), Err(()));
    }

    #[test]
    fn ready_required_capabilities_default_to_the_operational_store() {
        assert_eq!(
            ready_required_capabilities("GET /ready HTTP/1.1\r\n\r\n"),
            vec![POSTGRES_OPERATIONAL_STORE_CAPABILITY_REF]
        );
        assert_eq!(
            ready_required_capabilities("GET /ready?capability=scoring HTTP/1.1\r\n\r\n"),
            vec!["scoring"]
        );
    }

    #[test]
    fn run_and_exit_code_fail_closed_for_missing_listen_config() {
        let error = run_health_process(|_| None).expect_err("missing listen config must not run");
        assert!(matches!(
            error,
            HealthProcessRunError::InvalidConfig(HealthProcessConfigError::MissingListenAddress)
        ));
        assert!(error.to_string().contains("HEALTH_LISTEN_ADDR"));
        assert!(error.source().is_some());
    }

    #[test]
    fn run_error_display_covers_listen_failures() {
        let error = HealthProcessRunError::Listen(io::Error::new(
            io::ErrorKind::AddrInUse,
            "address already in use",
        ));
        assert!(error.to_string().contains("HEALTH_LISTEN_ADDR"));
        assert!(error.to_string().contains("address already in use"));
        assert!(error.source().is_some());
    }
}
