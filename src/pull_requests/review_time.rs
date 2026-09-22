//! Time spent reviewing each pull request (pull-requests.md §12): accrued frame by
//! frame while a review surface is on screen in the focused window, kept in its
//! own TOML beside the prefs, read back by `helm pr time`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::model::{ForgeKind, PullRequest};

pub const REVIEW_TIME_FILE: &str = "review_time.toml";

/// A frame gap longer than this is a stretch the app spent without drawing (a
/// hidden or minimized window paints nothing), not time in front of the PR. A
/// focused app redraws at least every few seconds, so a real frame never hits it.
pub const MAX_FRAME_GAP_SECS: f64 = 90.0;

/// Accrued seconds are written to disk once this many are unsaved — a crash loses
/// at most a minute, which is the display's own granularity.
pub const FLUSH_EVERY_SECS: u64 = 60;

/// Turns egui's frame clock into whole seconds of review time. Sub-second
/// remainders carry over to the next frame; nothing is credited on the frame
/// counting starts (its gap belongs to whatever was on screen before) nor across
/// a gap the app spent not drawing.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ReviewClock {
    last_frame: Option<f64>,
    counting: bool,
    carry: f64,
}

impl ReviewClock {
    /// Once per frame, with egui's `time`: the whole seconds to credit to the open PR.
    pub fn tick(&mut self, now: f64, counting: bool) -> u64 {
        let previous = self.last_frame.replace(now);
        let was_counting = std::mem::replace(&mut self.counting, counting);
        if !(counting && was_counting) {
            self.carry = 0.0;
            return 0;
        }
        let dt = now - previous.unwrap_or(now);
        if dt <= 0.0 || dt > MAX_FRAME_GAP_SECS {
            return 0;
        }
        self.carry += dt;
        let whole = self.carry.floor();
        self.carry -= whole;
        whole as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewTimeEntry {
    pub forge: ForgeKind,
    pub repo: String,
    pub number: u64,
    pub title: String,
    pub seconds: u64,
    /// Unix epoch seconds of the last accrual.
    pub last_active: i64,
}

impl ReviewTimeEntry {
    fn is_for(&self, pr: &PullRequest) -> bool {
        self.forge == pr.forge_kind && self.repo == pr.repo_label && self.number == pr.number
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewTimeLog {
    #[serde(default, rename = "pr")]
    pub entries: Vec<ReviewTimeEntry>,
}

impl ReviewTimeLog {
    /// Adds `seconds` to the PR's entry (created on first sight, title refreshed)
    /// and returns its new total.
    pub fn accrue(&mut self, pr: &PullRequest, seconds: u64, now_epoch: i64) -> u64 {
        let entry = match self.entries.iter_mut().position(|e| e.is_for(pr)) {
            Some(index) => &mut self.entries[index],
            None => {
                self.entries.push(ReviewTimeEntry {
                    forge: pr.forge_kind,
                    repo: pr.repo_label.clone(),
                    number: pr.number,
                    title: String::new(),
                    seconds: 0,
                    last_active: now_epoch,
                });
                self.entries.last_mut().expect("just pushed")
            }
        };
        entry.title = pr.title.clone();
        entry.seconds += seconds;
        entry.last_active = now_epoch;
        entry.seconds
    }

    pub fn seconds_for(&self, pr: &PullRequest) -> u64 {
        self.entries
            .iter()
            .find(|e| e.is_for(pr))
            .map_or(0, |e| e.seconds)
    }

    /// Entries most recently reviewed first — the order a person looks them up in.
    pub fn by_recency(&self) -> Vec<&ReviewTimeEntry> {
        let mut entries: Vec<&ReviewTimeEntry> = self.entries.iter().collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.last_active));
        entries
    }

    pub fn load() -> Self {
        path().map(|p| Self::load_from(&p)).unwrap_or_default()
    }

    /// A missing file is the first launch; anything else unreadable is logged and
    /// starts empty rather than silently — the same stance as the prefs.
    pub fn load_from(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(err) => {
                eprintln!("helm: cannot read review time {}: {err}", path.display());
                return Self::default();
            }
        };
        match toml::from_str(&text) {
            Ok(log) => log,
            Err(err) => {
                eprintln!("helm: cannot parse review time {}: {err}", path.display());
                Self::default()
            }
        }
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }
}

/// `review_time.toml` in the support dir — beside the prefs, split dev / bundle
/// the same way.
pub fn path() -> Option<PathBuf> {
    crate::persistence::support_file(REVIEW_TIME_FILE)
}

/// Minutes only — a second hand on a review timer reads as pressure.
pub fn format_time_spent(seconds: u64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "< 1 min".to_owned(),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m:02} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pull_requests::model::{Checks, PrRole, PrState, Review};

