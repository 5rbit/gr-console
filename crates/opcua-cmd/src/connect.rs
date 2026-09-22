//! Endpoint discovery, security/auth selection and session creation.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use opcua::client::{Client, ClientBuilder, IdentityToken, Password, Session, SessionActivity, SessionEventLoop, SessionPollResult};
use opcua::crypto::SecurityPolicy;
use opcua::types::{EndpointDescription, MessageSecurityMode, StatusCode, UserTokenType};

use crate::{Auth, OpcError, OpcUaConfig};

/// Why the session driver task ended.
#[derive(Debug, Clone, PartialEq)]
pub enum LoopEnd {
    /// The transport closed / the (single) connect attempt failed / the session was closed on purpose.
    Closed(StatusCode),
    /// The channel is still up but the session is gone (server answered a keep-alive with a
    /// session-invalid code, or too many keep-alives timed out). The driver stopped polling, which
    /// drops the transport, so the library stops logging keep-alive failures for this session.
    SessionDead { code: StatusCode, detail: String },
}

/// An established (activated) session plus its driver task.
pub struct Connection {
    pub session: Arc<Session>,
    /// Polls the library event loop (`SessionEventLoop::enter`) and ends on connection loss
    /// (no in-library reconnect) or when the keep-alive says the session is dead.
    pub event_loop: tokio::task::JoinHandle<LoopEnd>,
    /// Endpoint descriptions returned by GetEndpoints (for diagnostics).
    pub endpoints: Vec<String>,
    /// URL of the endpoint the session was created on.
    pub endpoint_url: String,
}

/// Run `fut` with a timeout, mapping expiry to [`OpcError::Timeout`].
pub async fn with_timeout<T, F: Future<Output = T>>(d: Duration, fut: F) -> Result<T, OpcError> {
    tokio::time::timeout(d, fut).await.map_err(|_| OpcError::Timeout)
}

/// Map an async-opcua error to ours (BadTimeout → Timeout, else Transport).
pub fn map_err(e: opcua::types::Error) -> OpcError {
    if e.status() == StatusCode::BadTimeout { OpcError::Timeout } else { OpcError::Transport(e.to_string()) }
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// Parse the configured policy name (case-insensitive, `-`/`_` ignored).
pub fn parse_policy(s: &str) -> Result<SecurityPolicy, OpcError> {
    Ok(match squash(s).as_str() {
        "" | "none" => SecurityPolicy::None,
        "basic256sha256" => SecurityPolicy::Basic256Sha256,
        "aes128sha256rsaoaep" => SecurityPolicy::Aes128Sha256RsaOaep,
        "aes256sha256rsapss" => SecurityPolicy::Aes256Sha256RsaPss,
        "basic256" => SecurityPolicy::Basic256,
        "basic128rsa15" => SecurityPolicy::Basic128Rsa15,
        _ => {
            return Err(OpcError::Config(format!("unknown security_policy {s:?}; use None | Basic256Sha256 | Aes128_Sha256_RsaOaep | Aes256_Sha256_RsaPss")));
        }
    })
}

/// Parse the configured message security mode.
pub fn parse_mode(s: &str) -> Result<MessageSecurityMode, OpcError> {
    Ok(match squash(s).as_str() {
        "" | "none" => MessageSecurityMode::None,
        "sign" => MessageSecurityMode::Sign,
        "signandencrypt" | "signencrypt" => MessageSecurityMode::SignAndEncrypt,
        _ => {
            return Err(OpcError::Config(format!("unknown security_mode {s:?}; use None | Sign | SignAndEncrypt")));
        }
    })
}

fn token_type_name(t: UserTokenType) -> &'static str {
    match t {
        UserTokenType::Anonymous => "Anonymous",
        UserTokenType::UserName => "UserName",
        UserTokenType::Certificate => "Certificate",
        UserTokenType::IssuedToken => "IssuedToken",
    }
}

