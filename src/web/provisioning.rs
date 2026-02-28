use chrono::Utc;
use salvo::prelude::*;
use serde_json::json;

use crate::db::RoomMapping;
use crate::web::web_state;

fn render_error(res: &mut Response, status: StatusCode, message: &str) {
    res.status_code(status);
    res.render(Json(json!({ "error": message })));
}

#[handler]
pub async fn list_rooms(req: &mut Request, res: &mut Response) {
    let limit = req.query::<i64>("limit").unwrap_or(100).clamp(1, 1000);
    let offset = req.query::<i64>("offset").unwrap_or(0).max(0);

    match web_state()
        .db_manager
        .room_store()
        .list_room_mappings(limit, offset)
        .await
    {
        Ok(rooms) => {
            res.render(Json(json!({
                "rooms": rooms,
                "count": rooms.len(),
                "limit": limit,
                "offset": offset,
            })));
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
pub async fn create_bridge(req: &mut Request, res: &mut Response) {
    let matrix_room_id = match req.query::<String>("matrix_room_id") {
        Some(v) if !v.is_empty() => v,
        _ => {
            render_error(
                res,
                StatusCode::BAD_REQUEST,
                "missing matrix_room_id query parameter",
            );
            return;
        }
    };
    let slack_channel_id = match req.query::<String>("slack_channel_id") {
        Some(v) if !v.is_empty() => v,
        _ => {
            render_error(
                res,
                StatusCode::BAD_REQUEST,
                "missing slack_channel_id query parameter",
            );
            return;
        }
    };
    let slack_team_id = req
        .query::<String>("slack_team_id")
        .unwrap_or_else(|| "unknown_guild".to_string());

    let bridge = web_state().bridge.clone();

    match bridge
        .bridge_matrix_room(&matrix_room_id, &slack_team_id, &slack_channel_id)
        .await
    {
        Ok(reply) => {
            if reply.contains("problem") || reply.contains("already") {
                render_error(res, StatusCode::BAD_REQUEST, &reply);
            } else {
                res.status_code(StatusCode::CREATED);
                res.render(Json(json!({
                    "ok": true,
                    "message": reply,
                })));
            }
        }
        Err(err) => {
            render_error(res, StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
        }
    }
}

#[handler]
pub async fn delete_bridge(req: &mut Request, res: &mut Response) {
    let id = match req.param::<i64>("id") {
        Some(v) if v > 0 => v,
        _ => {
            render_error(res, StatusCode::BAD_REQUEST, "invalid bridge id");
            return;
        }
    };

    // Find mapping by ID first to get matrix_room_id
    let room_store = web_state().db_manager.room_store();
    let mapping = match room_store.get_room_by_id(id).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            render_error(res, StatusCode::NOT_FOUND, "bridge not found");
            return;
        }
        Err(err) => {
            render_error(res, StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
            return;
        }
    };

    match web_state()
        .bridge
        .unbridge_matrix_room(&mapping.matrix_room_id)
        .await
    {
        Ok(reply) => {
            res.render(Json(json!({ "ok": true, "message": reply })));
        }
        Err(err) => {
            render_error(res, StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
        }
    }
}

#[handler]
pub async fn get_bridge_info(req: &mut Request, res: &mut Response) {
    let id = match req.param::<i64>("id") {
        Some(v) if v > 0 => v,
        _ => {
            render_error(res, StatusCode::BAD_REQUEST, "invalid bridge id");
            return;
        }
    };

    match web_state().db_manager.room_store().get_room_by_id(id).await {
        Ok(Some(mapping)) => {
            res.render(Json(json!({ "mapping": mapping })));
        }
        Ok(None) => {
            render_error(res, StatusCode::NOT_FOUND, "bridge not found");
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
