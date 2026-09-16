//! Device discovery: UDP multicast announce/answer plus an HTTP subnet scan.
//!
//! Two independent ways to find peers, because multicast is blocked on plenty
//! of routers:
//!
//! 1. Multicast. We announce ourselves to 224.0.0.167:53317. Every peer that
//!    hears an announce answers with `POST /api/localsend/v2/register`. We do
//!    the same for the announces we hear, falling back to a UDP answer when
//!    the HTTP call fails.
//! 2. Subnet scan. `rescan()` probes `/register` on every host of the local
//!    /24, which needs no multicast at all.
//!
//! The official app requires a client certificate on every HTTPS request, so
//! our HTTP client always presents ours.

use crate::protocol::{
    AnnounceMessage, DeviceInfo, DeviceType, ProtocolType, RegisterResponse, DEFAULT_PORT,
    MULTICAST_GROUP,
};
use serde::Serialize;
use socket2::{Domain, Protocol as SockProtocol, Socket, Type};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;

/// Devices not heard from within this window are dropped from the list.
pub const DEVICE_TIMEOUT: Duration = Duration::from_secs(60);
/// How often we re-announce ourselves.
pub const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(10);
/// An announce burst repeats the datagram: a single one is easily lost, and a
/// device that just joined may not be listening yet. Same delays as the
/// official app.
const ANNOUNCE_BURST_DELAYS: [Duration; 3] = [
    Duration::from_millis(100),
    Duration::from_millis(500),
    Duration::from_millis(2000),
];
/// Timeout of a single register request. LAN peers answer fast or not at all;
/// this also bounds how long one dead host stalls a scan.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
/// How many hosts a subnet scan probes at once, matching the official app.
const SCAN_CONCURRENCY: usize = 50;
const PRUNE_INTERVAL: Duration = Duration::from_secs(5);
/// How often known devices are re-probed to keep their last-seen fresh.
///
/// The official app announces on startup and on user refresh, not on a timer,
/// so without this a still-present peer would age out after 60s.
const REFRESH_INTERVAL: Duration = Duration::from_secs(20);
const RECEIVE_BUFFER_SIZE: usize = 65536;
pub const REGISTER_PATH: &str = "/api/localsend/v2/register";

/// A discovered peer, as handed to the frontend.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// The peer's fingerprint. Doubles as its stable id.
    pub fingerprint: String,
    pub alias: String,
    pub device_model: Option<String>,
    pub device_type: DeviceType,
    pub ip: String,
    pub port: u16,
    pub protocol: ProtocolType,
    pub download: bool,
    /// Unix milliseconds of the last confirmation.
    pub last_seen: u64,
}

impl Device {
    /// Everything except `last_seen`. A pure refresh must not churn the UI.
    fn visibly_same(&self, other: &Device) -> bool {
        self.fingerprint == other.fingerprint
            && self.alias == other.alias
            && self.device_model == other.device_model
            && self.device_type == other.device_type
            && self.ip == other.ip
            && self.port == other.port
            && self.protocol == other.protocol
            && self.download == other.download
    }

    fn from_announce(info: &DeviceInfo, ip: Ipv4Addr, now: u64) -> Device {
        Device {
            fingerprint: info.fingerprint.clone(),
            alias: info.alias.clone(),
            device_model: info.device_model.clone(),
            device_type: info.device_type.unwrap_or_default(),
            ip: ip.to_string(),
            port: info.port,
            protocol: info.protocol,
            download: info.download,
            last_seen: now,
        }
    }

    fn from_register_response(
        res: RegisterResponse,
        ip: Ipv4Addr,
        probed_port: u16,
        probed_protocol: ProtocolType,
        now: u64,
    ) -> Device {
        Device {
            fingerprint: res.fingerprint,
            alias: res.alias,
            device_model: res.device_model,
            device_type: res.device_type.unwrap_or_default(),
            ip: ip.to_string(),
            port: res.port.unwrap_or(probed_port),
            protocol: res.protocol.unwrap_or(probed_protocol),
            download: res.download,
            last_seen: now,
        }
    }
}

/// The set of currently known peers, keyed by fingerprint.
pub struct Registry {
    self_fingerprint: String,
    devices: Mutex<HashMap<String, Device>>,
}

