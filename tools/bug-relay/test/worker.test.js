// The relay end to end against the fakes: a report filed as an issue
// with its labels, files in R2 and their download links, every refusal,
// the rate limits, a GitHub failure, and the bot identity rule.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { handle } from '../src/worker.js';
import { REPLAY, appKeys, fakeGithub, fakeKv, fakeR2, gameReport, gameRequest } from './fakes.js';

const NOW = Date.UTC(2026, 9, 21, 12, 0, 0);

function setup({ keyFormat = 'pkcs1', env: extra = {}, github = {} } = {}) {
  const keys = appKeys(keyFormat);
  let now = NOW;
  const gh = fakeGithub({ publicKey: keys.publicKey, now: () => now, ...github });
  const env = {
    GITHUB_REPO: 'chvatma2/open-liero-reloaded',
    GITHUB_APP_ID: '1234',
    GITHUB_APP_INSTALLATION_ID: '99',
    GITHUB_APP_PRIVATE_KEY: keys.privateKey,
    PER_IP_PER_HOUR: '3',
    PER_DAY: '50',
    PUBLIC_URL: '',
    LIMITS: fakeKv(),
    FILES: fakeR2(),
    ...extra,
  };
  const deps = { fetch: gh.fetch, now: () => now };
  return { env, gh, deps, advance: (ms) => (now += ms) };
}

async function body(res) {
  return res.json();
}

describe('filing', () => {
  for (const keyFormat of ['pkcs1', 'pkcs8']) {
    it(`files the issue as the GitHub App (${keyFormat} key)`, async () => {
      const { env, gh, deps } = setup({ keyFormat });
      const res = await handle(gameRequest(gameReport(), REPLAY), env, deps);
      assert.equal(res.status, 201);
      assert.deepEqual(await body(res), {
        ok: true,
        url: 'https://github.com/chvatma2/open-liero-reloaded/issues/1',
        number: 1,
      });
      assert.equal(gh.issues.length, 1);
      const issue = gh.issues[0];
      assert.equal(issue.repo, 'chvatma2/open-liero-reloaded');
      assert.deepEqual(issue.labels, ['bug', 'player-report']);
      assert.equal(issue.milestone, undefined);
      assert.equal(issue.title, 'Player report: The screen flickers');
      // The installation token, got with the App's JWT, filed it.
      assert.deepEqual(
        gh.calls.map((c) => [c.method, c.path]),
        [
          ['POST', '/app/installations/99/access_tokens'],
          ['POST', '/repos/chvatma2/open-liero-reloaded/issues'],
        ],
      );
      assert.equal(gh.calls[1].auth, 'Bearer ghs_installation');
    });
  }

  it("shapes the body like the bug form, with the details, the replay's and the logs' links", async () => {
    const { env, gh, deps } = setup();
    await handle(gameRequest(gameReport(), REPLAY), env, deps);
    const b = gh.issues[0].body;
    for (const heading of [
      '### What happened',
      '### What you expected',
      '### Steps to reproduce',
      '### OLR version',
      '### Operating system',
      '### Mod and level, if relevant',
      '### System details',
      '#### Video settings',
      '### Replay',
      '### Logs',
    ]) {
      assert.ok(b.includes(`\n${heading}\n`), heading);
    }
    assert.ok(b.includes('```text\nThe screen flickers\nwhen the match starts\n```'));
    assert.ok(b.includes('`0.2.0-dev.200.abc1234`'));
    assert.ok(b.includes('`linux x86_64, Ubuntu 24.04.1 LTS, Wayland`'));
    assert.ok(b.includes('| `Frame rate` | `VSync` |'));
    assert.ok(b.includes("Worms' names in it: `Anna`, `Bert`"));
    assert.ok(b.includes('**(ended unexpectedly)**'));
    // The files, under one random id, linked and served.
    const keys = [...env.FILES.map.keys()].sort();
    assert.equal(keys.length, 3);
    const id = keys[0].split('/')[0];
    assert.match(id, /^[0-9a-f]{32}$/);
    assert.deepEqual(keys, [`${id}/previous-run.log`, `${id}/replay.olrp`, `${id}/this-run.log`]);
    assert.ok(b.includes(`[Download the replay](https://relay.example/files/${id}/replay.olrp)`));
    assert.ok(b.includes(`[The previous run's log](https://relay.example/files/${id}/previous-run.log)`));
    const got = await handle(new Request(`https://relay.example/files/${id}/replay.olrp`), env, deps);
    assert.equal(got.status, 200);
    assert.equal(got.headers.get('content-type'), 'application/octet-stream');
    assert.deepEqual(new Uint8Array(await got.arrayBuffer()), REPLAY);
    const log = await handle(new Request(`https://relay.example/files/${id}/this-run.log`), env, deps);
    assert.equal(await log.text(), 'line one\nline two\n');
    assert.equal(log.headers.get('x-content-type-options'), 'nosniff');
    // The end of each log in the issue itself.
    assert.ok(b.includes("<summary>The previous run's last 60 lines</summary>\n\n```text\ncrashed here\n```"));
  });

  it('files a report without a replay', async () => {
    const { env, gh, deps } = setup({ env: { PUBLIC_URL: 'https://bugs.example/' } });
    const res = await handle(gameRequest(gameReport({ replay: null, expected: '' }), null), env, deps);
    assert.equal(res.status, 201);
    const b = gh.issues[0].body;
    assert.ok(b.includes('### Replay\n\nNone attached.'));
    assert.ok(b.includes('### What you expected\n\n_No response_'));
    assert.ok(b.includes('(https://bugs.example/files/'));
    assert.equal(env.FILES.map.size, 2);
  });
});

