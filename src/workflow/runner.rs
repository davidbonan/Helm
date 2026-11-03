//! Live runs, shared by the UI thread (launch, strip buttons) and the control
//! socket thread (hook events): a hidden helm draws no frame, yet its runs must
//! advance (workflows.md §6).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::definition::Step;
use super::hooks::{self, Hook};
use super::run::{Effect, RunEvent, WorkflowRun};
use crate::terminal::pane::PaneHandle;

/// Claude runs its `Stop` hook synchronously: the paste waits for the hook to have
/// returned and the prompt to be back.
const PASTE_DELAY: Duration = Duration::from_millis(1000);
/// Between the paste and the `\r` that submits it, as measured on the TUI (WF0).
const SUBMIT_DELAY: Duration = Duration::from_millis(300);

pub struct RunnerConfig {
    /// Where the per-run settings files live (support dir).
    pub dir: PathBuf,
    pub helm_exe: PathBuf,
    /// The Claude Code program, `claude` outside tests.
    pub claude: String,
}

/// Where a run was launched, for its notifications.
#[derive(Debug, Clone)]
pub struct RunOrigin {
    pub name: String,
    pub repo: String,
    pub branch: String,
}

impl RunOrigin {
    fn banner_body(&self) -> String {
        [&self.repo, &self.branch, &self.name]
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// What the caller spawns: `command` through the login shell, with
/// [`hooks::RUN_ENV`] set to `run_id`.
pub struct Launch<'a> {
    pub run_id: &'a str,
    pub command: &'a str,
}

struct LiveRun {
    run: WorkflowRun,
    pane: PaneHandle,
    settings: PathBuf,
    origin: RunOrigin,
}

#[derive(Clone)]
pub struct WorkflowRunner {
    shared: Arc<Shared>,
}

struct Shared {
    config: RunnerConfig,
    runs: Mutex<BTreeMap<String, LiveRun>>,
    next_id: AtomicU64,
    alerts: AtomicBool,
    on_change: Box<dyn Fn() + Send + Sync>,
}

impl WorkflowRunner {
    pub fn new(config: RunnerConfig, on_change: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            shared: Arc::new(Shared {
                config,
                runs: Mutex::new(BTreeMap::new()),
                next_id: AtomicU64::new(1),
                alerts: AtomicBool::new(false),
                on_change: Box::new(on_change),
            }),
        }
    }

    /// Native banners on Needs you / Done (*Completion notifications*).
    pub fn set_alerts(&self, on: bool) {
        self.shared.alerts.store(on, Ordering::Relaxed);
    }

    /// Writes the run's settings file, then lets `spawn` start Claude and hand back
    /// what it built along with the pane's handle.
    pub fn start<T>(
        &self,
        steps: Vec<Step>,
        origin: RunOrigin,
        spawn: impl FnOnce(&Launch) -> anyhow::Result<(T, PaneHandle)>,
    ) -> anyhow::Result<(String, T)> {
        let config = &self.shared.config;
        let run = WorkflowRun::start(steps);
        let id = format!(
            "{}-{}",
            std::process::id(),
            self.shared.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let settings = config.dir.join(format!("{id}.json"));
        fs::create_dir_all(&config.dir)?;
        fs::write(&settings, hooks::settings_json(&config.helm_exe))?;
        let command = hooks::launch_command(&config.claude, &settings, run.initial_prompt());
        let launch = Launch {
            run_id: &id,
            command: &command,
        };
        let (built, pane) = spawn(&launch).inspect_err(|_| {
            let _ = fs::remove_file(&settings);
        })?;
        let live = LiveRun {
            run,
            pane,
            settings,
            origin,
        };
        self.lock_runs().insert(id.clone(), live);
        (self.shared.on_change)();
        Ok((id, built))
    }

    /// From the control socket; an unknown run is ignored (workflows.md §6).
    pub fn deliver(&self, run_id: &str, hook: Hook) {
        self.apply(run_id, hook.into_event());
    }

    pub fn apply(&self, run_id: &str, event: RunEvent) {
        let (outcome, changed) = {
            let mut runs = self.lock_runs();
            let Some(live) = runs.get_mut(run_id) else {
                return;
            };
            let before = (live.run.state(), live.run.step_index());
            let effect = live.run.apply(event);
            if live.run.is_over() {
                let _ = fs::remove_file(&live.settings);
            }
            let changed = before != (live.run.state(), live.run.step_index());
            let outcome = effect.map(|effect| (effect, live.pane.clone(), live.origin.clone()));
            (outcome, changed)
        };
        if let Some((effect, pane, origin)) = outcome {
            self.perform(effect, pane, &origin);
        }
        // Only on a change: the app re-reports a dead process every frame.
        if changed {
            (self.shared.on_change)();
        }
    }

    pub fn snapshot(&self, run_id: &str) -> Option<WorkflowRun> {
        self.lock_runs().get(run_id).map(|live| live.run.clone())
    }

    /// The run's tab closed: the run stops and is dropped.
    pub fn forget(&self, run_id: &str) {
        self.apply(run_id, RunEvent::StopRun);
        self.lock_runs().remove(run_id);
    }

    fn perform(&self, effect: Effect, pane: PaneHandle, origin: &RunOrigin) {
        let alerts = self.shared.alerts.load(Ordering::Relaxed);
        match effect {
            Effect::SendPrompt(text) => {
                std::thread::spawn(move || {
                    std::thread::sleep(PASTE_DELAY);
                    let _ = pane.paste(&text);
                    std::thread::sleep(SUBMIT_DELAY);
                    let _ = pane.feed(b"\r");
                });
            }
            Effect::NotifyNeedsYou if alerts => {
                crate::notify::post("Claude needs you", &origin.banner_body());
            }
            Effect::NotifyFinished if alerts => {
                crate::notify::post("Workflow finished", &origin.banner_body());
            }
            Effect::NotifyNeedsYou | Effect::NotifyFinished => {}
        }
    }

    fn lock_runs(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, LiveRun>> {
        self.shared
            .runs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
