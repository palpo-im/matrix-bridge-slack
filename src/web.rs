use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use once_cell::sync::OnceCell;
use salvo::prelude::*;
use tracing::info;

use crate::bridge::BridgeCore;
use crate::config::Config;
use crate::db::DatabaseManager;
use crate::matrix::MatrixAppservice;

mod health;
mod metrics;
mod provisioning;
mod thirdparty;

use health::{get_status, health_check};
use metrics::metrics_endpoint;
use provisioning::{create_bridge, delete_bridge, get_bridge_info, list_rooms};
use thirdparty::{get_locations, get_networks, get_protocol, get_users};

#[derive(Clone)]
pub struct WebState {
    pub db_manager: Arc<DatabaseManager>,
    pub matrix_client: Arc<MatrixAppservice>,
    pub bridge: Arc<BridgeCore>,
    pub started_at: Instant,
}

static WEB_STATE: OnceCell<WebState> = OnceCell::new();

pub fn web_state() -> &'static WebState {
    WEB_STATE
        .get()
        .expect("web state is not initialized before handler execution")
}

#[derive(Clone)]
pub struct WebServer {
    config: Arc<Config>,
    matrix_client: Arc<MatrixAppservice>,
}

impl WebServer {
    pub async fn new(
        config: Arc<Config>,
        matrix_client: Arc<MatrixAppservice>,
        db_manager: Arc<DatabaseManager>,
        bridge: Arc<BridgeCore>,
    ) -> Result<Self> {
        let _ = WEB_STATE.set(WebState {
            db_manager,
            matrix_client: matrix_client.clone(),
            bridge,
            started_at: Instant::now(),
        });

        Ok(Self {
            config,
            matrix_client,
        })
    }

    pub async fn start(&self) -> Result<()> {
        let bind_addr = format!(
            "{}:{}",
            self.config.bridge.bind_address, self.config.bridge.port
        );
        info!("starting web server on {}", bind_addr);

        let acceptor = TcpListener::new(bind_addr).bind().await;
        let appservice_router = self.matrix_client.appservice.router();
        let main_router = root_router().push(appservice_router);
        Server::new(acceptor).serve(main_router).await;

        Ok(())
    }
}

pub fn root_router() -> Router {
    Router::new()
        .push(Router::with_path("health").get(health_check))
        .push(Router::with_path("status").get(get_status))
        .push(Router::with_path("metrics").get(metrics_endpoint))
        .push(
            Router::with_path("_matrix/app/v1")
                .push(Router::with_path("rooms").get(list_rooms))
                .push(Router::with_path("bridges").post(create_bridge))
                .push(
                    Router::with_path("bridges/{id}")
                        .get(get_bridge_info)
                        .delete(delete_bridge),
                )
                .push(
                    Router::with_path("thirdparty")
                        .push(Router::with_path("protocol").get(get_protocol))
                        .push(Router::with_path("protocol/slack").get(get_protocol))
                        .push(Router::with_path("network").get(get_networks))
                        .push(Router::with_path("network/slack").get(get_networks))
                        .push(Router::with_path("location").get(get_locations))
                        .push(Router::with_path("location/slack").get(get_locations))
                        .push(Router::with_path("user").get(get_users))
                        .push(Router::with_path("user/slack").get(get_users)),
                ),
        )
        .push(
            Router::with_path("admin")
                .push(
                    Router::with_path("bridges")
                        .get(list_rooms)
                        .post(create_bridge),
                )
                .push(
                    Router::with_path("bridges/{id}")
                        .get(get_bridge_info)
                        .delete(delete_bridge),
                ),
        )
}
