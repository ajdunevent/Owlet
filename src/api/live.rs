use serde::Deserialize;
use serde_json::Value;
use soup::prelude::*;

use super::client::Client;
use super::error::{Error, Result};
use super::http;
use super::models::{ListInfo, ListItem};

/// Engine.IO frames carry a one-digit type; Socket.IO messages add a second digit after "4".
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Open {
        ping_interval: u32,
        ping_timeout: u32,
    },
    Ping,
    Connected,
    ConnectError(String),
    Event(LiveEvent),
    Close,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    /// Also sent when an item's description changes. The item has `category_id` rather than a
    /// nested category, so it deserializes with no category.
    ItemAdded {
        list: ListInfo,
        item: Box<ListItem>,
    },
    ItemRemoved {
        list: ListInfo,
        item_id: i64,
    },
    ListAdded(ListInfo),
    ListDeleted(ListInfo),
}

impl LiveEvent {
    pub fn household_id(&self) -> i64 {
        match self {
            Self::ItemAdded { list, .. } | Self::ItemRemoved { list, .. } => list.household_id,
            Self::ListAdded(list) | Self::ListDeleted(list) => list.household_id,
        }
    }
}

pub const CONNECT: &str = "40";
pub const PONG: &str = "3";

pub fn parse(text: &str) -> Frame {
    match text.as_bytes().first() {
        Some(b'0') => parse_open(&text[1..]),
        Some(b'1') => Frame::Close,
        Some(b'2') => Frame::Ping,
        Some(b'4') => parse_message(&text[1..]),
        _ => Frame::Other,
    }
}

fn parse_open(json: &str) -> Frame {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Open {
        ping_interval: u32,
        ping_timeout: u32,
    }
    match serde_json::from_str::<Open>(json) {
        Ok(open) => Frame::Open {
            ping_interval: open.ping_interval,
            ping_timeout: open.ping_timeout,
        },
        Err(_) => Frame::Other,
    }
}

fn parse_message(text: &str) -> Frame {
    match text.as_bytes().first() {
        Some(b'0') => Frame::Connected,
        Some(b'1') => Frame::Close,
        Some(b'2') => parse_event(&text[1..]).map_or(Frame::Other, Frame::Event),
        Some(b'4') => Frame::ConnectError(text[1..].to_owned()),
        _ => Frame::Other,
    }
}

fn parse_event(json: &str) -> Option<LiveEvent> {
    #[derive(Deserialize)]
    struct ItemPayload {
        item: ListItem,
        shoppinglist: ListInfo,
    }
    #[derive(Deserialize)]
    struct ListPayload {
        shoppinglist: ListInfo,
    }
    let (name, payload): (String, Value) = serde_json::from_str(json).ok()?;
    let event = match name.as_str() {
        "shoppinglist_item:add" => {
            let p: ItemPayload = serde_json::from_value(payload).ok()?;
            LiveEvent::ItemAdded {
                list: p.shoppinglist,
                item: Box::new(p.item),
            }
        }
        "shoppinglist_item:remove" => {
            let p: ItemPayload = serde_json::from_value(payload).ok()?;
            LiveEvent::ItemRemoved {
                list: p.shoppinglist,
                item_id: p.item.item.id,
            }
        }
        "shoppinglist:add" => LiveEvent::ListAdded(
            serde_json::from_value::<ListPayload>(payload)
                .ok()?
                .shoppinglist,
        ),
        "shoppinglist:delete" => LiveEvent::ListDeleted(
            serde_json::from_value::<ListPayload>(payload)
                .ok()?
                .shoppinglist,
        ),
        _ => return None,
    };
    Some(event)
}

impl Client {
    /// Opens the Engine.IO WebSocket. The server reads the bearer token from the upgrade
    /// request's headers, so it cannot go in the Socket.IO connect packet.
    pub async fn connect_live(&self) -> Result<soup::WebsocketConnection> {
        let mut url = self
            .base()
            .join("socket.io/")
            .map_err(|e| Error::InvalidUrl(e.to_string()))?;
        url.set_query(Some("EIO=4&transport=websocket"));
        let message = soup::Message::new("GET", url.as_str())
            .map_err(|_| Error::InvalidUrl(url.to_string()))?;
        if let (Some(token), Some(headers)) = (self.token(), message.request_headers()) {
            headers.replace("Authorization", &format!("Bearer {token}"));
        }
        let (sender, receiver) = futures_channel::oneshot::channel();
        http::live_session().websocket_connect_async(
            &message,
            None,
            &[],
            glib::Priority::DEFAULT,
            gio::Cancellable::NONE,
            move |result| {
                let _ = sender.send(result);
            },
        );
        let connection = receiver
            .await
            .map_err(|_| Error::Network("connection attempt was dropped".into()))??;
        Ok(connection)
    }
}
