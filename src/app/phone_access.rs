//! App side of phone access (specs/remote.md §2, §4): the palette starts and stops
//! the server, the agent poll publishes the live panes to it.

use super::*;
use crate::remote::firewall::FirewallBlock;
use crate::remote::launch::{LaunchTarget, LaunchTargets, LaunchedPane, Launcher};
use crate::remote::qr::QrMatrix;
use crate::remote::registry::{ExposedPane, Registry};
use crate::remote::server::{PhoneServer, PhoneServices, StartError};
use crate::theme::Palette;
use crate::ui::phone_access_modal::{phone_access_modal, PhoneAccessView};

/// While the device count may move, the open modal repaints at this pace.
const MODAL_REFRESH: std::time::Duration = std::time::Duration::from_secs(1);

/// How often *Start at launch* looks at the network while access is off.
const NETWORK_CHECK: std::time::Duration = std::time::Duration::from_secs(30);

/// How often the open modal re-reads the firewall, so a fix in Settings clears its banner.
const FIREWALL_CHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// How access gets started, by hand or by *Start at launch* (remote.md §3.4).
pub(super) struct PhoneStarter {
    /// A *Stop phone access* holds *Start at launch* off until the next *Open on phone*.
    held_off: bool,
    last_check: Option<f64>,
    /// The gateway lookup in flight: two subprocesses, kept off the UI thread.
    pending: Option<crossbeam_channel::Receiver<Option<String>>>,
    gateway_mac: fn() -> Option<String>,
    bind: fn(PhoneServices) -> Result<PhoneServer, StartError>,
    firewall: fn() -> Option<FirewallBlock>,
}

impl Default for PhoneStarter {
    fn default() -> Self {
        Self {
            held_off: false,
            last_check: None,
            pending: None,
            gateway_mac: crate::remote::network::current_gateway_mac,
            bind: PhoneServer::start,
            firewall: crate::remote::firewall::helm_blocked,
        }
    }
}

impl PhoneStarter {
    #[cfg(test)]
    pub(super) fn seamed(
        gateway_mac: fn() -> Option<String>,
        bind: fn(PhoneServices) -> Result<PhoneServer, StartError>,
    ) -> Self {
        Self {
            gateway_mac,
            bind,
            ..Self::default()
        }
    }

    /// Starts a gateway lookup every [`NETWORK_CHECK`]; returns its answer on the
    /// frame it lands.
    fn check_network(&mut self, ctx: &egui::Context, now: f64) -> Option<String> {
        if let Some(pending) = &self.pending {
            return match pending.try_recv() {
                Err(crossbeam_channel::TryRecvError::Empty) => None,
                landed => {
                    self.pending = None;
                    landed.ok().flatten()
                }
            };
        }
        if self
            .last_check
            .is_some_and(|at| now - at < NETWORK_CHECK.as_secs_f64())
        {
            return None;
        }
        self.last_check = Some(now);
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let (lookup, repaint) = (self.gateway_mac, ctx.clone());
        let _ = std::thread::Builder::new()
            .name("phone-network".into())
            .spawn(move || {
                let _ = sender.send(lookup());
                repaint.request_repaint();
            });
        self.pending = Some(receiver);
        ctx.request_repaint_after(NETWORK_CHECK);
        None
    }
}

pub(super) struct PhoneAccess {
    server: PhoneServer,
    /// The pairing code the modal shows; none until the modal opens.
    offered: Option<OfferedPairing>,
    registry: Registry,
    launches: crossbeam_channel::Receiver<LaunchedPane>,
    firewall: FirewallWatch,
}

/// The firewall reading the modal shows; each read is two subprocesses, kept off the UI thread.
#[derive(Default)]
struct FirewallWatch {
    block: Option<FirewallBlock>,
    pending: Option<crossbeam_channel::Receiver<Option<FirewallBlock>>>,
    last_check: Option<f64>,
}

impl FirewallWatch {
    fn poll(
        &mut self,
        ctx: &egui::Context,
        now: f64,
        read: fn() -> Option<FirewallBlock>,
    ) -> Option<FirewallBlock> {
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Err(crossbeam_channel::TryRecvError::Empty) => return self.block,
                landed => {
                    self.pending = None;
                    self.block = landed.ok().flatten();
                }
            }
        }
        if self
            .last_check
            .is_none_or(|at| now - at >= FIREWALL_CHECK.as_secs_f64())
        {
            self.last_check = Some(now);
            let (sender, receiver) = crossbeam_channel::bounded(1);
            let repaint = ctx.clone();
            let _ = std::thread::Builder::new()
                .name("phone-firewall".into())
                .spawn(move || {
                    let _ = sender.send(read());
                    repaint.request_repaint();
                });
            self.pending = Some(receiver);
        }
        self.block
    }
}

