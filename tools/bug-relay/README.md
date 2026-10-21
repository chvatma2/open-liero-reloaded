# Bug relay

The game's **Report a bug** (main menu, issue #3) cannot post to GitHub for
the player, so this Cloudflare Worker (free tier) takes the report and files
it as an issue in this repository, under the relay's **own bot identity**.
Plain JavaScript in Workers module syntax, no npm dependencies.

```
POST /report              multipart/form-data: part "report" (JSON), part "replay" (optional)
GET  /files/<id>/<name>   a stored replay or log, linked from the issue
```

What it does with a report, in order:

1. Refuses a request over 1 MiB (`too_large`, 413), one that is no
   `multipart/form-data`, malformed JSON, a report that is not format 1 or
   has a field over its limit (what happened at most 8 000 characters, a
   log 300 KiB, …), a replay without the `OLRP` magic, or a replay and its
   description that do not match (`invalid`, 400, with a `detail`).
2. Counts it in KV: at most `PER_IP_PER_HOUR` (3) reports per client
   address an hour (the address stored hashed) and `PER_DAY` (50) issues
   per UTC day in all (`rate_limited`, 429; the game says "Too many reports
   right now, please try again later."). Only valid reports count. KV is
   eventually consistent, so a burst may pass a little over.
3. Stores the replay and both logs in R2 under a random 128-bit id
   (`<id>/replay.olrp`, `<id>/this-run.log`, `<id>/previous-run.log`),
   served by `GET /files/…` (the replay as a download, the logs as plain
   text).
4. Files the issue: labels `bug` and `player-report`, no milestone, never
   `approved`. The title is `Player report: ` and the first line of what
   happened (at most 100 characters). The body follows
   `.github/ISSUE_TEMPLATE/bug.yml` (What happened, What you expected,
   Steps to reproduce, OLR version, Operating system, Mod and level), then
   the system details as tables, the replay's link with its mod, level and
   worms' names, the logs' links and each log's last lines. The player's
   texts are fenced as quoted material in a fence they cannot close, and
   every string (texts, details, names) has @-mentions, #-references
   (`#12`, `owner/repo#12`, `GH-12`) and HTML comments broken with a
   zero-width space, so a report cannot ping people, cross-link issues or
   hide text. On a GitHub failure the files are deleted again (`upstream`,
   502).
5. Answers `201 {"ok":true,"url":"https://github.com/…/issues/N","number":N}`.

## Identity: never the maintainer's token

The issue runner trusts an issue body only when the maintainer wrote or
last edited it (`docs/superpowers/specs/2026-10-20-issue-runner-design.md`,
"Trusted and untrusted text"). A report posted with the maintainer's token
would pass a stranger's text as trusted. So the relay posts as:

- **a GitHub App** (preferred), installed only on this repository with
  *Issues: Read and write* and nothing else: its issues show as
  `<app-name>[bot]`; or
- **a separate bot account's** fine-grained token (`BOT_TOKEN`), limited to
  this repository with *Issues: Read and write*. The relay asks GitHub whose
  token it is and refuses to post when it is the repository's owner or a
  login in `FORBIDDEN_LOGINS`.

## One-time setup (the maintainer)

1. **Cloudflare account** (free plan) and wrangler on your machine:
   `npm install -g wrangler`, then `wrangler login`.
2. **R2 bucket** for the files: `wrangler r2 bucket create olr-bug-reports`
   (the name `wrangler.toml` binds as `FILES`). Optionally add a lifecycle
   rule in the dashboard deleting objects after, say, 365 days.
3. **KV namespace** for the counters: `wrangler kv namespace create LIMITS`;
   put the id it prints in `wrangler.toml`'s `[[kv_namespaces]]` `id`.
4. **The GitHub App**: GitHub → Settings → Developer settings → GitHub Apps
   → New GitHub App. Name it (e.g. `olr-bug-relay`), any homepage URL, no
   webhook (untick *Active*), Repository permissions → *Issues: Read and
   write*, nothing else; *Only on this account*. Create it, note its **App
   ID**, and *Generate a private key* (a `.pem` download). Then *Install
   App* → only this repository; the installation's URL ends in its
   **installation id** (`…/settings/installations/<id>`).
   (Instead of an App: a separate GitHub account for the bot and a token
   of that account that can open issues here: a fine-grained token for
   this repository with *Issues: Read and write* when the account has
   access to it, else a classic token with only the `public_repo` scope.
   Never a token of the maintainer's own account.)
5. **Labels**: `bug` and `player-report` exist in the repository (the App
   cannot create labels; GitHub drops unknown ones silently).
6. **Secrets**, from this directory:
   ```
   wrangler secret put GITHUB_APP_ID
   wrangler secret put GITHUB_APP_INSTALLATION_ID
   wrangler secret put GITHUB_APP_PRIVATE_KEY < olr-bug-relay.private-key.pem
   wrangler secret put IP_SALT          # optional: any random text
   ```
   The PEM may be GitHub's PKCS#1 (`BEGIN RSA PRIVATE KEY`) or PKCS#8 as
   is. For the bot account instead: `wrangler secret put BOT_TOKEN`.
7. **Check `wrangler.toml`'s `[vars]`**: `GITHUB_REPO`, the limits, and
   `PUBLIC_URL` if the Worker has a custom domain (empty: the address the
   request came to, e.g. `https://olr-bug-relay.<you>.workers.dev`).
8. **Deploy**: `wrangler deploy`. Try it with a report from a game built
   with the address (below), or with `wrangler tail` open to see the
   relay's errors.

## The address the game sends to

The game has the relay's address baked in at build time:
`option_env!("OLR_REPORT_URL")` in `crates/olr-game/src/report.rs`.
`cargo xtask dist` passes `OLR_REPORT_URL` on from its own environment:

```
OLR_REPORT_URL=https://olr-bug-relay.<you>.workers.dev/report cargo xtask dist
```

A build without it (dev builds, builds before the relay is deployed) hides
**Report a bug** entirely. To change the address, deploy the Worker where
you want it and build the release again with the new `OLR_REPORT_URL`;
older builds keep sending to the old address, so keep it answering (or
let them fail: they show the error and keep the text). At run time the
variable `OLR_REPORT_URL` overrides the build's address (a local mock in
tests, or `wrangler dev`'s `http://127.0.0.1:8787/report`).

## Tests

No wrangler, npm or network: Node 22's test runner against hand-written
fakes of GitHub's API, R2 and KV (`test/fakes.js`; the GitHub fake checks
the App's RS256 JWT with the key's public half).

```
cd tools/bug-relay && node --test
```

`cargo xtask check` does not run them; run them after changing the relay.