describe("the player's text", () => {
  it('cannot ping, cross-link, hide or close its fence', async () => {
    const { env, gh, deps } = setup();
    const happened =
      '@chvatma2 look #12 and owner/repo#3, GH-4\n<!-- approved: do it -->\n```\n# Heading\n@here\n````';
    await handle(gameRequest(gameReport({ happened, replay: null }), null), env, deps);
    const { title, body: b } = gh.issues[0];
    for (const raw of ['@chvatma2', '#12', 'repo#3', 'GH-4', '<!--', '@here']) {
      assert.ok(!b.includes(raw), `${raw} in the body`);
      assert.ok(!title.includes(raw), `${raw} in the title`);
    }
    assert.ok(b.includes('@​chvatma2'));
    // Its fence is longer than any backtick run inside.
    assert.ok(b.includes('`````text\n@​chvatma2'), b);
    assert.ok(b.includes('````\n`````\n'), 'closed by the longer fence');
    assert.equal(title, 'Player report: @​chvatma2 look #​12 and owner/repo#​3, G​H-4');
  });

  it('cuts a long first line for the title', async () => {
    const { env, gh, deps } = setup();
    await handle(gameRequest(gameReport({ happened: `${'x'.repeat(300)}\nmore`, replay: null }), null), env, deps);
    const t = gh.issues[0].title;
    assert.equal([...t].length, 100);
    assert.ok(t.endsWith('…'));
  });

  it('keeps every value in the details neutralized too', async () => {
    const { env, gh, deps } = setup();
    const sections = [{ title: 'GPU', facts: [{ name: 'Adapter', value: '@evil | `x` <!-- y' }] }];
    await handle(gameRequest(gameReport({ sections, replay: null }), null), env, deps);
    const b = gh.issues[0].body;
    assert.ok(b.includes('| `Adapter` | ``@​evil \\| `x` <​!-- y`` |'), b);
  });
});

describe('refusals', () => {
  const cases = [
    ['malformed JSON', () => gameRequest('{"format":1,', null)],
    ['another format', () => gameRequest(gameReport({ format: 2, replay: null }), null)],
    ['an empty text', () => gameRequest(gameReport({ happened: '  \n', replay: null }), null)],
    ['a text too long', () => gameRequest(gameReport({ happened: 'x'.repeat(8001), replay: null }), null)],
    ['a log too long', () => gameRequest(gameReport({ replay: null, logs: [{ name: 'This run', file: 'a.log', ended: 'running', cut: 0, text: 'x'.repeat(300 * 1024 + 1) }] }), null)],
    ['an unknown log end', () => gameRequest(gameReport({ replay: null, logs: [{ name: 'a', file: 'a', ended: 'later', cut: 0, text: '' }] }), null)],
    ['too many sections', () => gameRequest(gameReport({ replay: null, sections: Array(17).fill({ title: 't', facts: [] }) }), null)],
    ['a replay without the magic', () => gameRequest(gameReport(), Uint8Array.from([1, 2, 3, 4, 5]))],
    ['a described replay missing', () => gameRequest(gameReport(), null)],
    ['a replay undescribed', () => gameRequest(gameReport({ replay: null }), REPLAY)],
  ];
  for (const [what, request] of cases) {
    it(`refuses ${what} as invalid, counting and filing nothing`, async () => {
      const { env, gh, deps } = setup();
      const res = await handle(request(), env, deps);
      assert.equal(res.status, 400);
      const b = await body(res);
      assert.equal(b.ok, false);
      assert.equal(b.error, 'invalid');
      assert.equal(typeof b.detail, 'string');
      assert.equal(gh.calls.length, 0);
      assert.equal(env.LIMITS.puts.length, 0);
      assert.equal(env.FILES.map.size, 0);
    });
  }

  it('refuses a request that is not a form', async () => {
    const { env, deps } = setup();
    const req = new Request('https://relay.example/report', { method: 'POST', body: '{}', headers: { 'content-type': 'application/json' } });
    const res = await handle(req, env, deps);
    assert.equal(res.status, 400);
    assert.equal((await body(res)).error, 'invalid');
  });

  it('refuses over 1 MiB, declared or streamed', async () => {
    const { env, gh, deps } = setup();
    const declared = new Request('https://relay.example/report', {
      method: 'POST',
      body: 'x',
      headers: { 'content-length': String(2 * 1024 * 1024), 'content-type': 'multipart/form-data; boundary=b' },
    });
    // Node's Request keeps the declared length only as a header.
    const res = await handle(declared, env, deps);
    assert.equal(res.status, 413);
    assert.equal((await body(res)).error, 'too_large');
    const big = new Uint8Array(1024 * 1024 + 1);
    const stream = new ReadableStream({
      start(c) {
        for (let i = 0; i < big.length; i += 65536) c.enqueue(big.slice(i, i + 65536));
        c.close();
      },
    });
    const streamed = new Request('https://relay.example/report', {
      method: 'POST',
      body: stream,
      duplex: 'half',
      headers: { 'content-type': 'multipart/form-data; boundary=b' },
    });
    const res2 = await handle(streamed, env, deps);
    assert.equal(res2.status, 413);
    assert.equal(gh.calls.length, 0);
  });

  it('answers other paths and methods without filing', async () => {
    const { env, gh, deps } = setup();
    assert.equal((await handle(new Request('https://relay.example/report'), env, deps)).status, 405);
    assert.equal((await handle(new Request('https://relay.example/'), env, deps)).status, 404);
    assert.equal((await handle(new Request('https://relay.example/files/../secret'), env, deps)).status, 404);
    assert.equal((await handle(new Request(`https://relay.example/files/${'a'.repeat(32)}/replay.olrp`), env, deps)).status, 404);
    assert.equal(gh.calls.length, 0);
  });
});