struct OfferedPairing {
    url: String,
    qr: Option<QrMatrix>,
}

impl PhoneAccess {
    pub(super) fn connected_devices(&self) -> Vec<String> {
        self.server.connected_devices()
    }

    /// A fresh code when none is shown yet, or the shown one was used or expired.
    fn refresh_offered_pairing(&mut self) {
        let shown = self.offered.as_ref().map(|offered| offered.url.as_str());
        if shown.is_some() && self.server.pairing_url().as_deref() == shown {
            return;
        }
        let url = self.server.offer_pairing();
        self.offered = Some(OfferedPairing {
            qr: QrMatrix::encode(&url),
            url,
        });
    }
}

impl HelmApp {
    pub(super) fn is_phone_access_on(&self) -> bool {
        self.phone.is_some()
    }

    /// *Open on phone*: starts access when off, then shows the pairing modal with
    /// a fresh code. Lifts a hold-off of *Start at launch*.
    pub(super) fn open_on_phone(&mut self, ctx: &egui::Context, now: f64) {
        self.phone_starter.held_off = false;
        if self.phone.is_none() {
            if let Err(message) = self.start_phone_access(ctx) {
                self.toasts.error(message, now);
                return;
            }
        }
        if let Some(phone) = &mut self.phone {
            phone.offered = None;
        }
        self.modal = Some(Modal::PhoneAccess);
    }

    /// Binds the server on the LAN; the error says why it could not.
    fn start_phone_access(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let registry = Registry::default();
        let (launcher, launches) = Launcher::channel(repaint_pacer(ctx));
        let services = PhoneServices {
            registry: registry.clone(),
            launcher,
            devices: self.phone_devices.clone(),
            alert: std::sync::Arc::new(|alert| {
                crate::notify::post(&alert.title(), &alert.body());
            }),
        };
        let server = (self.phone_starter.bind)(services).map_err(|err| match err {
            StartError::NoLocalNetwork => "Phone access needs a local network (Wi-Fi)".to_owned(),
            StartError::Bind(err) => format!("Phone access failed — {err}"),
        })?;
        self.adopt_phone_server(server, registry, launches);
        Ok(())
    }

    /// `registry` and `launches` are the ones the server was started with.
    pub(super) fn adopt_phone_server(
        &mut self,
        server: PhoneServer,
        registry: Registry,
        launches: crossbeam_channel::Receiver<LaunchedPane>,
    ) {
        self.phone = Some(PhoneAccess {
            offered: None,
            server,
            registry,
            launches,
            firewall: FirewallWatch::default(),
        });
        self.publish_phone_panes();
    }

    /// The user's *Stop*: *Start at launch* holds off until the next *Open on phone*.
    pub(super) fn stop_phone_access(&mut self) {
        self.phone_starter.held_off = true;
        self.end_phone_access();
    }

    fn end_phone_access(&mut self) {
        self.phone = None;
        if matches!(self.modal, Some(Modal::PhoneAccess)) {
            self.modal = None;
        }
    }

    /// At the agent poll: republish the live panes, take note of a stop the server
    /// decided on its own (LAN address gone), or — access off — start it on a
    /// recorded network.
    pub(super) fn sync_phone_access(&mut self, ctx: &egui::Context, now: f64) {
        let Some(phone) = &self.phone else {
            self.start_phone_access_on_recorded_network(ctx, now);
            return;
        };
        if !phone.server.is_running() {
            self.end_phone_access();
            self.toasts.success("Phone access stopped", now);
            return;
        }
        self.publish_phone_panes();
    }

    /// *Start at launch* (remote.md §3.4): silent — no modal, no toast.
    fn start_phone_access_on_recorded_network(&mut self, ctx: &egui::Context, now: f64) {
        let recorded_any = self.phone_devices.read(|book| !book.networks.is_empty());
        if !self.phone_access_at_launch || self.phone_starter.held_off || !recorded_any {
            return;
        }
        let Some(gateway_mac) = self.phone_starter.check_network(ctx, now) else {
            return;
        };
        if self
            .phone_devices
            .read(|book| book.is_recorded_network(&gateway_mac))
        {
            let _ = self.start_phone_access(ctx);
        }
    }

