//! KitchenOwl REST client. No GTK here, so its tests run without a display.

mod client;
mod error;
mod http;
mod live;
mod models;

#[cfg(test)]
mod tests;

pub use client::{CategoryUpdate, Client, ItemUpdate, ListOrder, MIN_SERVER_BUILD, SignIn};
pub use error::{Error, Result};
pub use live::{CONNECT, Frame, LiveEvent, PONG, parse as parse_frame};
pub use models::{
    Category, Health, Household, Item, ListInfo, ListItem, Member, ShoppingList, Timestamp, User,
};
