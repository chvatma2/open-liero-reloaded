//! GitHub as the runner sees it: open milestones, approved issues and one
//! issue's history, read and changed through the `gh` CLI.
//!
//! Timestamps stay GitHub's UTC `YYYY-MM-DDTHH:MM:SSZ` strings, which
//! compare correctly as strings.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

/// The labels the runner uses: name, colour, description.
pub const LABELS: [(&str, &str, &str); 4] = [
    ("approved", "0e8a16", "Approved by the maintainer: the issue runner implements it"),
    ("in-progress", "fbca04", "The issue runner is working on it"),
    ("needs-maintainer", "d93f0b", "The issue runner handed it back; see its comment"),
    ("needs-bench", "5319e7", "Done; wants a real benchmark before release"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Milestone {
    pub number: u64,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueRef {
    pub number: u64,
    pub title: String,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: String,
}

/// A label added (`added`) or removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelEvent {
    pub actor: String,
    pub label: String,
    pub at: String,
    pub added: bool,
}

/// A title change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub actor: String,
    pub at: String,
}

/// One issue with the history the trust rules need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssueDetail {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    /// Who last edited the body (`None`: never edited).
    pub editor: Option<String>,
    pub last_edited_at: Option<String>,
    pub labels: Vec<String>,
    pub comments: Vec<Comment>,
    pub label_events: Vec<LabelEvent>,
    pub renames: Vec<Rename>,
}

/// What the runner reads from and does on GitHub.
pub trait GitHub {
    fn open_milestones(&self) -> Result<Vec<Milestone>>;
    /// Open issues of `milestone` labelled `approved`.
    fn approved_issues(&self, milestone: u64) -> Result<Vec<IssueRef>>;
    /// Open issues of `milestone` labelled `needs-maintainer`.
    fn handed_back_issues(&self, milestone: u64) -> Result<Vec<IssueRef>>;
    fn issue(&self, n: u64) -> Result<IssueDetail>;
    fn edit_labels(&self, n: u64, add: &[&str], remove: &[&str]) -> Result<()>;
    fn comment(&self, n: u64, body: &str) -> Result<()>;
    fn close(&self, n: u64) -> Result<()>;
    /// Creates or updates [`LABELS`].
    fn ensure_labels(&self) -> Result<()>;
}

const ISSUE_QUERY: &str = r#"query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){issue(number:$number){
  number title body author{login} editor{login} lastEditedAt labels(first:50){nodes{name}}
  comments(first:100){nodes{author{login} body createdAt}}
  timelineItems(last:100,itemTypes:[LABELED_EVENT,UNLABELED_EVENT,RENAMED_TITLE_EVENT]){nodes{
    __typename ... on LabeledEvent{actor{login} label{name} createdAt}
    ... on UnlabeledEvent{actor{login} label{name} createdAt}
    ... on RenamedTitleEvent{actor{login} createdAt}}}}}}"#;

fn str_at(v: &Value, ptr: &str) -> String {
    v.pointer(ptr).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn opt_str_at(v: &Value, ptr: &str) -> Option<String> {
    v.pointer(ptr).and_then(Value::as_str).map(str::to_string)
}

fn array<'a>(v: &'a Value, ptr: &str) -> &'a [Value] {
    v.pointer(ptr).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default()
}

/// `gh api 'repos/{repo}/milestones?state=open'`.
pub fn parse_milestones(json: &str) -> Result<Vec<Milestone>> {
    let v: Value = serde_json::from_str(json).context("milestones JSON")?;
    let list = v.as_array().ok_or_else(|| anyhow!("milestones: not a list"))?;
    Ok(list
        .iter()
        .filter(|m| m["state"].as_str().is_none_or(|s| s == "open"))
        .map(|m| Milestone { number: m["number"].as_u64().unwrap_or_default(), title: str_at(m, "/title") })
        .collect())
}

/// `gh api 'repos/{repo}/issues?…'`; pull requests are skipped.
pub fn parse_issues(json: &str) -> Result<Vec<IssueRef>> {
    let v: Value = serde_json::from_str(json).context("issues JSON")?;
    let list = v.as_array().ok_or_else(|| anyhow!("issues: not a list"))?;
    Ok(list
        .iter()
        .filter(|i| i.get("pull_request").is_none())
        .map(|i| IssueRef {
            number: i["number"].as_u64().unwrap_or_default(),
            title: str_at(i, "/title"),
            labels: array(i, "/labels").iter().map(|l| str_at(l, "/name")).collect(),
        })
        .collect())
}

