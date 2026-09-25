//! Boot-time reader for the paid-proxy collection used by Kilo Gateway.
//!
//! The MongoDB connection and field mapping are durable client settings. The
//! proxy rows are runtime state: this module reads enabled rows once during
//! client startup and places them in `KiloGatewaySettings` for the inference
//! workers. It never writes to MongoDB and never serializes the rows back to
//! `config.toml`.

use ilium_inference::{
    KiloGatewaySettings, PaidProxy, ProxyDatabaseSettings, ProxyDatabaseStructure,
};
use mongodb::bson::{Bson, Document};
use mongodb::Client;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyDatabaseError {
    #[error("MongoDB URI is empty")]
    EmptyUri,
    #[error("MongoDB database name is empty")]
    EmptyDatabase,
    #[error("MongoDB collection name is empty")]
    EmptyCollection,
    #[error("MongoDB field mapping `{field}` is invalid")]
    InvalidFieldName { field: String },
    #[error("failed to connect to MongoDB at {uri}: {message}")]
    Connect { uri: String, message: String },
    #[error("failed to query MongoDB database `{database}` collection `{collection}`: {message}")]
    Query {
        database: String,
        collection: String,
        message: String,
    },
    #[error("paid proxy row {row} is invalid: {message}")]
    InvalidRow { row: usize, message: String },
    #[error("MongoDB database `{database}` collection `{collection}` has no enabled paid proxies")]
    Empty {
        database: String,
        collection: String,
    },
}

/// Loads enabled paid proxies into the runtime-only portion of the Kilo
/// settings. An enabled configuration with no usable rows is an error so the
/// provider cannot silently fall back to direct egress.
pub async fn load_paid_proxies(
    settings: &mut KiloGatewaySettings,
) -> Result<(), ProxyDatabaseError> {
    let proxies = read_paid_proxies(&settings.proxy_database).await?;
    settings.paid_proxies = proxies;
    Ok(())
}

async fn read_paid_proxies(
    database_settings: &ProxyDatabaseSettings,
) -> Result<Vec<PaidProxy>, ProxyDatabaseError> {
    validate_database_settings(database_settings)?;

    let client = Client::with_uri_str(&database_settings.uri)
        .await
        .map_err(|error| ProxyDatabaseError::Connect {
            uri: ilium_logging::redacted_url(database_settings.uri.trim()),
            message: error.to_string(),
        })?;
    let database = client.database(&database_settings.database);
    let collection = database.collection::<Document>(&database_settings.collection);

    let filter = enabled_filter(&database_settings.structure);
    let projection = proxy_projection(&database_settings.structure);
    let mut cursor = collection
        .find(filter)
        .projection(projection)
        .await
        .map_err(|error| query_error(database_settings, error))?;

    let mut proxies = Vec::new();
    let mut row = 0usize;
    while cursor
        .advance()
        .await
        .map_err(|error| query_error(database_settings, error))?
    {
        row = row.saturating_add(1);
        let document: Document = cursor
            .deserialize_current()
            .map_err(|error| query_error(database_settings, error))?;
        let proxy = parse_proxy(&document, &database_settings.structure)
            .map_err(|message| ProxyDatabaseError::InvalidRow { row, message })?;
        proxies.push(proxy);
    }

    if proxies.is_empty() {
        return Err(ProxyDatabaseError::Empty {
            database: database_settings.database.clone(),
            collection: database_settings.collection.clone(),
        });
    }

    tracing::info!(
        database = %database_settings.database,
        collection = %database_settings.collection,
        proxy_count = proxies.len(),
        "loaded enabled Kilo Gateway paid proxies from MongoDB"
    );
    Ok(proxies)
}

fn validate_database_settings(
    database_settings: &ProxyDatabaseSettings,
) -> Result<(), ProxyDatabaseError> {
    if database_settings.uri.trim().is_empty() {
        return Err(ProxyDatabaseError::EmptyUri);
    }
    if database_settings.database.trim().is_empty() {
        return Err(ProxyDatabaseError::EmptyDatabase);
    }
    if database_settings.collection.trim().is_empty() {
        return Err(ProxyDatabaseError::EmptyCollection);
    }

    for field in structure_fields(&database_settings.structure) {
        if field.is_empty() || field.starts_with('$') || field.contains('.') {
            return Err(ProxyDatabaseError::InvalidFieldName {
                field: field.to_owned(),
            });
        }
    }
    Ok(())
}

fn structure_fields(structure: &ProxyDatabaseStructure) -> [&str; 6] {
    [
        structure.ip.as_str(),
        structure.port.as_str(),
        structure.protocol.as_str(),
        structure.username.as_str(),
        structure.password.as_str(),
        structure.enabled.as_str(),
    ]
}

fn enabled_filter(structure: &ProxyDatabaseStructure) -> Document {
    let mut filter = Document::new();
    filter.insert(structure.enabled.clone(), Bson::Boolean(true));
    filter
}