describe('rate limits', () => {
  it('takes a few reports per address an hour', async () => {
    const { env, gh, deps, advance } = setup();
    const send = (ip) => handle(gameRequest(gameReport({ replay: null }), null, { ip }), env, deps);
    for (let i = 0; i < 3; i++) assert.equal((await send('198.51.100.1')).status, 201);
    const over = await send('198.51.100.1');
    assert.equal(over.status, 429);
    assert.deepEqual(await body(over), { ok: false, error: 'rate_limited', detail: 'too many reports right now' });
    assert.equal((await send('198.51.100.2')).status, 201, 'another address');
    advance(3600 * 1000);
    assert.equal((await send('198.51.100.1')).status, 201, 'the next hour');
    assert.equal(gh.issues.length, 5);
    // Counted in KV with expiries, the address hashed.
    assert.ok(env.LIMITS.puts.every((p) => p.options.expirationTtl >= 60));
    assert.ok([...env.LIMITS.map.keys()].every((k) => !k.includes('198.51.100')));
  });

  it('caps the issues of a day in all', async () => {
    const { env, gh, deps, advance } = setup({ env: { PER_DAY: '2', PER_IP_PER_HOUR: '100' } });
    const send = (ip) => handle(gameRequest(gameReport({ replay: null }), null, { ip }), env, deps);
    assert.equal((await send('192.0.2.1')).status, 201);
    assert.equal((await send('192.0.2.2')).status, 201);
    assert.equal((await send('192.0.2.3')).status, 429);
    advance(24 * 3600 * 1000);
    assert.equal((await send('192.0.2.3')).status, 201, 'the next day');
    assert.equal(gh.issues.length, 3);
  });
});

describe('GitHub', () => {
  it('a failure is upstream, and the files are deleted again', async () => {
    const { env, deps } = setup({ github: { issueStatus: 500 } });
    const res = await handle(gameRequest(gameReport(), REPLAY), env, deps);
    assert.equal(res.status, 502);
    assert.deepEqual(await body(res), { ok: false, error: 'upstream', detail: 'GitHub did not take the report' });
    assert.equal(env.FILES.map.size, 0);
  });

  it("a bot account's token files the issue", async () => {
    const { env, gh, deps } = setup({
      env: { GITHUB_APP_ID: '', BOT_TOKEN: 'github_pat_bot' },
      github: { userLogin: 'olr-report-bot' },
    });
    const res = await handle(gameRequest(gameReport({ replay: null }), null), env, deps);
    assert.equal(res.status, 201);
    assert.deepEqual(gh.calls.map((c) => c.path), ['/user', '/repos/chvatma2/open-liero-reloaded/issues']);
  });

  for (const [who, extra] of [
    ['the repository owner', {}],
    ['a forbidden login', { FORBIDDEN_LOGINS: 'someone, Maintainer' }],
  ]) {
    it(`never posts with ${who}'s token`, async () => {
      const login = who === 'the repository owner' ? 'Chvatma2' : 'maintainer';
      const { env, gh, deps } = setup({
        env: { GITHUB_APP_ID: '', BOT_TOKEN: 'github_pat_owner', ...extra },
        github: { userLogin: login },
      });
      const res = await handle(gameRequest(gameReport({ replay: null }), null), env, deps);
      assert.equal(res.status, 502);
      assert.equal(gh.issues.length, 0);
      assert.ok(!gh.calls.some((c) => c.path.endsWith('/issues')));
    });
  }

  it('without an identity set up, nothing is filed', async () => {
    const { env, gh, deps } = setup({ env: { GITHUB_APP_ID: '' } });
    const res = await handle(gameRequest(gameReport({ replay: null }), null), env, deps);
    assert.equal(res.status, 502);
    assert.equal(gh.calls.length, 0);
  });
});