impl Registry {
    pub fn new(self_fingerprint: String) -> Self {
        Registry {
            self_fingerprint,
            devices: Mutex::new(HashMap::new()),
        }
    }

    /// Adds or refreshes a device. Returns whether the visible list changed,
    /// i.e. whether an event is worth emitting. Our own fingerprint is
    /// ignored: multicast loopback means we hear our own announces.
    pub fn upsert(&self, device: Device) -> bool {
        if device.fingerprint.is_empty() || device.fingerprint == self.self_fingerprint {
            return false;
        }
        let mut devices = self.devices.lock().expect("registry poisoned");
        match devices.get(&device.fingerprint) {
            Some(existing) if existing.visibly_same(&device) => {
                devices.insert(device.fingerprint.clone(), device);
                false
            }
            _ => {
                devices.insert(device.fingerprint.clone(), device);
                true
            }
        }
    }

    /// Drops devices last seen more than `timeout` ago. Returns whether
    /// anything was dropped.
    pub fn prune(&self, now: u64, timeout: Duration) -> bool {
        let cutoff = timeout.as_millis() as u64;
        let mut devices = self.devices.lock().expect("registry poisoned");
        let before = devices.len();
        devices.retain(|_, d| now.saturating_sub(d.last_seen) < cutoff);
        devices.len() != before
    }

    /// All known devices, ordered so the UI does not reshuffle on every event.
    pub fn list(&self) -> Vec<Device> {
        let devices = self.devices.lock().expect("registry poisoned");
        let mut list: Vec<Device> = devices.values().cloned().collect();
        list.sort_by(|a, b| {
            a.alias
                .to_lowercase()
                .cmp(&b.alias.to_lowercase())
                .then_with(|| a.fingerprint.cmp(&b.fingerprint))
        });
        list
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Every host of `local`'s /24, minus the network, broadcast and own address.
pub fn subnet_hosts(local: Ipv4Addr) -> Vec<Ipv4Addr> {
    let [a, b, c, own] = local.octets();
    (1u8..=254)
        .filter(|host| *host != own)
        .map(|host| Ipv4Addr::new(a, b, c, host))
        .collect()
}

/// The address of the interface the LAN is on.
///
/// Connecting a UDP socket sends nothing; it only makes the kernel pick a
/// source address for that destination. The multicast group is asked first,
/// because that follows the 224.0.0.0/4 route: asking a public address
/// instead would answer with the tunnel address whenever a VPN owns the
/// default route, and neither multicast nor the /24 scan works there.
///
/// Machines with several LAN interfaces are only discovered on this one,
/// which is the common case on a laptop.
pub fn local_ipv4() -> Option<Ipv4Addr> {
    route_source(IpAddr::V4(MULTICAST_GROUP), DEFAULT_PORT)
        .or_else(|| route_source(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), 53))
}

fn route_source(destination: IpAddr, port: u16) -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((destination, port)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Binds the multicast socket and joins the group.
///
/// `SO_REUSEPORT` matters: the official app may already hold port 53317 on
/// this machine, and both processes must receive the datagrams. Loopback is
/// left on for the same reason, which is why announces from our own
/// fingerprint have to be filtered out.
fn bind_multicast_socket(port: u16) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(SockProtocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).into())?;
    socket.join_multicast_v4(&MULTICAST_GROUP, &Ipv4Addr::UNSPECIFIED)?;
    if let Some(local) = local_ipv4() {
        // Best effort: already-joined and unsupported-interface both surface
        // as errors here and neither is fatal.
        let _ = socket.join_multicast_v4(&MULTICAST_GROUP, &local);
        let _ = socket.set_multicast_if_v4(&local);
    }
    socket.set_multicast_loop_v4(true)?;
    UdpSocket::from_std(socket.into())
}

/// Builds the HTTP client used for `/register`.
///
/// Peers use self-signed certificates and are identified by fingerprint, not
/// by a CA, so certificate verification is off by design. The official app's
/// server requires a client certificate, so ours is always presented.
fn build_client(cert_pem: &str, key_pem: &str) -> Result<reqwest::Client, String> {
    let mut bundle = Vec::with_capacity(cert_pem.len() + key_pem.len());
    bundle.extend_from_slice(cert_pem.as_bytes());
    bundle.extend_from_slice(key_pem.as_bytes());
    let identity = reqwest::Identity::from_pem(&bundle).map_err(|e| e.to_string())?;
    reqwest::Client::builder()
        .identity(identity)
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())
}

