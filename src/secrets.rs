use std::collections::HashMap;

const SCHEMA_NAME: &str = "net.dunevent.Owlet.Token";
const ATTR_SERVER: &str = "server";
const ATTR_USERNAME: &str = "username";

fn schema() -> libsecret::Schema {
    let attributes = HashMap::from([
        (ATTR_SERVER, libsecret::SchemaAttributeType::String),
        (ATTR_USERNAME, libsecret::SchemaAttributeType::String),
    ]);
    libsecret::Schema::new(SCHEMA_NAME, libsecret::SchemaFlags::NONE, attributes)
}

fn attributes<'a>(server: &'a str, username: &'a str) -> HashMap<&'a str, &'a str> {
    HashMap::from([(ATTR_SERVER, server), (ATTR_USERNAME, username)])
}

pub async fn load_token(server: &str, username: &str) -> Result<Option<String>, glib::Error> {
    libsecret::password_lookup_future(Some(&schema()), attributes(server, username))
        .await
        .map(|secret| secret.map(|s| s.to_string()))
}

pub async fn store_token(server: &str, username: &str, token: &str) -> Result<(), glib::Error> {
    let label = format!("Owlet: {username} on {server}");
    libsecret::password_store_future(
        Some(&schema()),
        attributes(server, username),
        None,
        &label,
        token,
    )
    .await
}

pub async fn clear_token(server: &str, username: &str) -> Result<(), glib::Error> {
    libsecret::password_clear_future(Some(&schema()), attributes(server, username))
        .await
        .map(drop)
}