    fn publish_phone_panes(&self) {
        let Some(phone) = &self.phone else {
            return;
        };
        let mut panes = Vec::new();
        for (key, tab_panes) in &self.caches.panes {
            let Some(index) = self.caches.keys.iter().position(|k| k == &key.0) else {
                continue;
            };
            for (pane_id, state) in tab_panes {
                let TerminalState::Live(pane) = state else {
                    continue;
                };
                panes.push(ExposedPane {
                    uid: pane.uid(),
                    project: self.workspace.project_name(index).unwrap_or_default(),
                    branch: self.caches.branch_labels.get(&key.0).cloned(),
                    tab: self
                        .workspace
                        .pane_label(key.1, *pane_id)
                        .unwrap_or_else(|| "Terminal".to_owned()),
                    handle: pane.handle(),
                });
            }
        }
        phone.registry.publish(
            panes,
            self.agent_watcher.as_ref().map(AgentWatcher::link),
            self.theme_preset,
        );
        phone.registry.publish_targets(self.launch_targets());
    }

    /// Every entry of a visible project, bare roots aside (remote.md §7.2).
    fn launch_targets(&self) -> LaunchTargets {
        let entries = (0..self.workspace.len())
            .filter(|&index| !self.workspace.is_in_hidden_project(index))
            .filter_map(|index| {
                let repo = self.workspace.repo(index).filter(|repo| !repo.bare)?;
                let branch = self
                    .caches
                    .keys
                    .get(index)
                    .and_then(|key| self.caches.branch_labels.get(key))
                    .cloned();
                Some(LaunchTarget::new(
                    repo.path.clone(),
                    self.workspace.project_name(index)?,
                    branch,
                    self.workspace.parent_root(index).is_some(),
                ))
            })
            .collect();
        let agents = self
            .agents
            .iter()
            .filter(|agent| agent.is_offered_on_phone())
            .cloned()
            .collect();
        LaunchTargets { entries, agents }
    }

    /// Panes the phone launched become tabs of their entry, not activated; one
    /// whose entry left the workspace meanwhile is dropped.
    pub(super) fn adopt_launched_panes(&mut self) {
        let Some(phone) = &self.phone else {
            return;
        };
        let launched: Vec<LaunchedPane> = phone.launches.try_iter().collect();
        for LaunchedPane { entry, pane } in launched {
            let index = (0..self.workspace.len())
                .find(|&i| self.workspace.repo(i).is_some_and(|r| r.path == entry));
            let slot = index.and_then(|index| {
                let key = self.caches.keys.get(index)?.clone();
                Some((key, self.workspace.append_tab(index)?))
            });
            match slot {
                Some((key, (tab_id, pane_id))) => {
                    self.caches
                        .panes
                        .entry((key, tab_id))
                        .or_default()
                        .insert(pane_id, TerminalState::Live(Box::new(pane)));
                }
                None => {
                    if let Some(phone) = &self.phone {
                        phone.registry.forget(pane.uid());
                    }
                }
            }
        }
    }

    pub(super) fn render_phone_access_modal(
        &mut self,
        ui: &mut egui::Ui,
        palette: &Palette,
        ctx: &egui::Context,
    ) {
        let Some(phone) = &mut self.phone else {
            self.modal = None;
            return;
        };
        ctx.request_repaint_after(MODAL_REFRESH);
        phone.refresh_offered_pairing();
        let now = ctx.input(|i| i.time);
        let firewall = phone.firewall.poll(ctx, now, self.phone_starter.firewall);
        let Some(offered) = &phone.offered else {
            return;
        };
        let pairing_url = offered.url.clone();
        let view = PhoneAccessView {
            pairing_url: &pairing_url,
            qr: offered.qr.as_ref(),
            clients: phone.server.clients(),
            firewall,
        };
        let action = phone_access_modal(ui, palette, &view);
        if action.copy_url {
            ctx.copy_text(pairing_url);
            self.toasts.success("Link copied", ctx.input(|i| i.time));
        }
        if action.open_firewall_settings {
            crate::remote::firewall::open_settings();
        }
        if action.stop {
            self.stop_phone_access();
        } else if action.dismiss {
            self.modal = None;
        }
    }
}
