//! App side of phone access (specs/remote.md §2, §4): the palette starts and stops
//! the server, the agent poll publishes the live panes to it.

use super::*;
use crate::remote::launch::{LaunchTarget, LaunchTargets, LaunchedPane, Launcher};
use crate::remote::qr::QrMatrix;
use crate::remote::registry::{ExposedPane, Registry};
use crate::remote::server::{PhoneServer, StartError};
use crate::theme::Palette;
use crate::ui::phone_access_modal::{phone_access_modal, PhoneAccessView};

/// While the device count may move, the open modal repaints at this pace.
const MODAL_REFRESH: std::time::Duration = std::time::Duration::from_secs(1);

pub(super) struct PhoneAccess {
    server: PhoneServer,
    qr: Option<QrMatrix>,
    registry: Registry,
    launches: crossbeam_channel::Receiver<LaunchedPane>,
}

impl HelmApp {
    pub(super) fn is_phone_access_on(&self) -> bool {
        self.phone.is_some()
    }

    /// *Open on phone*: starts access when off, then shows the pairing modal.
    pub(super) fn open_on_phone(&mut self, ctx: &egui::Context, now: f64) {
        if self.phone.is_none() {
            let registry = Registry::default();
            let (launcher, launches) = Launcher::channel(repaint_pacer(ctx));
            let server = match PhoneServer::start(registry.clone(), launcher) {
                Ok(server) => server,
                Err(StartError::NoLocalNetwork) => {
                    self.toasts
                        .error("Phone access needs a local network (Wi-Fi)", now);
                    return;
                }
                Err(StartError::Bind(err)) => {
                    self.toasts
                        .error(format!("Phone access failed — {err}"), now);
                    return;
                }
            };
            self.adopt_phone_server(server, registry, launches);
        }
        self.modal = Some(Modal::PhoneAccess);
    }

    /// `registry` and `launches` are the ones the server was started with.
    pub(super) fn adopt_phone_server(
        &mut self,
        server: PhoneServer,
        registry: Registry,
        launches: crossbeam_channel::Receiver<LaunchedPane>,
    ) {
        self.phone = Some(PhoneAccess {
            qr: QrMatrix::encode(server.pairing_url()),
            server,
            registry,
            launches,
        });
        self.publish_phone_panes();
    }

    pub(super) fn stop_phone_access(&mut self) {
        self.phone = None;
        if matches!(self.modal, Some(Modal::PhoneAccess)) {
            self.modal = None;
        }
    }

    /// At the agent poll: republish the live panes, or take note of a stop the
    /// server decided on its own (idle delay, LAN address gone).
    pub(super) fn sync_phone_access(&mut self, now: f64) {
        let Some(phone) = &self.phone else {
            return;
        };
        if !phone.server.is_running() {
            self.stop_phone_access();
            self.toasts.success("Phone access stopped", now);
            return;
        }
        self.publish_phone_panes();
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
            for state in tab_panes.values() {
                let TerminalState::Live(pane) = state else {
                    continue;
                };
                panes.push(ExposedPane {
                    uid: pane.uid(),
                    project: self.workspace.project_name(index).unwrap_or_default(),
                    branch: self.caches.branch_labels.get(&key.0).cloned(),
                    tab: self
                        .workspace
                        .tab_label(key.1)
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
            .launch_agents
            .iter()
            .filter(|agent| agent.is_offered())
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
        let Some(phone) = &self.phone else {
            self.modal = None;
            return;
        };
        ctx.request_repaint_after(MODAL_REFRESH);
        let pairing_url = phone.server.pairing_url().to_owned();
        let view = PhoneAccessView {
            pairing_url: &pairing_url,
            qr: phone.qr.as_ref(),
            clients: phone.server.clients(),
        };
        let action = phone_access_modal(ui, palette, &view);
        if action.copy_url {
            ctx.copy_text(pairing_url);
            self.toasts.success("Link copied", ctx.input(|i| i.time));
        }
        if action.stop {
            self.stop_phone_access();
        } else if action.dismiss {
            self.modal = None;
        }
    }
}