fn proxy_projection(structure: &ProxyDatabaseStructure) -> Document {
    let mut projection = Document::new();
    projection.insert("_id", Bson::Int32(0));
    for field in structure_fields(structure) {
        projection.insert(field, Bson::Int32(1));
    }
    projection
}

fn parse_proxy(
    document: &Document,
    structure: &ProxyDatabaseStructure,
) -> Result<PaidProxy, String> {
    let ip = required_string(document, &structure.ip)?;
    let port = read_port(document, &structure.port)?;
    let protocol = required_string(document, &structure.protocol)?;
    let username = optional_string(document, &structure.username)?;
    let password = optional_string(document, &structure.password)?;

    Ok(PaidProxy {
        ip,
        port,
        protocol,
        username,
        password,
    })
}

fn required_string(document: &Document, field: &str) -> Result<String, String> {
    let value = document
        .get(field)
        .ok_or_else(|| format!("missing required field `{field}`"))?;
    let Bson::String(value) = value else {
        return Err(format!("field `{field}` must be a string"));
    };
    if value.trim().is_empty() {
        return Err(format!("field `{field}` must not be empty"));
    }
    Ok(value.clone())
}

fn optional_string(document: &Document, field: &str) -> Result<String, String> {
    let Some(value) = document.get(field) else {
        return Ok(String::new());
    };
    let Bson::String(value) = value else {
        return Err(format!("field `{field}` must be a string when present"));
    };
    Ok(value.clone())
}

fn read_port(document: &Document, field: &str) -> Result<u16, String> {
    let value = document
        .get(field)
        .ok_or_else(|| format!("missing required field `{field}`"))?;
    let number = match value {
        Bson::Int32(value) => i64::from(*value),
        Bson::Int64(value) => *value,
        Bson::Double(value) if value.is_finite() && value.fract() == 0.0 => *value as i64,
        _ => return Err(format!("field `{field}` must be an integer")),
    };
    let port =
        u16::try_from(number).map_err(|_| format!("field `{field}` is outside 1..=65535"))?;
    if port == 0 {
        return Err(format!("field `{field}` is outside 1..=65535"));
    }
    Ok(port)
}

fn query_error(
    database_settings: &ProxyDatabaseSettings,
    error: mongodb::error::Error,
) -> ProxyDatabaseError {
    ProxyDatabaseError::Query {
        database: database_settings.database.clone(),
        collection: database_settings.collection.clone(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_structure() -> ProxyDatabaseStructure {
        ProxyDatabaseStructure {
            ip: "host".to_string(),
            port: "port_number".to_string(),
            protocol: "scheme".to_string(),
            username: "user_name".to_string(),
            password: "pass_word".to_string(),
            enabled: "active".to_string(),
        }
    }

    #[test]
    fn projection_and_filter_follow_configured_field_names() {
        let structure = fixture_structure();
        let filter = enabled_filter(&structure);
        let projection = proxy_projection(&structure);

        assert_eq!(filter.get_bool("active"), Ok(true));
        assert_eq!(projection.get_i32("host"), Ok(1));
        assert_eq!(projection.get_i32("port_number"), Ok(1));
        assert_eq!(projection.get_i32("_id"), Ok(0));
        assert!(projection.get("ip").is_none());
    }

    #[test]
    fn parses_the_configured_document_shape() {
        let structure = fixture_structure();
        let document = mongodb::bson::doc! {
            "host": "198.51.100.7",
            "port_number": 8080_i32,
            "scheme": "http",
            "user_name": "user",
            "pass_word": "pass",
            "active": true,
        };

        assert_eq!(
            parse_proxy(&document, &structure).expect("valid proxy document"),
            PaidProxy {
                ip: "198.51.100.7".to_string(),
                port: 8080,
                protocol: "http".to_string(),
                username: "user".to_string(),
                password: "pass".to_string(),
            }
        );
    }

    #[test]
    fn absent_credentials_are_treated_as_ip_authorized() {
        let structure = fixture_structure();
        let document = mongodb::bson::doc! {
            "host": "198.51.100.7",
            "port_number": 8080_i32,
            "scheme": "http",
            "active": true,
        };

        let proxy = parse_proxy(&document, &structure).expect("valid IP-authorized proxy");
        assert!(proxy.username.is_empty());
        assert!(proxy.password.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires the configured local MongoDB paid_proxies collection"]
    async fn live_money_collection_loads_enabled_proxies() {
        let mut settings = KiloGatewaySettings {
            paid_proxies_enabled: true,
            ..KiloGatewaySettings::default()
        };
        load_paid_proxies(&mut settings)
            .await
            .expect("configured MongoDB paid_proxies collection should load");
        assert!(!settings.paid_proxies.is_empty());
    }
}
