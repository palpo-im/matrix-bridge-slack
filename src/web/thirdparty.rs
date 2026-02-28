use std::collections::HashMap;

use salvo::prelude::*;
use serde::Serialize;
use serde_json::json;

use crate::web::web_state;

#[derive(Debug, Clone, Serialize)]
pub struct ThirdPartyProtocol {
    pub user_fields: Vec<String>,
    pub location_fields: Vec<String>,
    pub field_types: HashMap<String, ThirdPartyFieldType>,
    pub instances: Vec<ThirdPartyInstance>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThirdPartyFieldType {
    #[serde(rename = "type")]
    pub field_type: String,
    pub placeholder: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThirdPartyInstance {
    pub network_id: String,
    pub bot_user_id: String,
    pub desc: String,
    pub icon: Option<String>,
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThirdPartyLocation {
    pub alias: String,
    pub protocol: String,
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThirdPartyUser {
    pub userid: String,
    pub protocol: String,
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThirdPartyNetwork {
    pub name: String,
    pub protocol: String,
    pub fields: HashMap<String, String>,
}

fn render_error(res: &mut Response, status: StatusCode, message: &str) {
    res.status_code(status);
    res.render(Json(json!({ "error": message })));
}

fn protocol_payload(bot_user_id: &str) -> ThirdPartyProtocol {
    let mut field_types = HashMap::new();
    field_types.insert(
        "guild_id".to_string(),
        ThirdPartyFieldType {
            field_type: "text".to_string(),
            placeholder: "Slack guild id".to_string(),
        },
    );
    field_types.insert(
        "channel_id".to_string(),
        ThirdPartyFieldType {
            field_type: "text".to_string(),
            placeholder: "Slack channel id".to_string(),
        },
    );
    field_types.insert(
        "userid".to_string(),
        ThirdPartyFieldType {
            field_type: "text".to_string(),
            placeholder: "Slack user id".to_string(),
        },
    );

    ThirdPartyProtocol {
        user_fields: vec!["userid".to_string()],
        location_fields: vec!["guild_id".to_string(), "channel_id".to_string()],
        field_types,
        instances: vec![ThirdPartyInstance {
            network_id: "slack".to_string(),
            bot_user_id: bot_user_id.to_string(),
            desc: "Slack".to_string(),
            icon: None,
            fields: HashMap::new(),
        }],
    }
}

#[handler]
pub async fn get_protocol(res: &mut Response) {
    let matrix_client = &web_state().matrix_client;
    let bot_user_id = matrix_client.bot_user_id();
    res.render(Json(protocol_payload(&bot_user_id)));
}

#[handler]
pub async fn get_networks(res: &mut Response) {
    let room_store = web_state().db_manager.room_store();
    match room_store.list_room_mappings(i64::MAX, 0).await {
        Ok(mappings) => {
            let mut by_guild: HashMap<String, ThirdPartyNetwork> = HashMap::new();
            for mapping in mappings {
                by_guild
                    .entry(mapping.slack_team_id.clone())
                    .or_insert_with(|| ThirdPartyNetwork {
                        name: mapping.slack_team_id.clone(),
                        protocol: "slack".to_string(),
                        fields: HashMap::from([(
                            "guild_id".to_string(),
                            mapping.slack_team_id.clone(),
                        )]),
                    });
            }
            let networks: Vec<ThirdPartyNetwork> = by_guild.into_values().collect();
            res.render(Json(networks));
        }
        Err(err) => {
            render_error(
                res,
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("database error: {}", err),
            );
        }
    }
}

#[handler]
pub async fn get_locations(req: &mut Request, res: &mut Response) {
    let guild_filter = req.query::<String>("guild_id");
    let channel_filter = req.query::<String>("channel_id");
    let domain = web_state().matrix_client.config().bridge.domain.clone();

    let room_store = web_state().db_manager.room_store();
    match room_store.list_room_mappings(i64::MAX, 0).await {
        Ok(mappings) => {
            let locations: Vec<ThirdPartyLocation> = mappings
                .into_iter()
                .filter(|mapping| {
                    guild_filter
                        .as_ref()
                        .map(|guild| mapping.slack_team_id == *guild)
                        .unwrap_or(true)
                        && channel_filter
                            .as_ref()
                            .map(|channel| mapping.slack_channel_id == *channel)
                            .unwrap_or(true)
                })
                .map(|mapping| ThirdPartyLocation {
                    alias: format!("#_slack_{}:{}", mapping.slack_channel_id, domain),
                    protocol: "slack".to_string(),
                    fields: HashMap::from([
                        ("guild_id".to_string(), mapping.slack_team_id),
                        ("channel_id".to_string(), mapping.slack_channel_id),
                    ]),
                })
                .collect();
            res.render(Json(locations));
        }
        Err(err) => {
            render_error(
                res,
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("database error: {}", err),
            );
        }
    }
}

#[handler]
pub async fn get_users(req: &mut Request, res: &mut Response) {
    let user_filter = req
        .query::<String>("userid")
        .or_else(|| req.query::<String>("user_id"));
    let domain = web_state().matrix_client.config().bridge.domain.clone();

    let user_store = web_state().db_manager.user_store();
    match user_store.get_all_user_ids().await {
        Ok(user_ids) => {
            let users: Vec<ThirdPartyUser> = user_ids
                .into_iter()
                .filter(|user_id| {
                    user_filter
                        .as_ref()
                        .map(|filter| user_id.contains(filter))
                        .unwrap_or(true)
                })
                .map(|slack_user_id| ThirdPartyUser {
                    userid: format!("@_slack_{}:{}", slack_user_id, domain),
                    protocol: "slack".to_string(),
                    fields: HashMap::from([("userid".to_string(), slack_user_id)]),
                })
                .collect();
            res.render(Json(users));
        }
        Err(err) => {
            render_error(
                res,
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("database error: {}", err),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::protocol_payload;

    #[test]
    fn protocol_payload_contains_expected_fields() {
        let payload = protocol_payload("@_slack_bot:example.org");
        assert!(!payload.user_fields.is_empty());
        assert!(!payload.location_fields.is_empty());
        assert_eq!(payload.instances[0].network_id, "slack");
    }
}

