//! Which issue is next: the oldest approval in the one open milestone,
//! unless an older issue of it was handed back. Issues more often than not
//! build on each other, so a hand-back holds every newer issue until the
//! maintainer unblocks it (retries it or removes `needs-maintainer`).

use crate::github::{IssueRef, Milestone};

/// What the next poll should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    NoMilestone,
    /// Only one milestone may be open; their titles.
    ManyMilestones(Vec<String>),
    Empty {
        milestone: String,
    },
    Next {
        milestone: String,
        number: u64,
    },
    /// Handed back (`needs-maintainer`) and older than the next issue:
    /// nothing starts until the maintainer unblocks it.
    Held {
        milestone: String,
        number: u64,
    },
}

/// Approved, and neither being worked nor handed back.
pub fn eligible(labels: &[String]) -> bool {
    let has = |l: &str| labels.iter().any(|x| x == l);
    has("approved") && !has("in-progress") && !has("needs-maintainer")
}

/// The next issue among `candidates`, each with the time it was approved;
/// `handed_back` are the milestone's open `needs-maintainer` issues.
pub fn pick(
    milestones: &[Milestone],
    candidates: &[(IssueRef, Option<String>)],
    handed_back: &[u64],
) -> Pick {
    let milestone = match milestones {
        [] => return Pick::NoMilestone,
        [one] => one.title.clone(),
        many => return Pick::ManyMilestones(many.iter().map(|m| m.title.clone()).collect()),
    };
    let next = candidates.iter().filter(|(i, _)| eligible(&i.labels)).min_by(|(a, at_a), (b, at_b)| {
        // A missing approval time sorts last.
        (at_a.is_none(), at_a, a.number).cmp(&(at_b.is_none(), at_b, b.number))
    });
    let held = handed_back.iter().copied().min();
    match (next, held) {
        (Some((i, _)), Some(h)) if h < i.number => Pick::Held { milestone, number: h },
        (Some((i, _)), _) => Pick::Next { milestone, number: i.number },
        (None, Some(h)) => Pick::Held { milestone, number: h },
        (None, None) => Pick::Empty { milestone },
    }
}
