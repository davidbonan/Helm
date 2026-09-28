//! Command palette (`Cmd+P`, keybindings.md §1): a curated list of commands, some
//! of which open a screen of their own in the same window — the breadcrumb is the
//! trail of screens entered. Holds the navigation state and the fuzzy ranking;
//! what a command does and which ones apply belong to the app.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    SwitchTo,
    RunningServers,
    RunServer,
    RelaunchServer,
    StopServer,
    StashChanges,
    Stashes,
    Pull,
    Push,
    ForcePush,
    Fetch,
    CheckoutBranch,
    CreateWorktree,
    DeleteWorktree,
    OpenInEditor,
    RevealInFinder,
    CopyPath,
    PullRequests,
    Agents,
    OpenOnPhone,
    StopPhoneAccess,
    ToggleTheme,
    SendFeedback,
    WhatsNew,
}

impl Command {
    /// Listing order on the `Commands` screen.
    pub const ALL: [Self; 24] = [
        Self::SwitchTo,
        Self::RunningServers,
        Self::RunServer,
        Self::RelaunchServer,
        Self::StopServer,
        Self::StashChanges,
        Self::Stashes,
        Self::Pull,
        Self::Push,
        Self::ForcePush,
        Self::Fetch,
        Self::CheckoutBranch,
        Self::CreateWorktree,
        Self::DeleteWorktree,
        Self::OpenInEditor,
        Self::RevealInFinder,
        Self::CopyPath,
        Self::PullRequests,
        Self::Agents,
        Self::OpenOnPhone,
        Self::StopPhoneAccess,
        Self::ToggleTheme,
        Self::SendFeedback,
        Self::WhatsNew,
    ];

    /// Persisted key of the command's usage: stable across renames and reorders.
    pub fn id(self) -> &'static str {
        match self {
            Self::SwitchTo => "switch-to",
            Self::RunningServers => "running-servers",
            Self::RunServer => "run-server",
            Self::RelaunchServer => "relaunch-server",
            Self::StopServer => "stop-server",
            Self::StashChanges => "stash-changes",
            Self::Stashes => "stashes",
            Self::Pull => "pull",
            Self::Push => "push",
            Self::ForcePush => "force-push",
            Self::Fetch => "fetch",
            Self::CheckoutBranch => "checkout-branch",
            Self::CreateWorktree => "create-worktree",
            Self::DeleteWorktree => "delete-worktree",
            Self::OpenInEditor => "open-in-editor",
            Self::RevealInFinder => "reveal-in-finder",
            Self::CopyPath => "copy-path",
            Self::PullRequests => "pull-requests",
            Self::Agents => "agents",
            Self::OpenOnPhone => "open-on-phone",
            Self::StopPhoneAccess => "stop-phone-access",
            Self::ToggleTheme => "toggle-theme",
            Self::SendFeedback => "send-feedback",
            Self::WhatsNew => "whats-new",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::SwitchTo => Screen::SwitchTo.title(),
            Self::RunningServers => Screen::RunningServers.title(),
            Self::RunServer => "Run server",
            Self::RelaunchServer => "Relaunch server",
            Self::StopServer => "Stop server",
            Self::StashChanges => "Stash changes",
            Self::Stashes => Screen::Stashes.title(),
            Self::Pull => "Pull",
            Self::Push => "Push",
            Self::ForcePush => "Force push…",
            Self::Fetch => "Fetch",
            Self::CheckoutBranch => Screen::Branches.title(),
            Self::CreateWorktree => "Create worktree…",
            Self::DeleteWorktree => "Delete worktree from disk",
            Self::OpenInEditor => Screen::Editors.title(),
            Self::RevealInFinder => "Reveal in Finder",
            Self::CopyPath => "Copy path",
            Self::PullRequests => Screen::PullRequests.title(),
            Self::Agents => Screen::Agents.title(),
            Self::OpenOnPhone => "Open on phone",
            Self::StopPhoneAccess => "Stop phone access",
            Self::ToggleTheme => "Toggle light / dark theme",
            Self::SendFeedback => "Send feedback…",
            Self::WhatsNew => "What\u{2019}s new",
        }
    }

    pub fn opened_screen(self) -> Option<Screen> {
        match self {
            Self::SwitchTo => Some(Screen::SwitchTo),
            Self::RunningServers => Some(Screen::RunningServers),
            Self::Stashes => Some(Screen::Stashes),
            Self::CheckoutBranch => Some(Screen::Branches),
            Self::OpenInEditor => Some(Screen::Editors),
            Self::PullRequests => Some(Screen::PullRequests),
            Self::Agents => Some(Screen::Agents),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Commands,
    SwitchTo,
    RunningServers,
    Stashes,
    Branches,
    Editors,
    PullRequests,
    Agents,
}

impl Screen {
    pub fn title(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::SwitchTo => "Switch to",
            Self::RunningServers => "Running servers",
            Self::Stashes => "Stashes",
            Self::Branches => "Checkout branch",
            Self::Editors => "Open in editor",
            Self::PullRequests => "Pull requests",
            Self::Agents => "Agents",
        }
    }

    pub fn query_hint(self) -> &'static str {
        match self {
            Self::Commands => "Type a command…",
            Self::SwitchTo => "Filter by project, branch or folder…",
            Self::RunningServers => "Filter by project, branch, port or command…",
            Self::Stashes => "Filter stashes…",
            Self::Branches => "Filter branches…",
            Self::Editors => "Filter editors…",
            Self::PullRequests => "Filter by title, number, author or branch…",
            Self::Agents => "Filter by agent, project or tab…",
        }
    }

    pub fn empty_text(self) -> &'static str {
        match self {
            Self::Commands => "No command available here.",
            Self::SwitchTo => "No project is open.",
            Self::RunningServers => "No server is running.",
            Self::Stashes => "No stash in this repository.",
            Self::Branches => "No other branch to check out.",
            Self::Editors => "No editor available.",
            Self::PullRequests => "No open pull request.",
            Self::Agents => "No agent is running.",
        }
    }
}

