//! Signing in with GitHub's device flow: the user types a code on github.com, and no password
//! ever reaches Sawit.

use std::time::Duration;

use serde_json::Value;

use super::http::{Reply, curl};
use super::{client_id, scope_list};
use crate::{CoreError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceCode {
    pub device_code: String,
    /// What the user types on the verification page.
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

pub fn parse_device_code(v: &Value) -> Option<DeviceCode> {
    Some(DeviceCode {
        device_code: v["device_code"].as_str()?.to_string(),
        user_code: v["user_code"].as_str()?.to_string(),
        verification_uri: v["verification_uri"].as_str()?.to_string(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
    })
}

/// Starts signing in: show `user_code` and open `verification_uri`, then
/// call [`wait_for_token`].
pub async fn start_sign_in() -> Result<DeviceCode> {
    let body = format!(
        "client_id={}&scope={}",
        client_id()?,
        scope_list().replace(' ', "%20")
    );
    let Reply {
        code, body: text, ..
    } = curl(
        "POST",
        "https://github.com/login/device/code",
        &["Accept: application/json".into()],
        Some(&body),
    )
    .await?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    parse_device_code(&v).ok_or_else(|| {
        CoreError::GitHub(format!(
            "GitHub didn't start the sign-in ({code}): {}",
            v["error_description"]
                .as_str()
                .unwrap_or("unexpected answer")
        ))
    })
}

/// What one poll of the token endpoint said.
#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    Token(String),
    Pending,
    SlowDown,
    Failed(String),
}

pub fn parse_poll(v: &Value) -> Poll {
    if let Some(token) = v["access_token"].as_str() {
        return Poll::Token(token.to_string());
    }
    match v["error"].as_str() {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown,
        Some("expired_token") => {
            Poll::Failed("The code expired before it was entered. Start signing in again.".into())
        }
        Some("access_denied") => Poll::Failed("Sign-in was canceled on GitHub.".into()),
        _ => Poll::Failed(format!(
            "GitHub didn't finish the sign-in: {}",
            v["error_description"]
                .as_str()
                .unwrap_or("unexpected answer")
        )),
    }
}

/// Polls until the user approves on github.com, and returns the token for the caller to keep
/// (in its [`TokenStore`](super::TokenStore)).
pub async fn wait_for_token(device: &DeviceCode) -> Result<String> {
    let client_id = client_id()?;
    let body = format!(
        "client_id={client_id}&device_code={}&grant_type=urn:ietf:params:oauth:grant-type:device_code",
        device.device_code
    );
    let mut interval = device.interval.max(1);
    let deadline = std::time::Instant::now() + Duration::from_secs(device.expires_in);
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let reply = curl(
            "POST",
            "https://github.com/login/oauth/access_token",
            &["Accept: application/json".into()],
            Some(&body),
        )
        .await?;
        match parse_poll(&serde_json::from_str(&reply.body).unwrap_or(Value::Null)) {
            Poll::Token(token) => return Ok(token),
            Poll::Pending => {}
            Poll::SlowDown => interval += 5,
            Poll::Failed(message) => return Err(CoreError::GitHub(message)),
        }
    }
    Err(CoreError::GitHub(
        "The code expired before it was entered. Start signing in again.".into(),
    ))
}
