use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use olr_runner::github::{GitHub, IssueDetail, IssueRef, Milestone};

/// GitHub in memory: label edits change the issues, every mutation is
/// logged, `fail` makes every call fail (the network is down) and
/// `fail_on` the calls of one kind (`labels`, `comment`, `close`).
#[derive(Default)]
pub struct FakeGitHub {
    pub milestones: RefCell<Vec<Milestone>>,
    pub details: RefCell<BTreeMap<u64, IssueDetail>>,
    pub closed: RefCell<BTreeSet<u64>>,
    pub log: RefCell<Vec<String>>,
    pub fail: Cell<bool>,
    pub fail_on: RefCell<Option<&'static str>>,
}

impl FakeGitHub {
    fn up(&self) -> Result<()> {
        if self.fail.get() {
            bail!("gh: network is down");
        }
        Ok(())
    }

    fn up_for(&self, op: &str) -> Result<()> {
        self.up()?;
        if *self.fail_on.borrow() == Some(op) {
            bail!("gh: {op} failed");
        }
        Ok(())
    }

    pub fn logged(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    pub fn labels(&self, n: u64) -> Vec<String> {
        self.details.borrow()[&n].labels.clone()
    }
}

impl GitHub for FakeGitHub {
    fn open_milestones(&self) -> Result<Vec<Milestone>> {
        self.up()?;
        Ok(self.milestones.borrow().clone())
    }

    fn approved_issues(&self, _milestone: u64) -> Result<Vec<IssueRef>> {
        self.up()?;
        let closed = self.closed.borrow();
        Ok(self
            .details
            .borrow()
            .values()
            .filter(|d| !closed.contains(&d.number) && d.labels.iter().any(|l| l == "approved"))
            .map(|d| IssueRef { number: d.number, title: d.title.clone(), labels: d.labels.clone() })
            .collect())
    }

    fn handed_back_issues(&self, _milestone: u64) -> Result<Vec<IssueRef>> {
        self.up()?;
        let closed = self.closed.borrow();
        Ok(self
            .details
            .borrow()
            .values()
            .filter(|d| !closed.contains(&d.number) && d.labels.iter().any(|l| l == "needs-maintainer"))
            .map(|d| IssueRef { number: d.number, title: d.title.clone(), labels: d.labels.clone() })
            .collect())
    }

    fn issue(&self, n: u64) -> Result<IssueDetail> {
        self.up()?;
        self.details.borrow().get(&n).cloned().ok_or_else(|| anyhow::anyhow!("no issue {n}"))
    }

    fn edit_labels(&self, n: u64, add: &[&str], remove: &[&str]) -> Result<()> {
        self.up_for("labels")?;
        let mut details = self.details.borrow_mut();
        let labels = &mut details.get_mut(&n).unwrap().labels;
        labels.retain(|l| !remove.contains(&l.as_str()));
        labels.extend(add.iter().map(|l| l.to_string()));
        let mut entry = format!("labels {n}");
        add.iter().for_each(|l| entry += &format!(" +{l}"));
        remove.iter().for_each(|l| entry += &format!(" -{l}"));
        self.log.borrow_mut().push(entry);
        Ok(())
    }

    fn comment(&self, n: u64, body: &str) -> Result<()> {
        self.up_for("comment")?;
        self.log.borrow_mut().push(format!("comment {n} {body}"));
        Ok(())
    }

    fn close(&self, n: u64) -> Result<()> {
        self.up_for("close")?;
        self.closed.borrow_mut().insert(n);
        self.log.borrow_mut().push(format!("close {n}"));
        Ok(())
    }

    fn ensure_labels(&self) -> Result<()> {
        self.up()?;
        self.log.borrow_mut().push("ensure-labels".into());
        Ok(())
    }
}
