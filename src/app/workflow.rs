//! App side of workflows (specs/workflows.md): launching a run in a new tab of the
//! active worktree, and keeping each run in step with its tab and Claude process.

use std::collections::BTreeMap;

use super::*;
use crate::ui::workflow_strip::{StripAction, StripView};
use crate::workflow::definition::StepKind;
use crate::workflow::definition::{LaunchContext, Workflow};
use crate::workflow::hooks::RUN_ENV;
use crate::workflow::run::{RunEvent, RunState};
use crate::workflow::runner::{RunOrigin, RunnerConfig, WorkflowRunner};

const SETTINGS_DIR: &str = "workflows";

/// A workflow tab: its run, the pane Claude was spawned in, the workflow's name.
pub(super) struct WorkflowTab {
    pub(super) run: String,
    pub(super) pane: PaneId,
    pub(super) name: String,
}

/// Runner of a headless app (tests): no repaint to request.
pub(super) fn default_runner() -> WorkflowRunner {
    WorkflowRunner::new(runner_config(), || {})
}

/// The windowed app's runner: a run changed by a hook repaints the strip.
pub(super) fn windowed_runner(ctx: &egui::Context) -> WorkflowRunner {
    let ctx = ctx.clone();
    WorkflowRunner::new(runner_config(), move || ctx.request_repaint())
}

fn runner_config() -> RunnerConfig {
    RunnerConfig {
        dir: crate::persistence::support_file(SETTINGS_DIR)
            .unwrap_or_else(|| std::env::temp_dir().join("helm-workflows")),
        helm_exe: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("helm")),
        claude: "claude".to_owned(),
    }
}

/// Free, not a method: the page render holds field borrows of `HelmApp` here.
pub(super) fn workflow_strip_view(
    tabs: &HashMap<PaneKey, WorkflowTab>,
    runner: &WorkflowRunner,
    key: &PaneKey,
) -> Option<StripView> {
    let tab = tabs.get(key)?;
    let run = runner.snapshot(&tab.run)?;
    let step = run.current_step();
    let gate_message = match (&step.kind, run.state()) {
        (StepKind::Gate { message }, RunState::Gate) => Some(message.clone()),
        _ => None,
    };
    Some(StripView {
        name: tab.name.clone(),
        step: run.step_index() + 1,
        steps: run.step_count(),
        title: step.title.clone(),
        state: run.state(),
        gate_message,
    })
}

impl HelmApp {
    /// The palette row of workflow `index`: straight to the run when it has no
    /// input, else its inputs modal.
    pub(super) fn open_workflow_launch(&mut self, index: usize, ctx: &egui::Context) {
        let Some(workflow) = self.prefs.workflows.get(index).cloned() else {
            return;
        };
        if workflow.inputs.is_empty() {
            self.launch_workflow(&workflow, &BTreeMap::new(), ctx);
        } else {
            self.modal = Some(Modal::LaunchWorkflow(LaunchForm::new(workflow)));
        }
    }

    pub(super) fn launch_workflow(
        &mut self,
        workflow: &Workflow,
        values: &BTreeMap<String, String>,
        ctx: &egui::Context,
    ) {
        let Some(index) = self.workspace.active() else {
            return;
        };
        let (Some(cwd), Some(key)) = (
            self.workspace.repo(index).map(|r| r.path.clone()),
            self.caches.keys.get(index).cloned(),
        ) else {
            return;
        };
        let repo = self.workspace.project_name(index).unwrap_or_default();
        let branch = self
            .caches
            .branch_labels
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let context = LaunchContext {
            repo: &repo,
            branch: &branch,
            worktree: &cwd,
        };
        let steps = workflow.render(values, &context);
        let origin = RunOrigin {
            name: workflow.name.clone(),
            repo,
            branch,
        };
        let Some(tab) = self.workspace.add_tab() else {
            return;
        };
        let (Some(tab_id), Some(pane_id)) = (
            self.workspace.tab_id(index, tab),
            self.workspace.active_layout().map(|l| l.focus()),
        ) else {
            return;
        };
        self.workspace.rename_tab(tab, &workflow.name);
        let pacer = repaint_pacer(ctx);
        let started = self.workflows.start(steps, origin, |launch| {
            let shell = crate::terminal::pty::shell_program();
            let mut cmd = crate::terminal::pty::run_command(shell, &cwd, launch.command);
            cmd.env(RUN_ENV, launch.run_id);
            let pane = Pane::from_command(cmd, INITIAL_ROWS, INITIAL_COLS, pacer)?;
            let handle = pane.handle();
            Ok((pane, handle))
        });
        let state = match started {
            Ok((run, pane)) => {
                let entry = WorkflowTab {
                    run,
                    pane: pane_id,
                    name: workflow.name.clone(),
                };
                self.workflow_tabs.insert((key.clone(), tab_id), entry);
                TerminalState::Live(Box::new(pane))
            }
            Err(err) => TerminalState::Failed(err.to_string()),
        };
        self.caches
            .panes
            .entry((key, tab_id))
            .or_default()
            .insert(pane_id, state);
        self.central_mode = CentralMode::Terminal;
    }

    /// Each frame: a closed tab (or Claude's pane) forgets its run, a Claude that
    /// exited stops it (workflows.md §8).
    pub(super) fn reconcile_workflow_runs(&mut self) {
        self.workflows.set_alerts(self.notify_on_agent_completion);
        let workflows = &self.workflows;
        let panes = &mut self.caches.panes;
        self.workflow_tabs.retain(|key, tab| {
            let Some(state) = panes.get_mut(key).and_then(|p| p.get_mut(&tab.pane)) else {
                workflows.forget(&tab.run);
                return false;
            };
            let exited = match state {
                TerminalState::Live(pane) => pane.exit_code().is_some(),
                TerminalState::Failed(_) => true,
            };
            if exited {
                workflows.apply(&tab.run, RunEvent::ProcessExited);
            }
            true
        });
    }

    /// Stop ends the run, not Claude; Dismiss drops the finished run's strip.
    pub(super) fn apply_workflow_strip(&mut self, key: &PaneKey, action: StripAction) {
        let Some(tab) = self.workflow_tabs.get(key) else {
            return;
        };
        let event = match action {
            StripAction::Continue => RunEvent::Continue,
            StripAction::NextStep => RunEvent::NextStep,
            StripAction::Stop => RunEvent::StopRun,
            StripAction::Dismiss => {
                self.workflows.forget(&tab.run);
                self.workflow_tabs.remove(key);
                return;
            }
        };
        self.workflows.apply(&tab.run, event);
    }

    /// The active tab's workflow state, for the palette's workflow commands.
    pub(super) fn active_workflow_state(&self) -> Option<RunState> {
        let key = self.active_pane_key()?;
        let tab = self.workflow_tabs.get(&key)?;
        Some(self.workflows.snapshot(&tab.run)?.state())
    }

    pub(super) fn apply_active_workflow(&mut self, action: StripAction) {
        if let Some(key) = self.active_pane_key() {
            self.apply_workflow_strip(&key, action);
        }
    }
}
