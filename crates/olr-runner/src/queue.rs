//! Which issue is next: the oldest approval in the one open milestone.

use crate::github::{IssueRef, Milestone};

/// What the next poll should do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    NoMilestone,
    /// Only one milestone may be open; their titles.
    ManyMilestones(Vec<String>),
    Empty { milestone: String },
    Next { milestone: String, number: u64 },
}

/// Approved, and neither being worked nor handed back.
pub fn eligible(labels: &[String]) -> bool {
    let has = |l: &str| labels.iter().any(|x| x == l);
    has("approved") && !has("in-progress") && !has("needs-maintainer")
}

/// The next issue among `candidates`, each with the time it was approved.
pub fn pick(milestones: &[Milestone], candidates: &[(IssueRef, Option<String>)]) -> Pick {
    let milestone = match milestones {
        [] => return Pick::NoMilestone,
        [one] => one.title.clone(),
        many => return Pick::ManyMilestones(many.iter().map(|m| m.title.clone()).collect()),
    };
    let next = candidates
        .iter()
        .filter(|(i, _)| eligible(&i.labels))
        .min_by(|(a, at_a), (b, at_b)| {
            // A missing approval time sorts last.
            (at_a.is_none(), at_a, a.number).cmp(&(at_b.is_none(), at_b, b.number))
        });
    match next {
        Some((i, _)) => Pick::Next { milestone, number: i.number },
        None => Pick::Empty { milestone },
    }
}
