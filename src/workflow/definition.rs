use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::template::{self, BUILTINS};

pub const STEP_DONE_MARKER: &str = "[[STEP_DONE]]";

const MARKER_INSTRUCTION: &str =
    "When this step is complete, end your final message with [[STEP_DONE]].";

/// Built-in slash commands that end no model turn: no `Stop` hook ever closes them.
const TURNLESS_COMMANDS: [&str; 2] = ["/compact", "/clear"];

// Every struct keeps its table-valued field last: TOML emits values before tables.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Workflow {
    pub name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<Input>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default: String,
    pub kind: InputKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum InputKind {
    Text,
    LongText,
    Select { options: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub title: String,
    pub kind: StepKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StepKind {
    Prompt { text: String },
    Gate { message: String },
}

impl StepKind {
    fn text(&self) -> &str {
        match self {
            Self::Prompt { text } => text,
            Self::Gate { message } => message,
        }
    }
}

/// The worktree a run is launched on, source of the built-in variables.
pub struct LaunchContext<'a> {
    pub repo: &'a str,
    pub branch: &'a str,
    pub worktree: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    MissingName,
    NoStep,
    BlankPrompt { step: usize },
    InvalidKey { input: usize },
    ReservedKey { key: String },
    DuplicateKey { key: String },
    SelectWithoutOption { key: String },
    DefaultNotAnOption { key: String },
    UnknownPlaceholder { step: usize, name: String },
    TurnlessCommand { step: usize, command: &'static str },
}

impl Issue {
    /// Only a turnless command is a warning: the workflow still launches.
    pub fn is_error(&self) -> bool {
        !matches!(self, Self::TurnlessCommand { .. })
    }
}

impl Workflow {
    pub fn issues(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        if self.name.trim().is_empty() {
            issues.push(Issue::MissingName);
        }
        issues.extend(self.input_issues());
        if self.steps.is_empty() {
            issues.push(Issue::NoStep);
        }
        issues.extend(self.step_issues());
        issues
    }

    pub fn is_launchable(&self) -> bool {
        self.issues().iter().all(|issue| !issue.is_error())
    }

    /// The launch form's starting values: each default, else a Select's first option.
    pub fn initial_values(&self) -> BTreeMap<String, String> {
        self.inputs
            .iter()
            .map(|input| {
                let value = match &input.kind {
                    InputKind::Select { options } if input.default.is_empty() => {
                        options.first().cloned().unwrap_or_default()
                    }
                    _ => input.default.clone(),
                };
                (input.key.clone(), value)
            })
            .collect()
    }

    /// Every input is required (workflows.md §3).
    pub fn accepts(&self, values: &BTreeMap<String, String>) -> bool {
        self.inputs.iter().all(|input| {
            values
                .get(&input.key)
                .is_some_and(|value| !value.trim().is_empty())
        })
    }

    fn input_issues(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, input) in self.inputs.iter().enumerate() {
            let key = input.key.clone();
            if !template::is_placeholder_name(&key) {
                issues.push(Issue::InvalidKey { input: index });
            } else if BUILTINS.contains(&key.as_str()) {
                issues.push(Issue::ReservedKey { key: key.clone() });
            } else if !seen.insert(key.clone()) {
                issues.push(Issue::DuplicateKey { key: key.clone() });
            }
            if let InputKind::Select { options } = &input.kind {
                if options.is_empty() {
                    issues.push(Issue::SelectWithoutOption { key });
                } else if !input.default.is_empty() && !options.contains(&input.default) {
                    issues.push(Issue::DefaultNotAnOption { key });
                }
            }
        }
        issues
    }

    fn step_issues(&self) -> Vec<Issue> {
        let known: BTreeSet<&str> = self
            .inputs
            .iter()
            .map(|input| input.key.as_str())
            .chain(BUILTINS)
            .collect();
        let mut issues = Vec::new();
        for (step, kind) in self.steps.iter().map(|s| &s.kind).enumerate() {
            if let StepKind::Prompt { text } = kind {
                if text.trim().is_empty() {
                    issues.push(Issue::BlankPrompt { step });
                }
                if let Some(command) = turnless_command(text) {
                    issues.push(Issue::TurnlessCommand { step, command });
                }
            }
            for name in template::placeholders(kind.text()) {
                if !known.contains(name) {
                    let name = name.to_owned();
                    issues.push(Issue::UnknownPlaceholder { step, name });
                }
            }
        }
        issues
    }

    /// The run's steps, fixed at launch: placeholders filled, and each prompt ending
    /// with the instruction that makes Claude write the end marker.
    pub fn render(&self, values: &BTreeMap<String, String>, context: &LaunchContext) -> Vec<Step> {
        let mut vars = values.clone();
        vars.insert("repo".to_owned(), context.repo.to_owned());
        vars.insert("branch".to_owned(), context.branch.to_owned());
        vars.insert(
            "worktree".to_owned(),
            context.worktree.display().to_string(),
        );
        self.steps
            .iter()
            .map(|step| Step {
                title: step.title.clone(),
                kind: match &step.kind {
                    StepKind::Prompt { text } => StepKind::Prompt {
                        text: format!("{}\n\n{MARKER_INSTRUCTION}", template::render(text, &vars)),
                    },
                    StepKind::Gate { message } => StepKind::Gate {
                        message: template::render(message, &vars),
                    },
                },
            })
            .collect()
    }
}

fn turnless_command(prompt: &str) -> Option<&'static str> {
    let first = prompt.split_whitespace().next()?;
    TURNLESS_COMMANDS
        .into_iter()
        .find(|command| *command == first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket_flow() -> Workflow {
        Workflow {
            name: "Ticket".to_owned(),
            inputs: vec![
                Input {
                    key: "ticket".to_owned(),
                    label: "Ticket number".to_owned(),
                    default: String::new(),
                    kind: InputKind::Text,
                },
                Input {
                    key: "depth".to_owned(),
                    label: "Review depth".to_owned(),
                    default: "light".to_owned(),
                    kind: InputKind::Select {
                        options: vec!["light".to_owned(), "deep".to_owned()],
                    },
                },
            ],
            steps: vec![
                prompt("Implement {ticket} on {branch} in {repo}."),
                Step {
                    title: "Read".to_owned(),
                    kind: StepKind::Gate {
                        message: "Check {ticket} before review.".to_owned(),
                    },
                },
                prompt("Review ({depth}) the changes in {worktree}."),
            ],
        }
    }

    fn prompt(text: &str) -> Step {
        Step {
            title: "Step".to_owned(),
            kind: StepKind::Prompt {
                text: text.to_owned(),
            },
        }
    }

    fn with(edit: impl FnOnce(&mut Workflow)) -> Workflow {
        let mut workflow = ticket_flow();
        edit(&mut workflow);
        workflow
    }

    #[test]
    fn a_complete_workflow_has_no_issue() {
        assert_eq!(ticket_flow().issues(), vec![]);
        assert!(ticket_flow().is_launchable());
    }

    #[test]
    fn each_error_is_reported_and_blocks_the_launch() {
        let cases = [
            (with(|w| w.name = " ".to_owned()), Issue::MissingName),
            (with(|w| w.steps.clear()), Issue::NoStep),
            (
                with(|w| w.steps[0] = prompt("  ")),
                Issue::BlankPrompt { step: 0 },
            ),
            (
                with(|w| w.inputs[0].key = "Ticket".to_owned()),
                Issue::InvalidKey { input: 0 },
            ),
            (
                with(|w| w.inputs[1].key = "ticket".to_owned()),
                Issue::DuplicateKey {
                    key: "ticket".to_owned(),
                },
            ),
            (
                with(|w| w.inputs[0].key = "branch".to_owned()),
                Issue::ReservedKey {
                    key: "branch".to_owned(),
                },
            ),
            (
                with(|w| w.inputs[1].kind = InputKind::Select { options: vec![] }),
                Issue::SelectWithoutOption {
                    key: "depth".to_owned(),
                },
            ),
            (
                with(|w| w.inputs[1].default = "medium".to_owned()),
                Issue::DefaultNotAnOption {
                    key: "depth".to_owned(),
                },
            ),
            (
                with(|w| {
                    w.steps[1].kind = StepKind::Gate {
                        message: "{nope}".to_owned(),
                    }
                }),
                Issue::UnknownPlaceholder {
                    step: 1,
                    name: "nope".to_owned(),
                },
            ),
        ];
        for (workflow, expected) in cases {
            assert!(workflow.issues().contains(&expected), "{expected:?}");
            assert!(!workflow.is_launchable(), "{expected:?}");
        }
    }

    #[test]
    fn a_turnless_command_warns_without_blocking() {
        let workflow = with(|w| w.steps.push(prompt("/compact keep the plan")));
        let issue = Issue::TurnlessCommand {
            step: 3,
            command: "/compact",
        };
        assert_eq!(workflow.issues(), vec![issue]);
        assert!(workflow.is_launchable());
        assert!(with(|w| w.steps.push(prompt("/review the diff")))
            .issues()
            .is_empty());
    }

    #[test]
    fn the_form_starts_on_defaults_and_needs_every_input() {
        let workflow = with(|w| w.inputs[1].default = String::new());
        let mut values = workflow.initial_values();
        assert_eq!(values["ticket"], "");
        assert_eq!(
            values["depth"], "light",
            "a Select starts on its first option"
        );
        assert!(!workflow.accepts(&values));
        values.insert("ticket".to_owned(), "BAPS-12".to_owned());
        assert!(workflow.accepts(&values));
    }

    #[test]
    fn render_fills_inputs_and_builtins_and_appends_the_marker_instruction() {
        let values = BTreeMap::from([
            ("ticket".to_owned(), "BAPS-12".to_owned()),
            ("depth".to_owned(), "deep".to_owned()),
        ]);
        let context = LaunchContext {
            repo: "helm",
            branch: "feat/x",
            worktree: Path::new("/w/helm"),
        };
        let kinds: Vec<StepKind> = ticket_flow()
            .render(&values, &context)
            .into_iter()
            .map(|step| step.kind)
            .collect();
        let marker = format!("\n\n{MARKER_INSTRUCTION}");
        assert_eq!(
            kinds,
            vec![
                StepKind::Prompt {
                    text: format!("Implement BAPS-12 on feat/x in helm.{marker}")
                },
                StepKind::Gate {
                    message: "Check BAPS-12 before review.".to_owned()
                },
                StepKind::Prompt {
                    text: format!("Review (deep) the changes in /w/helm.{marker}")
                },
            ]
        );
        assert!(MARKER_INSTRUCTION.contains(STEP_DONE_MARKER));
    }
}