/// One-line description of an endpoint: url, policy, mode, token types, security level.
pub fn describe_endpoint(e: &EndpointDescription) -> String {
    let policy = SecurityPolicy::from_uri(e.security_policy_uri.as_ref());
    let tokens = e.user_identity_tokens.as_ref().map(|v| v.iter().map(|t| format!("{}({})", token_type_name(t.token_type), t.policy_id.as_ref())).collect::<Vec<_>>().join(",")).unwrap_or_default();
    format!("{} policy={} mode={} tokens=[{}] level={}", e.endpoint_url.as_ref(), policy.to_str(), e.security_mode, tokens, e.security_level)
}

fn build_client(cfg: &OpcUaConfig, secure: bool) -> Result<Client, OpcError> {
    let pki_dir = cfg.pki_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("gr-console-opcua-pki"));
    ClientBuilder::new()
        .application_name("gr-console")
        .application_uri("urn:gr-console:opcua-cmd")
        .product_uri("urn:gr-console")
        .session_name("gr-console cmd writer")
        .pki_dir(pki_dir)
        .create_sample_keypair(secure)
        .trust_server_certs(cfg.trust_server_cert)
        .ignore_clock_skew(true)
        // Reconnects are handled by the outer loop in `writer.rs` so that every
        // connect re-verifies / re-browses the node map and publishes state.
        .session_retry_limit(0)
        .session_timeout(cfg.session_timeout())
        .channel_lifetime(cfg.channel_lifetime())
        // The library keep-alive (Read of Server_ServerStatus_State) is the session health probe;
        // `drive` reacts to its failures. The library itself never acts on them
        // (`max_failed_keep_alive_count` defaults to 0 = only log).
        .keep_alive_interval(cfg.keepalive_interval())
        .request_timeout(cfg.write_timeout())
        .client()
        .map_err(|errs| OpcError::Config(format!("client config: {}", errs.join("; "))))
}

fn identity(cfg: &OpcUaConfig) -> IdentityToken {
    match &cfg.auth {
        Auth::Anonymous => IdentityToken::Anonymous,
        Auth::UserPass { user, pass } => IdentityToken::UserName(user.clone(), Password::new(pass.clone())),
    }
}

fn endpoint_supports(e: &EndpointDescription, wanted: UserTokenType) -> bool {
    match &e.user_identity_tokens {
        None => wanted == UserTokenType::Anonymous,
        Some(list) => list.iter().any(|t| t.token_type == wanted),
    }
}

fn auth_hint(code: StatusCode, endpoints: &[String]) -> Option<String> {
    let msg = match code {
        StatusCode::BadUserAccessDenied | StatusCode::BadIdentityTokenInvalid | StatusCode::BadIdentityTokenRejected => {
            "server rejected the user identity token (check user/password, that the user exists \
             in the PLC's OPC UA user management, and that the endpoint offers the token type)"
        }
        StatusCode::BadCertificateUntrusted
        | StatusCode::BadCertificateInvalid
        | StatusCode::BadSecurityChecksFailed
        | StatusCode::BadCertificateHostNameInvalid
        | StatusCode::BadCertificateUriInvalid => {
            "certificate rejected (the PLC must trust the client certificate from pki_dir/own, \
             and trust_server_cert must be true unless the server cert is in pki_dir/trusted)"
        }
        _ => return None,
    };
    Some(format!("{code}: {msg}; endpoints offered: [{}]", endpoints.join(" | ")))
}

