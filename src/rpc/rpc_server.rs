use std::{net::SocketAddr, sync::Arc};

use async_dnssd;
use futures::{future, StreamExt, TryStreamExt};
use rustls::RootCertStore;
use tarpc::{
    context,
    rpc::server::{BaseChannel, Channel, Handler},
    server,
};

use namib_shared::{codec, models::DHCPRequestData, open_file_with, rpc::RPC};

use crate::error::Result;
use namib_shared::config_firewall::{FirewallConfig, FirewallRule};

use super::tls_serde_transport;
use crate::{
    error::*,
    services::{config_firewall_service, device_service, mud_service},
};

#[derive(Clone)]
pub struct RPCServer(SocketAddr);

#[server]
impl RPC for RPCServer {
    async fn heartbeat(self, _: context::Context, version: Option<String>) -> Option<FirewallConfig> {
        debug!("Received a heartbeat from client {:?} with version {:?}", self.0, version);
        let current_config_version = mud_service::get_config_version().await;
        if Some(&current_config_version) != version.as_ref() {
            debug!("Client has outdated version \"{}\". Starting update...", current_config_version);
            let devices = device_service::get_all_devices().await;
            let rules: Vec<FirewallRule> = devices
                .iter()
                .flat_map(move |d| config_firewall_service::convert_device_to_fw_rules(d).unwrap_or_else(|_| Vec::new()))
                .collect();

            debug!("Returning Heartbeat to client with config: {:#?}", rules);
            return Some(FirewallConfig::new(current_config_version, rules));
        }

        None
    }

    async fn dhcp_request(self, _: context::Context, dhcp_data: DHCPRequestData) {
        debug!("dhcp_request from: {:?}. Data: {:?}", self.0, dhcp_data);
    }
}

pub async fn listen() -> Result<()> {
    debug!("Registering in dnssd");
    let (_registration, result) = async_dnssd::register("_namib_controller._tcp", 8734)?.await?;
    info!("Registered: {:?}", result);

    // Build TLS configuration.
    let tls_cfg = {
        // Use client certificate authentication.
        let mut client_auth_roots = RootCertStore::empty();
        open_file_with("../namib_shared/certs/ca.pem", |b| client_auth_roots.add_pem_file(b))?;

        // Load server cert
        let certs = open_file_with("certs/server.pem", rustls::internal::pemfile::certs)?;
        let key = open_file_with("certs/server-key.pem", rustls::internal::pemfile::rsa_private_keys)?[0].clone();

        let mut cfg = rustls::ServerConfig::new(rustls::AllowAnyAuthenticatedClient::new(client_auth_roots));
        cfg.set_single_cert(certs, key)?;

        Arc::new(cfg)
    };

    let addr: SocketAddr = "0.0.0.0:8734".parse()?;
    info!("Starting to serve on {}.", addr);

    // Create a TLS listener via tokio.
    let mut listener = tls_serde_transport::listen(tls_cfg, addr, codec()).await?;
    listener.config_mut().max_frame_length(50 * 1024 * 1024);
    listener
        // Ignore accept errors.
        .inspect_err(|err| warn!("Failed to accept {:?}", err))
        .filter_map(|r| future::ready(r.ok()))
        .map(BaseChannel::with_defaults)
        // Limit channels to 1 per IP.
        .max_channels_per_key(1, |t| t.get_ref().get_ref().get_ref().0.peer_addr().unwrap().ip())
        // serve is generated by the service attribute. It takes as input any type implementing
        // the generated World trait.
        .map(|channel| {
            let server = RPCServer(channel.get_ref().get_ref().get_ref().get_ref().0.peer_addr().unwrap());
            channel.respond_with(server.serve()).execute()
        })
        // Max 10 channels.
        .buffer_unordered(10)
        .for_each(|_| async {})
        .await;

    info!("done");

    Ok(())
}
