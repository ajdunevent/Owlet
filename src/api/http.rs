use std::cell::OnceCell;

use glib::translate::IntoGlib;
use serde::de::DeserializeOwned;
use soup::prelude::*;

use super::error::{Error, Result};

/// A request that times out counts as being offline.
const TIMEOUT_SECONDS: u32 = 10;

thread_local! {
    static SESSION: OnceCell<soup::Session> = const { OnceCell::new() };
    static LIVE_SESSION: OnceCell<soup::Session> = const { OnceCell::new() };
}

fn new_session(timeout: u32) -> soup::Session {
    let session = soup::Session::new();
    session.set_user_agent(&format!("Owlet/{}", env!("CARGO_PKG_VERSION")));
    session.set_timeout(timeout);
    session
}

fn session() -> soup::Session {
    SESSION.with(|cell| cell.get_or_init(|| new_session(TIMEOUT_SECONDS)).clone())
}

/// The socket sits idle between pings, so its connection has no I/O timeout; Engine.IO's
/// ping deadline detects a dead link instead.
pub fn live_session() -> soup::Session {
    LIVE_SESSION.with(|cell| {
        cell.get_or_init(|| {
            let session = new_session(0);
            session.set_idle_timeout(0);
            session
        })
        .clone()
    })
}

pub struct Response {
    pub status: u32,
    pub body: glib::Bytes,
}

impl Response {
    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_slice(&self.body)?)
    }

    /// Maps KitchenOwl's error statuses; its error bodies carry a `msg`.
    pub fn check(self) -> Result<Self> {
        if (200..300).contains(&self.status) {
            return Ok(self);
        }
        let message = serde_json::from_slice::<serde_json::Value>(&self.body)
            .ok()
            .and_then(|v| v.get("msg").and_then(|m| m.as_str()).map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&self.body).into_owned());
        Err(match self.status {
            400 => Error::Refused(message),
            // A token that no longer decodes, such as after the server's secret changes,
            // gets 422 from flask-jwt-extended rather than 401.
            401 | 422 => Error::Unauthorized,
            404 => Error::NotFound,
            status => Error::Http { status, message },
        })
    }
}

pub async fn request(
    method: &str,
    url: &str,
    bearer: Option<&str>,
    payload: Option<&serde_json::Value>,
) -> Result<Response> {
    let message = soup::Message::new(method, url).map_err(|_| Error::InvalidUrl(url.to_owned()))?;
    if let (Some(token), Some(headers)) = (bearer, message.request_headers()) {
        headers.replace("Authorization", &format!("Bearer {token}"));
    }
    if let Some(payload) = payload {
        message.set_request_body_from_bytes(
            Some("application/json"),
            Some(&glib::Bytes::from_owned(serde_json::to_vec(payload)?)),
        );
    }
    let body = session()
        .send_and_read_future(&message, glib::Priority::DEFAULT)
        .await?;
    Ok(Response {
        status: message.status().into_glib() as u32,
        body,
    })
}