/// `gh api graphql` with [`ISSUE_QUERY`].
pub fn parse_issue(json: &str) -> Result<IssueDetail> {
    let v: Value = serde_json::from_str(json).context("issue JSON")?;
    if let Some(errors) = v.get("errors") {
        bail!("GitHub: {errors}");
    }
    let i = v
        .pointer("/data/repository/issue")
        .filter(|i| !i.is_null())
        .ok_or_else(|| anyhow!("no such issue"))?;
    let mut label_events = Vec::new();
    let mut renames = Vec::new();
    for node in array(i, "/timelineItems/nodes") {
        let actor = str_at(node, "/actor/login");
        let at = str_at(node, "/createdAt");
        match node["__typename"].as_str() {
            Some(kind @ ("LabeledEvent" | "UnlabeledEvent")) => label_events.push(LabelEvent {
                actor,
                label: str_at(node, "/label/name"),
                at,
                added: kind == "LabeledEvent",
            }),
            Some("RenamedTitleEvent") => renames.push(Rename { actor, at }),
            _ => {}
        }
    }
    Ok(IssueDetail {
        number: i["number"].as_u64().unwrap_or_default(),
        title: str_at(i, "/title"),
        body: str_at(i, "/body"),
        author: str_at(i, "/author/login"),
        editor: opt_str_at(i, "/editor/login"),
        last_edited_at: opt_str_at(i, "/lastEditedAt"),
        labels: array(i, "/labels/nodes").iter().map(|l| str_at(l, "/name")).collect(),
        comments: array(i, "/comments/nodes")
            .iter()
            .map(|c| Comment {
                author: str_at(c, "/author/login"),
                body: str_at(c, "/body"),
                created_at: str_at(c, "/createdAt"),
            })
            .collect(),
        label_events,
        renames,
    })
}

/// [`GitHub`] through the `gh` CLI and its existing login.
pub struct GhCli {
    pub repo: String,
}

impl GhCli {
    fn gh(&self, args: &[&str], stdin: Option<&str>) -> Result<String> {
        let mut child = Command::new("gh")
            .args(args)
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("cannot run gh")?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(text.as_bytes())?;
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("gh {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Open issues of `milestone` labelled `label`.
    fn labelled(&self, milestone: u64, label: &str) -> Result<Vec<IssueRef>> {
        let path = format!(
            "repos/{}/issues?milestone={milestone}&labels={label}&state=open&per_page=100",
            self.repo
        );
        parse_issues(&self.gh(&["api", &path], None)?)
    }
}

impl GitHub for GhCli {
    fn open_milestones(&self) -> Result<Vec<Milestone>> {
        parse_milestones(
            &self.gh(&["api", &format!("repos/{}/milestones?state=open&per_page=100", self.repo)], None)?,
        )
    }

    fn approved_issues(&self, milestone: u64) -> Result<Vec<IssueRef>> {
        self.labelled(milestone, "approved")
    }

    fn handed_back_issues(&self, milestone: u64) -> Result<Vec<IssueRef>> {
        self.labelled(milestone, "needs-maintainer")
    }

    fn issue(&self, n: u64) -> Result<IssueDetail> {
        let (owner, name) = self.repo.split_once('/').ok_or_else(|| anyhow!("repo is not owner/name"))?;
        let query = format!("query={ISSUE_QUERY}");
        let (owner, name, number) = (format!("owner={owner}"), format!("name={name}"), format!("number={n}"));
        parse_issue(
            &self.gh(&["api", "graphql", "-f", &query, "-f", &owner, "-f", &name, "-F", &number], None)?,
        )
    }

    fn edit_labels(&self, n: u64, add: &[&str], remove: &[&str]) -> Result<()> {
        let n = n.to_string();
        let mut args = vec!["issue", "edit", &n, "-R", &self.repo];
        for l in add {
            args.extend(["--add-label", l]);
        }
        for l in remove {
            args.extend(["--remove-label", l]);
        }
        self.gh(&args, None).map(drop)
    }

    fn comment(&self, n: u64, body: &str) -> Result<()> {
        self.gh(&["issue", "comment", &n.to_string(), "-R", &self.repo, "--body-file", "-"], Some(body))
            .map(drop)
    }

    fn close(&self, n: u64) -> Result<()> {
        self.gh(&["issue", "close", &n.to_string(), "-R", &self.repo], None).map(drop)
    }

    fn ensure_labels(&self) -> Result<()> {
        for (name, colour, description) in LABELS {
            self.gh(
                &[
                    "label",
                    "create",
                    name,
                    "--color",
                    colour,
                    "--description",
                    description,
                    "--force",
                    "-R",
                    &self.repo,
                ],
                None,
            )?;
        }
        Ok(())
    }
}
