use super::definition::{Step, StepKind, STEP_DONE_MARKER};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Running,
    AwaitingReply,
    NeedsYou,
    Gate,
    Done,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// Claude's `Stop` hook: `turn` is the hook's `prompt_id`, shared by every hook
    /// of one turn.
    TurnEnded {
        turn: String,
        last_message: Option<String>,
    },
    PermissionPrompt,
    /// `/compact` or `/clear` finished: Claude's `SessionStart` hook.
    SessionReset,
    Continue,
    NextStep,
    StopRun,
    ProcessExited,
}

/// What the runner must do after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    SendPrompt(String),
    NotifyNeedsYou,
    NotifyFinished,
}

/// One launched workflow (workflows.md §5): its rendered steps and where it stands.
#[derive(Debug, Clone)]
pub struct WorkflowRun {
    steps: Vec<Step>,
    current: usize,
    state: RunState,
    advanced_turn: Option<String>,
}

impl WorkflowRun {
    /// `steps` come from `Workflow::render`, non-empty (validation refuses an empty
    /// workflow).
    pub fn start(steps: Vec<Step>) -> Self {
        let state = state_on(&steps[0]);
        Self {
            steps,
            current: 0,
            state,
            advanced_turn: None,
        }
    }

    /// Step 1 travels as Claude's initial prompt, not as a paste (workflows.md §6).
    pub fn initial_prompt(&self) -> Option<&str> {
        match &self.steps[0].kind {
            StepKind::Prompt { text } => Some(text),
            StepKind::Gate { .. } => None,
        }
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    pub fn step_index(&self) -> usize {
        self.current
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    pub fn current_step(&self) -> &Step {
        &self.steps[self.current]
    }

    pub fn is_over(&self) -> bool {
        matches!(self.state, RunState::Done | RunState::Stopped)
    }

    pub fn apply(&mut self, event: RunEvent) -> Option<Effect> {
        if self.is_over() {
            return None;
        }
        match event {
            RunEvent::StopRun | RunEvent::ProcessExited => {
                self.state = RunState::Stopped;
                None
            }
            RunEvent::TurnEnded { turn, last_message } => self.turn_ended(turn, last_message),
            RunEvent::PermissionPrompt if self.is_in_prompt_step() => {
                self.state = RunState::NeedsYou;
                Some(Effect::NotifyNeedsYou)
            }
            RunEvent::SessionReset
                if self.is_in_prompt_step() && self.current_step().kind.is_session_reset() =>
            {
                self.advance()
            }
            RunEvent::NextStep if self.is_waiting_on_user() => self.advance(),
            RunEvent::Continue if self.state == RunState::Gate => self.advance(),
            RunEvent::PermissionPrompt
            | RunEvent::SessionReset
            | RunEvent::NextStep
            | RunEvent::Continue => None,
        }
    }

    fn turn_ended(&mut self, turn: String, last_message: Option<String>) -> Option<Effect> {
        let is_duplicate = self.advanced_turn.as_deref() == Some(turn.as_str());
        if !self.is_in_prompt_step() || is_duplicate {
            return None;
        }
        let has_marker = last_message.is_some_and(|m| m.contains(STEP_DONE_MARKER));
        if !has_marker {
            self.state = RunState::AwaitingReply;
            return None;
        }
        self.advanced_turn = Some(turn);
        self.advance()
    }

    fn is_in_prompt_step(&self) -> bool {
        matches!(
            self.state,
            RunState::Running | RunState::AwaitingReply | RunState::NeedsYou
        )
    }

    fn is_waiting_on_user(&self) -> bool {
        matches!(self.state, RunState::AwaitingReply | RunState::NeedsYou)
    }

    fn advance(&mut self) -> Option<Effect> {
        if self.current + 1 == self.steps.len() {
            self.state = RunState::Done;
            return Some(Effect::NotifyFinished);
        }
        self.current += 1;
        let step = &self.steps[self.current];
        self.state = state_on(step);
        match &step.kind {
            StepKind::Prompt { text } => Some(Effect::SendPrompt(text.clone())),
            StepKind::Gate { .. } => None,
        }
    }
}

fn state_on(step: &Step) -> RunState {
    match step.kind {
        StepKind::Prompt { .. } => RunState::Running,
        StepKind::Gate { .. } => RunState::Gate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(text: &str) -> Step {
        Step {
            title: text.to_owned(),
            kind: StepKind::Prompt {
                text: text.to_owned(),
            },
        }
    }

    fn gate() -> Step {
        Step {
            title: "Read".to_owned(),
            kind: StepKind::Gate {
                message: "Check".to_owned(),
            },
        }
    }

    fn ended(turn: &str, message: &str) -> RunEvent {
        RunEvent::TurnEnded {
            turn: turn.to_owned(),
            last_message: Some(message.to_owned()),
        }
    }

    fn done(turn: &str) -> RunEvent {
        ended(turn, "All good.\n\n[[STEP_DONE]]")
    }

    fn dev_gate_review() -> WorkflowRun {
        WorkflowRun::start(vec![prompt("dev"), gate(), prompt("review")])
    }

    #[test]
    fn a_marked_turn_moves_through_gate_to_the_next_prompt_then_done() {
        let mut run = dev_gate_review();
        assert_eq!(run.initial_prompt(), Some("dev"));
        assert_eq!(run.apply(done("t1")), None);
        assert_eq!((run.state(), run.step_index()), (RunState::Gate, 1));
        assert_eq!(
            run.apply(RunEvent::Continue),
            Some(Effect::SendPrompt("review".to_owned()))
        );
        assert_eq!(run.state(), RunState::Running);
        assert_eq!(run.apply(done("t2")), Some(Effect::NotifyFinished));
        assert_eq!(run.state(), RunState::Done);
    }

    #[test]
    fn a_turn_without_marker_waits_for_the_user_then_a_marked_turn_advances() {
        let mut run = WorkflowRun::start(vec![prompt("dev"), prompt("review")]);
        run.apply(ended("t1", "Which colour?"));
        assert_eq!(run.state(), RunState::AwaitingReply);
        let missing = RunEvent::TurnEnded {
            turn: "t2".to_owned(),
            last_message: None,
        };
        run.apply(missing);
        assert_eq!(run.state(), RunState::AwaitingReply);
        assert_eq!(
            run.apply(done("t3")),
            Some(Effect::SendPrompt("review".to_owned()))
        );
    }

    #[test]
    fn a_duplicate_stop_of_the_advancing_turn_advances_once() {
        let mut run = WorkflowRun::start(vec![prompt("a"), prompt("b"), prompt("c")]);
        run.apply(done("t1"));
        assert_eq!(run.apply(done("t1")), None);
        assert_eq!(run.step_index(), 1);
    }

    #[test]
    fn a_session_reset_ends_only_a_compact_or_clear_step() {
        let mut run = WorkflowRun::start(vec![prompt("dev"), prompt("/compact"), prompt("review")]);
        assert_eq!(
            run.apply(RunEvent::SessionReset),
            None,
            "an auto-compact mid-turn"
        );
        assert_eq!(run.step_index(), 0);
        run.apply(done("t1"));
        assert_eq!(
            run.apply(RunEvent::SessionReset),
            Some(Effect::SendPrompt("review".to_owned()))
        );
    }

    #[test]
    fn a_permission_prompt_needs_you_once_and_the_next_turn_clears_it() {
        let mut run = WorkflowRun::start(vec![prompt("a"), prompt("b")]);
        assert_eq!(
            run.apply(RunEvent::PermissionPrompt),
            Some(Effect::NotifyNeedsYou)
        );
        assert_eq!(run.state(), RunState::NeedsYou);
        run.apply(done("t1"));
        assert_eq!((run.state(), run.step_index()), (RunState::Running, 1));
    }

    #[test]
    fn next_step_forces_only_a_run_waiting_on_the_user() {
        let mut run = WorkflowRun::start(vec![prompt("a"), prompt("b")]);
        assert_eq!(run.apply(RunEvent::NextStep), None);
        assert_eq!(run.step_index(), 0);
        run.apply(ended("t1", "Done, no marker"));
        assert_eq!(
            run.apply(RunEvent::NextStep),
            Some(Effect::SendPrompt("b".to_owned()))
        );
    }

    #[test]
    fn a_gate_ignores_claude_hooks_and_next_step() {
        let mut run = dev_gate_review();
        run.apply(done("t1"));
        for event in [done("t2"), RunEvent::PermissionPrompt, RunEvent::NextStep] {
            assert_eq!(run.apply(event), None);
            assert_eq!(run.state(), RunState::Gate);
        }
    }

    #[test]
    fn a_workflow_opening_on_a_gate_launches_claude_without_a_prompt() {
        let mut run = WorkflowRun::start(vec![gate(), prompt("dev")]);
        assert_eq!((run.initial_prompt(), run.state()), (None, RunState::Gate));
        assert_eq!(
            run.apply(RunEvent::Continue),
            Some(Effect::SendPrompt("dev".to_owned()))
        );
    }

    #[test]
    fn stop_or_exit_ends_the_run_and_nothing_revives_it() {
        for end in [RunEvent::StopRun, RunEvent::ProcessExited] {
            let mut run = dev_gate_review();
            run.apply(end);
            assert_eq!(run.state(), RunState::Stopped);
            assert_eq!(run.apply(done("t1")), None);
            assert_eq!(run.state(), RunState::Stopped);
        }
    }
}
