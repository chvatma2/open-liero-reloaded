# tools/bug-relay

The Cloudflare Worker that files the game's bug reports as GitHub issues
(issue #3; setup and deploy in `README.md`). The game side is olr-ui's
`report.rs` (the payload) and olr-game's `report.rs` (the request).

- **Plain JavaScript, Workers module syntax, no npm dependencies**: only
  what both Workers and Node 22 have (`fetch`, `Request`, `Response`,
  `FormData`, `crypto.subtle`, `atob`/`btoa`). `package.json` only sets
  `"type": "module"`. The maintainer deploys with wrangler; nothing here
  runs in `cargo xtask check`.
- **Files**: `src/worker.js` (routes `POST /report` and `GET
  /files/<id>/<name>`; `handle(request, env, deps)` takes the tests'
  `fetch` and `now`), `src/report.js` (the format-1 checks, `neutralize`,
  `fenced`, `code`, `title`, `body`: pure), `src/github.js` (the App's
  RS256 JWT with WebCrypto, PKCS#1 keys wrapped as PKCS#8, the
  installation token, or `BOT_TOKEN` refused for the owner's or a
  `FORBIDDEN_LOGINS` login; `createIssue`), `src/limits.js` (KV counters:
  per hashed address an hour, per UTC day), `wrangler.toml` (bindings
  `LIMITS` KV, `FILES` R2; vars `GITHUB_REPO`, `PER_IP_PER_HOUR`,
  `PER_DAY`, `PUBLIC_URL`).
- **The wire** must match the game's: multipart parts `report` (olr-ui's
  `Report::json`: `format` 1, `happened`, `expected`, `sections`
  `[{title, facts: [{name, value}]}]`, `logs` `[{name, file, ended:
  running|clean|unclean, cut, text}]`, `replay` `{file, date, mod, level,
  worms, bytes}` or null) and `replay` (the file); answers `{ok: true,
  url, number}` or `{ok: false, error: too_large|invalid|rate_limited|
  upstream, detail}`, which olr-game's `report::outcome` reads. A change
  to either side changes both, and `test/fakes.js`' `gameReport` and
  `gameRequest` (the game's request byte for byte).
- **Never the maintainer's identity** (the issue runner's trust rule):
  only a GitHub App or a bot account's token.

Test: `node --test` here (`test/worker.test.js` end to end on the fakes,
`test/report.test.js` the rendering).