#[derive(Debug, Default)]
pub struct CommandPalette {
    /// Screens entered past `Commands`, innermost last.
    entered: Vec<Entered>,
    pub query: String,
    selected: usize,
}

/// A screen entered past `Commands`, with the filter and row its parent was left on.
#[derive(Debug)]
struct Entered {
    screen: Screen,
    parent_query: String,
    parent_selected: usize,
}

impl CommandPalette {
    pub fn screen(&self) -> Screen {
        self.entered
            .last()
            .map_or(Screen::Commands, |entered| entered.screen)
    }

    pub fn trail(&self) -> impl Iterator<Item = Screen> + '_ {
        std::iter::once(Screen::Commands).chain(self.entered.iter().map(|entered| entered.screen))
    }

    pub fn enter(&mut self, screen: Screen) {
        self.entered.push(Entered {
            screen,
            parent_query: std::mem::take(&mut self.query),
            parent_selected: std::mem::take(&mut self.selected),
        });
    }

    /// Leaves the current screen for its parent, back on the row it was left from;
    /// `false` at `Commands`, where going back closes.
    pub fn back(&mut self) -> bool {
        let Some(left) = self.entered.pop() else {
            return false;
        };
        self.resume(left);
        true
    }

    /// Returns to the `depth`-th screen of the trail (`0` = `Commands`).
    pub fn back_to(&mut self, depth: usize) {
        let left = self.entered.drain(depth..).next();
        if let Some(left) = left {
            self.resume(left);
        }
    }

    fn resume(&mut self, left: Entered) {
        self.query = left.parent_query;
        self.selected = left.parent_selected;
    }

    /// The highlighted position among `len` visible rows, clamped as the list shrinks.
    pub fn selected(&self, len: usize) -> Option<usize> {
        (len > 0).then(|| self.selected.min(len - 1))
    }

    pub fn select(&mut self, position: usize) {
        self.selected = position;
    }

    pub fn select_next(&mut self, len: usize) {
        if let Some(current) = self.selected(len) {
            self.selected = (current + 1).min(len - 1);
        }
    }

    pub fn select_previous(&mut self, len: usize) {
        if let Some(current) = self.selected(len) {
            self.selected = current.saturating_sub(1);
        }
    }
}