    fn pr(repo: &str, number: u64, title: &str) -> PullRequest {
        PullRequest {
            forge_kind: ForgeKind::GitHub,
            repo_label: repo.to_owned(),
            number,
            title: title.to_owned(),
            role: PrRole::ToReview,
            state: PrState::Open,
            author: "mira".to_owned(),
            source_branch: "feature".to_owned(),
            dest_branch: "main".to_owned(),
            source_commit: String::new(),
            dest_commit: String::new(),
            url: String::new(),
            updated_at: String::new(),
            checks: Checks::None,
            review: Review::default(),
            reviewers: Vec::new(),
            labels: Vec::new(),
            diffstat: None,
            comment_count: None,
        }
    }

    #[test]
    fn the_first_counting_frame_credits_nothing_then_whole_seconds_carry_over() {
        let mut clock = ReviewClock::default();
        assert_eq!(clock.tick(0.0, false), 0);
        assert_eq!(
            clock.tick(50.0, true),
            0,
            "the idle gap before opening the PR is not review time"
        );
        assert_eq!(clock.tick(50.7, true), 0);
        assert_eq!(clock.tick(51.4, true), 1);
        assert_eq!(
            clock.tick(53.9, true),
            2,
            "the 0.4 s remainder carried into this frame"
        );
    }

    #[test]
    fn a_non_drawing_gap_and_a_pause_are_not_counted() {
        let mut clock = ReviewClock::default();
        clock.tick(0.0, true);
        clock.tick(1.5, true);
        assert_eq!(
            clock.tick(1.5 + MAX_FRAME_GAP_SECS + 1.0, true),
            0,
            "hidden window"
        );
        assert_eq!(clock.tick(200.0, false), 0);
        assert_eq!(
            clock.tick(200.9, true),
            0,
            "resuming starts the carry afresh"
        );
        assert_eq!(clock.tick(201.5, true), 0);
    }

    #[test]
    fn accrual_creates_then_grows_an_entry_and_refreshes_its_title() {
        let mut log = ReviewTimeLog::default();
        assert_eq!(
            log.accrue(&pr("acme/web", 42, "Draft title"), 30, 1_000),
            30
        );
        assert_eq!(
            log.accrue(&pr("acme/web", 42, "Final title"), 45, 2_000),
            75
        );
        log.accrue(&pr("acme/api", 42, "Other repo"), 5, 3_000);
        assert_eq!(
            log.entries.len(),
            2,
            "same number on another repo is another PR"
        );
        assert_eq!(log.entries[0].title, "Final title");
        assert_eq!(log.entries[0].last_active, 2_000);
        assert_eq!(log.seconds_for(&pr("acme/web", 42, "")), 75);
        assert_eq!(log.seconds_for(&pr("acme/web", 7, "")), 0);
        let recent: Vec<u64> = log.by_recency().iter().map(|e| e.seconds).collect();
        assert_eq!(recent, vec![5, 75]);
    }

    #[test]
    fn the_log_round_trips_through_toml_and_a_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(REVIEW_TIME_FILE);
        assert_eq!(ReviewTimeLog::load_from(&path), ReviewTimeLog::default());
        let mut log = ReviewTimeLog::default();
        log.accrue(&pr("acme/web", 42, "PR cockpit"), 3_900, 1_700_000_000);
        log.save_to(&path).unwrap();
        assert_eq!(ReviewTimeLog::load_from(&path), log);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("forge = \"github\""));
    }

    #[test]
    fn time_spent_reads_in_minutes_never_seconds() {
        assert_eq!(format_time_spent(0), "< 1 min");
        assert_eq!(format_time_spent(59), "< 1 min");
        assert_eq!(format_time_spent(60), "1 min");
        assert_eq!(format_time_spent(59 * 60 + 59), "59 min");
        assert_eq!(format_time_spent(3_900), "1 h 05 min");
        assert_eq!(format_time_spent(26 * 3_600), "26 h 00 min");
    }
}