/// GetEndpoints → pick the configured policy/mode/token → CreateSession + ActivateSession.
pub async fn connect(cfg: &OpcUaConfig) -> Result<Connection, OpcError> {
    let policy = parse_policy(&cfg.security_policy)?;
    let mode = parse_mode(&cfg.security_mode)?;
    match (policy, mode) {
        (SecurityPolicy::None, MessageSecurityMode::None) => {}
        (SecurityPolicy::None, m) => {
            return Err(OpcError::Config(format!("security_policy None requires security_mode None (got {m})")));
        }
        (p, MessageSecurityMode::None) => {
            return Err(OpcError::Config(format!("security_policy {} requires security_mode Sign or SignAndEncrypt", p.to_str())));
        }
        _ => {}
    }
    let secure = policy != SecurityPolicy::None;
    let mut client = build_client(cfg, secure)?;
    let timeout = cfg.connect_timeout();

    let endpoints = with_timeout(timeout, client.get_server_endpoints_from_url(cfg.endpoint.as_str())).await?.map_err(|e| OpcError::Transport(format!("GetEndpoints {}: {e}", cfg.endpoint)))?;

    let descriptions: Vec<String> = endpoints.iter().map(describe_endpoint).collect();
    for d in &descriptions {
        tracing::info!(endpoint = %d, "server endpoint");
    }

    let wanted_token = match cfg.auth {
        Auth::Anonymous => UserTokenType::Anonymous,
        Auth::UserPass { .. } => UserTokenType::UserName,
    };

    let matched = Client::find_matching_endpoint(&endpoints, &cfg.endpoint, policy, mode)
        .ok_or_else(|| OpcError::Config(format!("endpoint with policy {} / mode {} not offered by {}; offered: [{}]", policy.to_str(), mode, cfg.endpoint, descriptions.join(" | "))))?;
    if !endpoint_supports(&matched, wanted_token) {
        return Err(OpcError::Config(format!("endpoint {} does not offer a {} user token; offered: [{}]", describe_endpoint(&matched), token_type_name(wanted_token), descriptions.join(" | "))));
    }
    let endpoint_url = matched.endpoint_url.as_ref().to_string();

    let (session, event_loop) = client.connect_to_endpoint_directly(matched, identity(cfg)).map_err(|e| OpcError::Config(format!("session setup: {e}")))?;
    let mut handle = tokio::spawn(drive(event_loop, cfg.keepalive_fail_limit()));

    let connected = tokio::select! {
        ok = session.wait_for_connection() => Ok(ok),
        r = &mut handle => Err(r),
        _ = tokio::time::sleep(timeout) => {
            // 서버에 반쯤 만든 세션이 남지 않게 짧게 닫아 보고 버린다(GRM 세션 수 한도).
            let _ = with_timeout(Duration::from_secs(1), session.disconnect()).await;
            handle.abort();
            return Err(OpcError::Timeout);
        }
    };
    match connected {
        Ok(true) => {}
        Ok(false) => {
            handle.abort();
            return Err(OpcError::Transport("session closed before activation".into()));
        }
        Err(join) => {
            let code = match join {
                Ok(LoopEnd::Closed(code) | LoopEnd::SessionDead { code, .. }) => code,
                Err(e) => return Err(OpcError::Transport(format!("event loop panicked: {e}"))),
            };
            return Err(match auth_hint(code, &descriptions) {
                Some(hint) => OpcError::Config(hint),
                None => OpcError::Transport(format!("connect failed: {code}")),
            });
        }
    }

    Ok(Connection { session, event_loop: handle, endpoints: descriptions, endpoint_url })
}

/// Status codes meaning "this session (or its channel) is gone on the server": reconnecting is the
/// only way forward. Returned e.g. after a server restart / PLC download that keeps the TCP channel
/// up, or when the server expired the session.
pub fn is_session_dead(code: StatusCode) -> bool {
    matches!(
        code,
        StatusCode::BadSessionIdInvalid
            | StatusCode::BadSessionClosed
            | StatusCode::BadSessionNotActivated
            | StatusCode::BadSecureChannelIdInvalid
            | StatusCode::BadSecureChannelClosed
            | StatusCode::BadConnectionClosed
            | StatusCode::BadNotConnected
    )
}

fn is_timeout(code: StatusCode) -> bool {
    matches!(code, StatusCode::BadTimeout | StatusCode::BadRequestTimeout)
}

/// Keep-alive bookkeeping: `Some(detail)` once the session is to be considered dead.
#[derive(Debug, Default)]
pub(crate) struct KeepAliveWatch {
    timeouts: u32,
}

impl KeepAliveWatch {
    pub(crate) fn observe(&mut self, activity: &SessionActivity, fail_limit: u32) -> Option<(StatusCode, String)> {
        match activity {
            SessionActivity::KeepAliveSucceeded => {
                self.timeouts = 0;
                None
            }
            SessionActivity::KeepAliveFailed(code) if is_session_dead(*code) => Some((*code, format!("keep-alive: {code}"))),
            SessionActivity::KeepAliveFailed(code) if is_timeout(*code) => {
                self.timeouts += 1;
                (self.timeouts >= fail_limit.max(1)).then(|| (*code, format!("keep-alive timed out {} times in a row", self.timeouts)))
            }
            // Other failures (e.g. BadServerHalted: server state not Running) are left to the
            // library's own warning; the session itself is still valid.
            SessionActivity::KeepAliveFailed(_) => None,
        }
    }
}