const RECENT_LIMIT: usize = 5;
const USAGE_HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 3600.0;

/// How often and how lately each command ran, keyed by `Command::id`: an unknown
/// id (a command since removed) is kept but never listed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandUsage(BTreeMap<String, CommandUse>);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct CommandUse {
    /// Every run counts 1, halving each `USAGE_HALF_LIFE_SECS`; valued as of `at`.
    score: f64,
    /// Unix seconds of the last run.
    at: i64,
}

impl CommandUse {
    fn score_at(self, now: i64) -> f64 {
        let elapsed = now.saturating_sub(self.at).max(0) as f64;
        self.score * 0.5f64.powf(elapsed / USAGE_HALF_LIFE_SECS)
    }
}

impl CommandUsage {
    pub fn record(&mut self, command: Command, now: i64) {
        let past = self
            .0
            .get(command.id())
            .map_or(0.0, |used| used.score_at(now));
        let used = CommandUse {
            score: past + 1.0,
            at: now,
        };
        self.0.insert(command.id().to_owned(), used);
    }

    /// The `available` commands heading the list: the last one run and the most
    /// used of the others, most recently run first.
    pub fn recent(&self, available: &[Command], now: i64) -> Vec<Command> {
        let mut used: Vec<(Command, CommandUse)> = available
            .iter()
            .filter_map(|&command| self.0.get(command.id()).map(|&used| (command, used)))
            .collect();
        used.sort_by_key(|(_, used)| std::cmp::Reverse(used.at));
        let mut others = used.split_off(used.len().min(1));
        others.sort_by(|(_, a), (_, b)| b.score_at(now).total_cmp(&a.score_at(now)));
        others.truncate(RECENT_LIMIT - 1);
        used.extend(others);
        used.sort_by_key(|(_, used)| std::cmp::Reverse(used.at));
        used.into_iter().map(|(command, _)| command).collect()
    }
}

/// One row's fuzzy match: matched char positions per searched field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub score: i32,
    pub positions: Vec<Vec<usize>>,
}

const MATCH_SCORE: i32 = 1;
const CONSECUTIVE_BONUS: i32 = 5;
const WORD_START_BONUS: i32 = 8;
const GAP_PENALTY: i32 = 1;

/// Case-insensitive subsequence match of `query` (whitespace ignored) across
/// `fields` read as one text. Picks the best-scoring start: consecutive runs and
/// word starts rank up, gaps rank down. An empty query matches with no position.
pub fn fuzzy_match(query: &str, fields: &[&str]) -> Option<FuzzyMatch> {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(fold_case)
        .collect();
    let mut hay: Vec<(usize, usize, char)> = Vec::new();
    for (field, text) in fields.iter().enumerate() {
        if field > 0 {
            hay.push((usize::MAX, 0, ' '));
        }
        hay.extend(text.chars().enumerate().map(|(i, c)| (field, i, c)));
    }
    let Some(&first) = needle.first() else {
        return Some(FuzzyMatch {
            score: 0,
            positions: vec![Vec::new(); fields.len()],
        });
    };
    let best = (0..hay.len())
        .filter(|&start| fold_case(hay[start].2) == first)
        .filter_map(|start| match_from(&needle, &hay, start))
        .max_by_key(|(score, _)| *score)?;
    let mut positions = vec![Vec::new(); fields.len()];
    for index in best.1 {
        let (field, char_index, _) = hay[index];
        positions[field].push(char_index);
    }
    Some(FuzzyMatch {
        score: best.0,
        positions,
    })
}

