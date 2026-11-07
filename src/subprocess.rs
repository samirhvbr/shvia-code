//! Processes started by Anna get only the environment they need for ordinary development work.

use std::env;
use std::process::Command;

const INHERITED: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    "LANG",
    "LANGUAGE",
    "LC_ALL",
    "LC_ADDRESS",
    "LC_COLLATE",
    "LC_CTYPE",
    "LC_IDENTIFICATION",
    "LC_MEASUREMENT",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NAME",
    "LC_NUMERIC",
    "LC_PAPER",
    "LC_TELEPHONE",
    "LC_TIME",
    "TZ",
    "TMPDIR",
    "TMP",
    "TEMP",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "USERPROFILE",
    "USERNAME",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "SystemRoot",
    "windir",
    "COMSPEC",
    "PATHEXT",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "GOPATH",
    "GOROOT",
    "JAVA_HOME",
    "SDKROOT",
    "DEVELOPER_DIR",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
    "GIT_SSL_CAINFO",
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "NO_PROXY",
    "no_proxy",
];

const PROXIES: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

/// A credential-free origin keeps the Workspace's egress proxy in use. Drop embedded
/// authentication and URL suffixes before the value can appear in shell or hook output.
fn proxy_origin(value: &str) -> Option<String> {
    let url = if value.contains("://") {
        reqwest::Url::parse(value).ok()?
    } else {
        reqwest::Url::parse(&format!("http://{value}")).ok()?
    };
    if !matches!(
        url.scheme(),
        "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
    ) {
        return None;
    }
    let host = url.host_str()?;
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    Some(format!("{}://{host}{port}", url.scheme()))
}

pub fn command(program: &str) -> Command {
    let mut child = Command::new(program);
    child.env_clear();
    // Start empty so new provider secrets and custom credentials are excluded by default.
    child.envs(
        INHERITED
            .iter()
            .filter_map(|&name| env::var_os(name).map(|value| (name, value))),
    );
    child.envs(PROXIES.iter().filter_map(|&name| {
        env::var(name)
            .ok()
            .and_then(|value| proxy_origin(&value))
            .map(|origin| (name, origin))
    }));
    child
}