/// Drive the library event loop ourselves instead of `SessionEventLoop::spawn()`: `spawn()` only
/// ends when the transport closes, so a session the server invalidated behind a live channel was
/// kept forever (every keep-alive logged `BadSessionIdInvalid`, state stayed Ready).
async fn drive<T: opcua::client::transport::Connector + Send + Sync + 'static>(event_loop: SessionEventLoop<T>, fail_limit: u32) -> LoopEnd {
    let stream = event_loop.enter();
    tokio::pin!(stream);
    let mut watch = KeepAliveWatch::default();
    let mut connected = false;
    loop {
        match stream.next().await {
            None => return LoopEnd::Closed(StatusCode::Good),
            Some(Err(code)) => return LoopEnd::Closed(code),
            Some(Ok(SessionPollResult::Reconnected(_))) => connected = true,
            // The library would try to re-activate the session on a fresh channel by itself; the
            // writer reconnects instead so the node map is re-verified and the state is published.
            Some(Ok(SessionPollResult::ConnectionLost(code))) if connected => return LoopEnd::Closed(code),
            Some(Ok(SessionPollResult::SessionActivity(a))) => {
                if let Some((code, detail)) = watch.observe(&a, fail_limit) {
                    return LoopEnd::SessionDead { code, detail };
                }
            }
            Some(Ok(_)) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keepalive_watch_classifies() {
        let mut w = KeepAliveWatch::default();
        assert_eq!(w.observe(&SessionActivity::KeepAliveSucceeded, 2), None);
        let dead = w.observe(&SessionActivity::KeepAliveFailed(StatusCode::BadSessionIdInvalid), 2).expect("dead");
        assert_eq!(dead.0, StatusCode::BadSessionIdInvalid);
        for c in [StatusCode::BadSessionClosed, StatusCode::BadSessionNotActivated, StatusCode::BadSecureChannelIdInvalid, StatusCode::BadConnectionClosed, StatusCode::BadNotConnected] {
            assert!(w.observe(&SessionActivity::KeepAliveFailed(c), 2).is_some(), "{c}");
        }
        // Timeouts: K in a row; a success in between resets the count.
        let mut w = KeepAliveWatch::default();
        assert_eq!(w.observe(&SessionActivity::KeepAliveFailed(StatusCode::BadTimeout), 2), None);
        assert_eq!(w.observe(&SessionActivity::KeepAliveSucceeded, 2), None);
        assert_eq!(w.observe(&SessionActivity::KeepAliveFailed(StatusCode::BadTimeout), 2), None);
        assert!(w.observe(&SessionActivity::KeepAliveFailed(StatusCode::BadTimeout), 2).is_some());
        // Server not running is not a dead session.
        let mut w = KeepAliveWatch::default();
        for _ in 0..5 {
            assert_eq!(w.observe(&SessionActivity::KeepAliveFailed(StatusCode::BadServerHalted), 2), None);
        }
        assert!(!is_session_dead(StatusCode::BadNotWritable));
    }

    #[test]
    fn policy_names() {
        assert_eq!(parse_policy("None").unwrap(), SecurityPolicy::None);
        assert_eq!(parse_policy("basic256sha256").unwrap(), SecurityPolicy::Basic256Sha256);
        assert_eq!(parse_policy("Aes128_Sha256_RsaOaep").unwrap(), SecurityPolicy::Aes128Sha256RsaOaep);
        assert_eq!(parse_policy("Aes256-Sha256-RsaPss").unwrap(), SecurityPolicy::Aes256Sha256RsaPss);
        assert!(parse_policy("Basic999").is_err());
        assert_eq!(parse_mode("SignAndEncrypt").unwrap(), MessageSecurityMode::SignAndEncrypt);
        assert_eq!(parse_mode("sign_and_encrypt").unwrap(), MessageSecurityMode::SignAndEncrypt);
        assert_eq!(parse_mode("Sign").unwrap(), MessageSecurityMode::Sign);
        assert!(parse_mode("encrypt").is_err());
    }
}
