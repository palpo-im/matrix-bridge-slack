use salvo::prelude::*;
use serde_json::json;

use crate::web::web_state;

#[handler]
pub async fn health_check(res: &mut Response) {
    res.render("OK");
}

#[handler]
pub async fn get_status(res: &mut Response) {
    let state = web_state();
    let uptime_seconds = state.started_at.elapsed().as_secs();

    let status = json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": uptime_seconds,
        "bridge": {
            "domain": state.matrix_client.registration_preview().get("url"),
        }
    });

    res.render(Json(status));
}
