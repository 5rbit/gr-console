//! Endpoint discovery, security/auth selection and session creation.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use opcua::client::{Client, ClientBuilder, IdentityToken, Password, Session};
use opcua::crypto::SecurityPolicy;
use opcua::types::{EndpointDescription, MessageSecurityMode, StatusCode, UserTokenType};

use crate::{Auth, OpcError, OpcUaConfig};

/// An established (activated) session plus its event loop task.
pub struct Connection {
    pub session: Arc<Session>,
    /// Ends when the session is closed or the library gives up reconnecting
    /// (`session_retry_limit(0)`: any connection loss ends the loop).
    pub event_loop: tokio::task::JoinHandle<StatusCode>,
    /// Endpoint descriptions returned by GetEndpoints (for diagnostics).
    pub endpoints: Vec<String>,
    /// URL of the endpoint the session was created on.
    pub endpoint_url: String,
}

/// Run `fut` with a timeout, mapping expiry to [`OpcError::Timeout`].
pub async fn with_timeout<T, F: Future<Output = T>>(d: Duration, fut: F) -> Result<T, OpcError> {
    tokio::time::timeout(d, fut)
        .await
        .map_err(|_| OpcError::Timeout)
}

/// Map an async-opcua error to ours (BadTimeout → Timeout, else Transport).
pub fn map_err(e: opcua::types::Error) -> OpcError {
    if e.status() == StatusCode::BadTimeout {
        OpcError::Timeout
    } else {
        OpcError::Transport(e.to_string())
    }
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
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
            return Err(OpcError::Config(format!(
                "unknown security_policy {s:?}; use None | Basic256Sha256 | Aes128_Sha256_RsaOaep | Aes256_Sha256_RsaPss"
            )));
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
            return Err(OpcError::Config(format!(
                "unknown security_mode {s:?}; use None | Sign | SignAndEncrypt"
            )));
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
    let tokens = e
        .user_identity_tokens
        .as_ref()
        .map(|v| {
            v.iter()
                .map(|t| format!("{}({})", token_type_name(t.token_type), t.policy_id.as_ref()))
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    format!(
        "{} policy={} mode={} tokens=[{}] level={}",
        e.endpoint_url.as_ref(),
        policy.to_str(),
        e.security_mode,
        tokens,
        e.security_level
    )
}

fn build_client(cfg: &OpcUaConfig, secure: bool) -> Result<Client, OpcError> {
    let pki_dir = cfg
        .pki_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("gr-console-opcua-pki"));
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
        .request_timeout(cfg.write_timeout())
        .client()
        .map_err(|errs| OpcError::Config(format!("client config: {}", errs.join("; "))))
}

fn identity(cfg: &OpcUaConfig) -> IdentityToken {
    match &cfg.auth {
        Auth::Anonymous => IdentityToken::Anonymous,
        Auth::UserPass { user, pass } => {
            IdentityToken::UserName(user.clone(), Password::new(pass.clone()))
        }
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
        StatusCode::BadUserAccessDenied
        | StatusCode::BadIdentityTokenInvalid
        | StatusCode::BadIdentityTokenRejected => {
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
    Some(format!(
        "{code}: {msg}; endpoints offered: [{}]",
        endpoints.join(" | ")
    ))
}

/// GetEndpoints → pick the configured policy/mode/token → CreateSession + ActivateSession.
pub async fn connect(cfg: &OpcUaConfig) -> Result<Connection, OpcError> {
    let policy = parse_policy(&cfg.security_policy)?;
    let mode = parse_mode(&cfg.security_mode)?;
    match (policy, mode) {
        (SecurityPolicy::None, MessageSecurityMode::None) => {}
        (SecurityPolicy::None, m) => {
            return Err(OpcError::Config(format!(
                "security_policy None requires security_mode None (got {m})"
            )));
        }
        (p, MessageSecurityMode::None) => {
            return Err(OpcError::Config(format!(
                "security_policy {} requires security_mode Sign or SignAndEncrypt",
                p.to_str()
            )));
        }
        _ => {}
    }
    let secure = policy != SecurityPolicy::None;
    let mut client = build_client(cfg, secure)?;
    let timeout = cfg.connect_timeout();

    let endpoints = with_timeout(
        timeout,
        client.get_server_endpoints_from_url(cfg.endpoint.as_str()),
    )
    .await?
    .map_err(|e| OpcError::Transport(format!("GetEndpoints {}: {e}", cfg.endpoint)))?;

    let descriptions: Vec<String> = endpoints.iter().map(describe_endpoint).collect();
    for d in &descriptions {
        tracing::info!(endpoint = %d, "server endpoint");
    }

    let wanted_token = match cfg.auth {
        Auth::Anonymous => UserTokenType::Anonymous,
        Auth::UserPass { .. } => UserTokenType::UserName,
    };

    let matched = Client::find_matching_endpoint(&endpoints, &cfg.endpoint, policy, mode)
        .ok_or_else(|| {
            OpcError::Config(format!(
                "endpoint with policy {} / mode {} not offered by {}; offered: [{}]",
                policy.to_str(),
                mode,
                cfg.endpoint,
                descriptions.join(" | ")
            ))
        })?;
    if !endpoint_supports(&matched, wanted_token) {
        return Err(OpcError::Config(format!(
            "endpoint {} does not offer a {} user token; offered: [{}]",
            describe_endpoint(&matched),
            token_type_name(wanted_token),
            descriptions.join(" | ")
        )));
    }
    let endpoint_url = matched.endpoint_url.as_ref().to_string();

    let (session, event_loop) = client
        .connect_to_endpoint_directly(matched, identity(cfg))
        .map_err(|e| OpcError::Config(format!("session setup: {e}")))?;
    let mut handle = event_loop.spawn();

    let connected = tokio::select! {
        ok = session.wait_for_connection() => Ok(ok),
        r = &mut handle => Err(r),
        _ = tokio::time::sleep(timeout) => {
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
                Ok(code) => code,
                Err(e) => return Err(OpcError::Transport(format!("event loop panicked: {e}"))),
            };
            return Err(match auth_hint(code, &descriptions) {
                Some(hint) => OpcError::Config(hint),
                None => OpcError::Transport(format!("connect failed: {code}")),
            });
        }
    }

    Ok(Connection {
        session,
        event_loop: handle,
        endpoints: descriptions,
        endpoint_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_names() {
        assert_eq!(parse_policy("None").unwrap(), SecurityPolicy::None);
        assert_eq!(parse_policy("basic256sha256").unwrap(), SecurityPolicy::Basic256Sha256);
        assert_eq!(
            parse_policy("Aes128_Sha256_RsaOaep").unwrap(),
            SecurityPolicy::Aes128Sha256RsaOaep
        );
        assert_eq!(
            parse_policy("Aes256-Sha256-RsaPss").unwrap(),
            SecurityPolicy::Aes256Sha256RsaPss
        );
        assert!(parse_policy("Basic999").is_err());
        assert_eq!(parse_mode("SignAndEncrypt").unwrap(), MessageSecurityMode::SignAndEncrypt);
        assert_eq!(parse_mode("sign_and_encrypt").unwrap(), MessageSecurityMode::SignAndEncrypt);
        assert_eq!(parse_mode("Sign").unwrap(), MessageSecurityMode::Sign);
        assert!(parse_mode("encrypt").is_err());
    }
}