fn match_from(
    needle: &[char],
    hay: &[(usize, usize, char)],
    start: usize,
) -> Option<(i32, Vec<usize>)> {
    let mut matched = Vec::with_capacity(needle.len());
    let mut cursor = start;
    for &wanted in needle {
        let found = (cursor..hay.len()).find(|&i| fold_case(hay[i].2) == wanted)?;
        matched.push(found);
        cursor = found + 1;
    }
    let score = matched
        .iter()
        .enumerate()
        .map(|(n, &index)| {
            let mut score = MATCH_SCORE;
            if is_word_start(hay, index) {
                score += WORD_START_BONUS;
            }
            if n > 0 {
                let gap = index - matched[n - 1] - 1;
                score += if gap == 0 {
                    CONSECUTIVE_BONUS
                } else {
                    -(gap as i32) * GAP_PENALTY
                };
            }
            score
        })
        .sum();
    Some((score, matched))
}

fn is_word_start(hay: &[(usize, usize, char)], index: usize) -> bool {
    let current = hay[index].2;
    let Some(previous) = index.checked_sub(1).map(|i| hay[i].2) else {
        return true;
    };
    !previous.is_alphanumeric() || (previous.is_lowercase() && current.is_uppercase())
}

fn fold_case(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Rows of `candidates` (each a list of searched fields) matching `query`, best
/// first, the shorter text breaking a tie. A blank query keeps the given order.
pub fn rank(query: &str, candidates: &[Vec<&str>]) -> Vec<(usize, FuzzyMatch)> {
    let mut hits: Vec<(usize, FuzzyMatch)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, fields)| fuzzy_match(query, fields).map(|hit| (index, hit)))
        .collect();
    if !query.trim().is_empty() {
        let length =
            |index: usize| -> usize { candidates[index].iter().map(|f| f.chars().count()).sum() };
        hits.sort_by_key(|(index, hit)| (std::cmp::Reverse(hit.score), length(*index)));
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scattered_subsequence_matches_across_case_and_spaces() {
        let hit = fuzzy_match("rSer", &["Running servers"]).unwrap();
        assert_eq!(hit.positions, vec![vec![0, 8, 9, 10]]);
    }

    #[test]
    fn a_query_out_of_order_does_not_match() {
        assert_eq!(fuzzy_match("sr", &["rs"]), None);
    }

    #[test]
    fn positions_are_reported_per_field() {
        let hit = fuzzy_match("api 3001", &["api", "main", ":3001"]).unwrap();
        assert_eq!(hit.positions, vec![vec![0, 1, 2], vec![], vec![1, 2, 3, 4]]);
    }

    #[test]
    fn the_best_start_wins_over_the_leftmost_one() {
        let hit = fuzzy_match("serv", &["some servers"]).unwrap();
        assert_eq!(
            hit.positions,
            vec![vec![5, 6, 7, 8]],
            "the consecutive word-start run beats the `s` of `some`"
        );
    }

    #[test]
    fn ranking_puts_the_tighter_match_first_and_drops_misses() {
        let candidates = vec![vec!["web · feature-runner"], vec!["api"], vec!["run"]];
        let ranked: Vec<usize> = rank("run", &candidates)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        assert_eq!(ranked, vec![2, 0]);
    }

    #[test]
    fn an_empty_query_keeps_every_row_in_order() {
        let candidates = vec![vec!["b"], vec!["a"]];
        let ranked: Vec<usize> = rank("  ", &candidates)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        assert_eq!(ranked, vec![0, 1]);
    }

    #[test]
    fn back_walks_the_trail_then_reports_the_root() {
        let mut palette = CommandPalette::default();
        palette.enter(Screen::RunningServers);
        palette.query.push_str("api");

        assert!(palette.back());
        assert_eq!(palette.screen(), Screen::Commands);
        assert!(palette.query.is_empty(), "the child's filter stays behind");
        assert!(!palette.back(), "at the root, back means close");
    }

    #[test]
    fn back_resumes_the_parent_on_the_row_and_filter_it_was_left_from() {
        let mut palette = CommandPalette::default();
        palette.query.push_str("st");
        palette.select(2);
        palette.enter(Screen::Stashes);
        assert_eq!(
            palette.selected(5),
            Some(0),
            "a screen opens on its first row"
        );
        palette.select(1);

        palette.back();

        assert_eq!(palette.query, "st");
        assert_eq!(palette.selected(5), Some(2));
    }

    #[test]
    fn a_breadcrumb_jump_resumes_that_screen_where_it_was_left() {
        let mut palette = CommandPalette::default();
        palette.select(3);
        palette.enter(Screen::SwitchTo);
        palette.select(1);
        palette.enter(Screen::Branches);

        palette.back_to(0);

        assert_eq!(palette.screen(), Screen::Commands);
        assert_eq!(palette.selected(5), Some(3));
    }

    const DAY: i64 = 24 * 3600;

    fn usage(runs: &[(Command, i64)]) -> CommandUsage {
        let mut usage = CommandUsage::default();
        for &(command, day) in runs {
            usage.record(command, day * DAY);
        }
        usage
    }

    #[test]
    fn command_ids_are_distinct() {
        let ids: std::collections::BTreeSet<&str> = Command::ALL.map(Command::id).into();
        assert_eq!(ids.len(), Command::ALL.len());
    }

    #[test]
    fn the_last_run_command_leads_even_over_a_more_used_one() {
        let usage = usage(&[
            (Command::Pull, 1),
            (Command::Pull, 1),
            (Command::Pull, 1),
            (Command::Fetch, 2),
        ]);
        assert_eq!(
            usage.recent(&Command::ALL, 2 * DAY),
            vec![Command::Fetch, Command::Pull]
        );
    }

    #[test]
    fn a_frequent_command_keeps_its_slot_over_a_later_one_off() {
        let usage = usage(&[
            (Command::Push, 1),
            (Command::Push, 1),
            (Command::Pull, 2),
            (Command::Fetch, 3),
            (Command::CopyPath, 4),
            (Command::ToggleTheme, 5),
            (Command::WhatsNew, 6),
        ]);
        let recent = usage.recent(&Command::ALL, 6 * DAY);
        assert_eq!(
            recent,
            vec![
                Command::WhatsNew,
                Command::ToggleTheme,
                Command::CopyPath,
                Command::Fetch,
                Command::Push,
            ],
            "Pull, run once, drops out before the twice-run Push"
        );
    }

    #[test]
    fn a_command_unused_for_weeks_fades_behind_recent_one_offs() {
        let usage = usage(&[
            (Command::Push, 0),
            (Command::Push, 0),
            (Command::Push, 0),
            (Command::Pull, 30),
            (Command::Fetch, 30),
            (Command::CopyPath, 30),
            (Command::ToggleTheme, 30),
            (Command::WhatsNew, 30),
        ]);
        assert!(!usage
            .recent(&Command::ALL, 30 * DAY)
            .contains(&Command::Push));
    }

    #[test]
    fn an_unavailable_command_is_never_recent() {
        let usage = usage(&[(Command::Pull, 1), (Command::Fetch, 2)]);
        assert_eq!(usage.recent(&[Command::Pull], 2 * DAY), vec![Command::Pull]);
    }

    #[test]
    fn selection_clamps_to_a_shrinking_list() {
        let mut palette = CommandPalette::default();
        palette.select(4);
        assert_eq!(palette.selected(2), Some(1));
        assert_eq!(palette.selected(0), None);
    }
}
