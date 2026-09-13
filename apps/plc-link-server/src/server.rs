//! Starts the hub, PLC port, passive PLC clients and the control API.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::api::{self, ApiState, ServerInfo};
use crate::client::{self, ConnectSpec};
use crate::contract::Loaded;
use crate::hub::{Hub, HubConfig};
use crate::plc_port;

pub struct ServeConfig {
    pub plc_bind: String,
    pub api_bind: String,
    pub contract: Loaded,
    pub hub: HubConfig,
    pub connects: Vec<ConnectSpec>,
}

/// A running server; dropping it stops its tasks.
pub struct Running {
    pub hub: Arc<Hub>,
    pub plc_addr: SocketAddr,
    pub api_addr: SocketAddr,
    tasks: Vec<JoinHandle<()>>,
}

impl Running {
    /// `ip:port` of the control API (for clients).
    pub fn api(&self) -> String {
        self.api_addr.to_string()
    }

    pub fn add_connect(&mut self, spec: ConnectSpec) {
        self.tasks.push(client::spawn(self.hub.clone(), spec));
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

pub async fn start(cfg: ServeConfig) -> anyhow::Result<Running> {
    let log_dir = cfg.hub.log_dir.as_ref().map(|d| d.display().to_string());
    let hub = Hub::new(cfg.contract.codec.clone(), &cfg.contract.name, cfg.hub);
    let plc_l = TcpListener::bind(&cfg.plc_bind).await.with_context(|| format!("bind PLC port {}", cfg.plc_bind))?;
    let api_l = TcpListener::bind(&cfg.api_bind).await.with_context(|| format!("bind control API {}", cfg.api_bind))?;
    let plc_addr = plc_l.local_addr()?;
    let api_addr = api_l.local_addr()?;
    let mut tasks = vec![hub.spawn_maintenance(), tokio::spawn(plc_port::serve(plc_l, hub.clone()))];
    for c in cfg.connects {
        tasks.push(client::spawn(hub.clone(), c));
    }
    let info = ServerInfo {
        plc_bind: plc_addr.to_string(),
        api_bind: api_addr.to_string(),
        contract_dir: cfg.contract.dir.display().to_string(),
        messages: cfg.contract.messages.display().to_string(),
        fallback_udts: cfg.contract.fallback_udts.clone(),
        log_dir,
    };
    let app = api::router(ApiState { hub: hub.clone(), info: Arc::new(info) });
    tasks.push(tokio::spawn(async move {
        if let Err(e) = axum::serve(api_l, app).await {
            tracing::error!(%e, "control API");
        }
    }));
    Ok(Running { hub, plc_addr, api_addr, tasks })
}