/// The running discovery service.
pub struct Discovery {
    pub registry: Arc<Registry>,
    info: DeviceInfo,
    client: reqwest::Client,
    socket: Option<Arc<UdpSocket>>,
    /// Called with the current device list whenever it visibly changes.
    notify: Arc<dyn Fn(Vec<Device>) + Send + Sync>,
}

impl Discovery {
    /// Builds the service. A multicast socket that cannot be bound is not
    /// fatal: the subnet scan still works, which is the point of having two
    /// discovery paths.
    ///
    /// Async because binding the socket registers it with the tokio reactor,
    /// which panics outside a runtime context.
    pub async fn new(
        info: DeviceInfo,
        cert_pem: &str,
        key_pem: &str,
        notify: Arc<dyn Fn(Vec<Device>) + Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        let registry = Arc::new(Registry::new(info.fingerprint.clone()));
        let client = build_client(cert_pem, key_pem)?;
        let socket = match bind_multicast_socket(DEFAULT_PORT) {
            Ok(socket) => Some(Arc::new(socket)),
            Err(e) => {
                eprintln!("multicast unavailable ({e}); falling back to subnet scan only");
                None
            }
        };
        Ok(Arc::new(Discovery {
            registry,
            info,
            client,
            socket,
            notify,
        }))
    }

    pub fn multicast_available(&self) -> bool {
        self.socket.is_some()
    }

