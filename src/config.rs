use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuth {
    pub user: String,
    pub pass: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    listen: Option<String>,
    templates_dir: Option<PathBuf>,
    font_paths: Option<Vec<PathBuf>>,
    max_source: Option<String>,
    compile_timeout: Option<String>,
    debounce_ms: Option<u64>,
    max_compiles_in_flight: Option<usize>,
    allow_universe: Option<bool>,
    base_title: Option<String>,
    auth: Option<BasicAuth>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub templates_dir: PathBuf,
    pub font_paths: Vec<PathBuf>,
    pub max_source: usize,
    pub compile_timeout: Duration,
    pub debounce_ms: u64,
    pub max_compiles_in_flight: usize,
    pub allow_universe: bool,
    pub base_title: String,
    pub auth: Option<BasicAuth>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read config file {}", path.display()))?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<Config> {
        let raw: RawConfig = toml::from_str(text).context("invalid config")?;
        let listen = raw
            .listen
            .as_deref()
            .unwrap_or("127.0.0.1:8080")
            .parse::<SocketAddr>()
            .context("invalid `listen` address")?;
        let config = Config {
            listen,
            templates_dir: raw.templates_dir.unwrap_or_else(|| "./templates".into()),
            font_paths: raw.font_paths.unwrap_or_else(|| vec!["./.fonts".into()]),
            max_source: raw
                .max_source
                .as_deref()
                .map(parse_size)
                .transpose()
                .context("invalid `max_source`")?
                .unwrap_or(256 * 1024),
            compile_timeout: raw
                .compile_timeout
                .as_deref()
                .map(parse_duration)
                .transpose()
                .context("invalid `compile_timeout`")?
                .unwrap_or(Duration::from_secs(10)),
            debounce_ms: raw.debounce_ms.unwrap_or(500),
            max_compiles_in_flight: raw.max_compiles_in_flight.unwrap_or(4),
            allow_universe: raw.allow_universe.unwrap_or(false),
            base_title: raw.base_title.unwrap_or_else(|| "Letters".to_string()),
            auth: raw.auth,
        };
        Ok(config)
    }

    /// Fail fast if the templates directory is missing or unreadable.
    pub fn check_templates_dir(&self) -> Result<()> {
        let dir = &self.templates_dir;
        match std::fs::read_dir(dir) {
            Ok(_) => Ok(()),
            Err(e) => bail!(
                "templates_dir {} is missing or unreadable: {e}",
                dir.display()
            ),
        }
    }
}

/// Parse human-readable sizes: "256KiB", "1MiB", "1024", "512B".
pub fn parse_size(s: &str) -> Result<usize> {
    let s = s.trim();
    let (num, factor) = if let Some(n) = s.strip_suffix("KiB") {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix("MiB") {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1)
    } else {
        (s, 1)
    };
    let num: usize = num
        .trim()
        .parse()
        .with_context(|| format!("bad size: {s:?}"))?;
    Ok(num * factor)
}

/// Parse human-readable durations: "10s", "500ms", "2m".
pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (num, unit): (&str, fn(u64) -> Duration) = if let Some(n) = s.strip_suffix("ms") {
        (n, Duration::from_millis)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, Duration::from_secs)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, |v| Duration::from_secs(v * 60))
    } else {
        bail!("bad duration {s:?}: expected suffix ms/s/m");
    };
    let num: u64 = num
        .trim()
        .parse()
        .with_context(|| format!("bad duration: {s:?}"))?;
    Ok(unit(num))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("256KiB").unwrap(), 262144);
        assert_eq!(parse_size("1MiB").unwrap(), 1048576);
        assert_eq!(parse_size("512B").unwrap(), 512);
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert!(parse_size("lots").is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("10s").unwrap(), Duration::from_secs(10));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert!(parse_duration("10").is_err());
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn defaults_when_empty() {
        let c = Config::from_toml("").unwrap();
        assert_eq!(c.listen.to_string(), "127.0.0.1:8080");
        assert_eq!(c.templates_dir, PathBuf::from("./templates"));
        assert_eq!(c.max_source, 262144);
        assert_eq!(c.compile_timeout, Duration::from_secs(10));
        assert_eq!(c.debounce_ms, 500);
        assert_eq!(c.max_compiles_in_flight, 4);
        assert!(!c.allow_universe);
        assert_eq!(c.base_title, "Letters");
        assert!(c.auth.is_none());
    }

    #[test]
    fn parses_full_config() {
        let c = Config::from_toml(
            r#"
listen = "0.0.0.0:9999"
templates_dir = "/data/templates"
font_paths = ["/data/fonts"]
max_source = "1MiB"
compile_timeout = "5s"
debounce_ms = 250
max_compiles_in_flight = 2
allow_universe = true
base_title = "Post"
auth = { user = "alice", pass = "secret" }
"#,
        )
        .unwrap();
        assert_eq!(c.listen.to_string(), "0.0.0.0:9999");
        assert_eq!(c.max_source, 1048576);
        assert_eq!(c.compile_timeout, Duration::from_secs(5));
        assert!(c.allow_universe);
        let auth = c.auth.unwrap();
        assert_eq!(auth.user, "alice");
        assert_eq!(auth.pass, "secret");
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(Config::from_toml("listne = \"x\"").is_err());
    }
}