    /// Spawns the listener and the announce, prune and refresh loops.
    pub fn start(self: &Arc<Self>) {
        if let Some(socket) = self.socket.clone() {
            let this = Arc::clone(self);
            tauri::async_runtime::spawn(async move { this.listen(socket).await });

            let this = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                this.announce_burst().await;
                let mut ticker = tokio::time::interval(ANNOUNCE_INTERVAL);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    this.announce_once().await;
                }
            });
        }

        let this = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(PRUNE_INTERVAL);
            loop {
                ticker.tick().await;
                if this.registry.prune(now_ms(), DEVICE_TIMEOUT) {
                    this.emit();
                }
            }
        });

        let this = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let mut ticker = tokio::time::interval(REFRESH_INTERVAL);
            ticker.tick().await;
            loop {
                ticker.tick().await;
                this.refresh_known().await;
            }
        });

        // A scan on startup finds peers even where multicast is blocked.
        let this = Arc::clone(self);
        tauri::async_runtime::spawn(async move { this.scan_subnet().await });
    }

    /// Records a peer that contacted us, e.g. one that answered our announce
    /// by calling `/register` on our own server.
    pub fn register_peer(&self, info: DeviceInfo, ip: IpAddr) {
        let IpAddr::V4(ip) = ip else { return };
        if self
            .registry
            .upsert(Device::from_announce(&info, ip, now_ms()))
        {
            self.emit();
        }
    }

    fn emit(&self) {
        let devices = self.registry.list();
        let summary: Vec<&str> = devices.iter().map(|d| d.alias.as_str()).collect();
        eprintln!("devices ({}): {}", devices.len(), summary.join(", "));
        (self.notify)(devices);
    }

    fn announce_payload(&self, announce: bool) -> Vec<u8> {
        serde_json::to_vec(&AnnounceMessage {
            device: self.info.clone(),
            announce,
        })
        .unwrap_or_default()
    }

    async fn announce_once(&self) {
        let Some(socket) = &self.socket else { return };
        let target = SocketAddrV4::new(MULTICAST_GROUP, DEFAULT_PORT);
        if let Err(e) = socket.send_to(&self.announce_payload(true), target).await {
            eprintln!("announce failed: {e}");
        }
    }

    async fn announce_burst(&self) {
        for delay in ANNOUNCE_BURST_DELAYS {
            tokio::time::sleep(delay).await;
            self.announce_once().await;
        }
    }

    async fn listen(self: Arc<Self>, socket: Arc<UdpSocket>) {
        let mut buffer = vec![0u8; RECEIVE_BUFFER_SIZE];
        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((len, from)) => {
                    let this = Arc::clone(&self);
                    let bytes = buffer[..len].to_vec();
                    tauri::async_runtime::spawn(async move {
                        this.handle_datagram(from, &bytes).await;
                    });
                }
                Err(e) => {
                    eprintln!("multicast receive failed: {e}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    async fn handle_datagram(&self, from: SocketAddr, bytes: &[u8]) {
        let IpAddr::V4(ip) = from.ip() else { return };
        let Ok(message) = serde_json::from_slice::<AnnounceMessage>(bytes) else {
            return;
        };
        if message.device.fingerprint == self.info.fingerprint {
            return; // our own announce, looped back
        }

        if self
            .registry
            .upsert(Device::from_announce(&message.device, ip, now_ms()))
        {
            self.emit();
        }

        // Only `announce: true` asks for an answer. Answering an answer would
        // bounce forever.
        if message.announce {
            let port = message.device.port;
            let protocol = message.device.protocol;
            if self.register_with(ip, port, protocol).await.is_none() {
                self.send_udp_answer(SocketAddr::new(IpAddr::V4(ip), from.port()))
                    .await;
            }
        }
    }

    /// UDP fallback for peers whose HTTP server did not answer. `announce` is
    /// false so the peer does not answer back.
    async fn send_udp_answer(&self, target: SocketAddr) {
        let Some(socket) = &self.socket else { return };
        if let Err(e) = socket.send_to(&self.announce_payload(false), target).await {
            eprintln!("udp answer to {target} failed: {e}");
        }
    }

    /// Sends `/register` and stores whatever answers. `None` means no usable
    /// answer: unreachable, malformed, or our own fingerprint coming back.
    async fn register_with(
        &self,
        ip: Ipv4Addr,
        port: u16,
        protocol: ProtocolType,
    ) -> Option<Device> {
        let url = format!(
            "{}://{}:{}{}",
            protocol.as_str(),
            ip,
            port,
            REGISTER_PATH
        );
        let response = self.client.post(&url).json(&self.info).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = response.json::<RegisterResponse>().await.ok()?;
        if body.fingerprint == self.info.fingerprint {
            return None;
        }
        let device = Device::from_register_response(body, ip, port, protocol, now_ms());
        if self.registry.upsert(device.clone()) {
            self.emit();
        }
        Some(device)
    }

    /// Re-probes known devices so a peer that stays quiet does not age out.
    async fn refresh_known(self: &Arc<Self>) {
        let known = self.registry.list();
        let mut tasks = tokio::task::JoinSet::new();
        for device in known {
            let Ok(ip) = device.ip.parse::<Ipv4Addr>() else {
                continue;
            };
            let this = Arc::clone(self);
            tasks.spawn(async move {
                this.register_with(ip, device.port, device.protocol).await;
            });
        }
        while tasks.join_next().await.is_some() {}
    }

    /// Probes `/register` on every host of the local /24.
    ///
    /// HTTPS only: that is what the official app uses by default, and trying
    /// both protocols would double an already large fan-out.
    pub async fn scan_subnet(self: &Arc<Self>) {
        let Some(local) = local_ipv4() else {
            eprintln!("no local IPv4 address; skipping subnet scan");
            return;
        };
        let permits = Arc::new(tokio::sync::Semaphore::new(SCAN_CONCURRENCY));
        let mut tasks = tokio::task::JoinSet::new();
        for host in subnet_hosts(local) {
            let this = Arc::clone(self);
            let permits = Arc::clone(&permits);
            tasks.spawn(async move {
                let _permit = permits.acquire_owned().await.ok()?;
                this.register_with(host, DEFAULT_PORT, ProtocolType::Https)
                    .await
            });
        }
        while tasks.join_next().await.is_some() {}
    }

    /// Announce burst plus a subnet scan, for the user-triggered rescan.
    pub async fn rescan(self: &Arc<Self>) {
        let announce = {
            let this = Arc::clone(self);
            tauri::async_runtime::spawn(async move { this.announce_burst().await })
        };
        self.scan_subnet().await;
        let _ = announce.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(fingerprint: &str, alias: &str, last_seen: u64) -> Device {
        Device {
            fingerprint: fingerprint.into(),
            alias: alias.into(),
            device_model: Some("Windows".into()),
            device_type: DeviceType::Desktop,
            ip: "192.168.1.5".into(),
            port: DEFAULT_PORT,
            protocol: ProtocolType::Https,
            download: false,
            last_seen,
        }
    }

    #[test]
    fn new_device_changes_the_list() {
        let registry = Registry::new("SELF".into());
        assert!(registry.upsert(device("A", "Alpha", 1000)));
        assert_eq!(registry.list().len(), 1);
    }

    #[test]
    fn pure_refresh_does_not_change_the_list() {
        let registry = Registry::new("SELF".into());
        registry.upsert(device("A", "Alpha", 1000));
        assert!(!registry.upsert(device("A", "Alpha", 2000)));
        assert_eq!(registry.list()[0].last_seen, 2000);
    }

    #[test]
    fn changed_alias_or_address_changes_the_list() {
        let registry = Registry::new("SELF".into());
        registry.upsert(device("A", "Alpha", 1000));
        assert!(registry.upsert(device("A", "Renamed", 1000)));
        let mut moved = device("A", "Renamed", 1000);
        moved.ip = "192.168.1.9".into();
        assert!(registry.upsert(moved));
    }

    #[test]
    fn own_fingerprint_is_ignored() {
        let registry = Registry::new("SELF".into());
        assert!(!registry.upsert(device("SELF", "Me", 1000)));
        assert!(registry.list().is_empty());
    }

    #[test]
    fn empty_fingerprint_is_ignored() {
        let registry = Registry::new("SELF".into());
        assert!(!registry.upsert(device("", "Nameless", 1000)));
        assert!(registry.list().is_empty());
    }

    #[test]
    fn stale_devices_are_pruned_after_the_timeout() {
        let registry = Registry::new("SELF".into());
        registry.upsert(device("A", "Alpha", 0));
        registry.upsert(device("B", "Bravo", 100_000));
        assert!(registry.prune(120_000, DEVICE_TIMEOUT));
        let list = registry.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].fingerprint, "B");
        assert!(!registry.prune(120_000, DEVICE_TIMEOUT));
    }

    #[test]
    fn list_is_sorted_by_alias_then_fingerprint() {
        let registry = Registry::new("SELF".into());
        registry.upsert(device("C", "charlie", 1));
        registry.upsert(device("A", "Alpha", 1));
        registry.upsert(device("B", "bravo", 1));
        let aliases: Vec<String> = registry.list().into_iter().map(|d| d.alias).collect();
        assert_eq!(aliases, vec!["Alpha", "bravo", "charlie"]);
    }

    #[test]
    fn subnet_covers_the_24_without_network_broadcast_or_self() {
        let hosts = subnet_hosts(Ipv4Addr::new(192, 168, 1, 42));
        assert_eq!(hosts.len(), 253);
        assert!(hosts.contains(&Ipv4Addr::new(192, 168, 1, 1)));
        assert!(hosts.contains(&Ipv4Addr::new(192, 168, 1, 254)));
        assert!(!hosts.contains(&Ipv4Addr::new(192, 168, 1, 0)));
        assert!(!hosts.contains(&Ipv4Addr::new(192, 168, 1, 255)));
        assert!(!hosts.contains(&Ipv4Addr::new(192, 168, 1, 42)));
    }

    #[test]
    fn announce_device_uses_the_datagram_source_address() {
        let info: DeviceInfo = serde_json::from_str(
            r#"{"alias":"Secret Banana","version":"2.2","deviceModel":"Windows",
                "deviceType":"desktop","fingerprint":"F","port":53317,"protocol":"https"}"#,
        )
        .unwrap();
        let device = Device::from_announce(&info, Ipv4Addr::new(10, 0, 0, 7), 42);
        assert_eq!(device.ip, "10.0.0.7");
        assert_eq!(device.port, 53317);
        assert_eq!(device.last_seen, 42);
    }

    #[test]
    fn register_response_falls_back_to_the_probed_address() {
        let res: RegisterResponse =
            serde_json::from_str(r#"{"alias":"A","fingerprint":"F"}"#).unwrap();
        let device = Device::from_register_response(
            res,
            Ipv4Addr::new(10, 0, 0, 8),
            53317,
            ProtocolType::Https,
            7,
        );
        assert_eq!(device.port, 53317);
        assert_eq!(device.protocol, ProtocolType::Https);
        assert_eq!(device.device_type, DeviceType::Desktop);
    }
}
