// Hand-written fakes of what the Worker talks to: KV, R2 and GitHub's
// REST API (as a `fetch`), and the game's request.

import { createVerify, generateKeyPairSync } from 'node:crypto';

/** KV: `get`, `put` (its options recorded), `delete`. */
export function fakeKv() {
  const map = new Map();
  return {
    map,
    puts: [],
    async get(key) {
      return map.has(key) ? map.get(key) : null;
    },
    async put(key, value, options) {
      this.puts.push({ key, value, options });
      map.set(key, value);
    },
    async delete(key) {
      map.delete(key);
    },
  };
}

/** R2: `put`, `get` (an object with `body` and `httpMetadata`), `delete`. */
export function fakeR2() {
  const map = new Map();
  return {
    map,
    async put(key, value, options) {
      const bytes = typeof value === 'string' ? new TextEncoder().encode(value) : new Uint8Array(value);
      map.set(key, { bytes, options });
    },
    async get(key) {
      const e = map.get(key);
      return e ? { body: e.bytes, httpMetadata: e.options?.httpMetadata } : null;
    },
    async delete(key) {
      map.delete(key);
    },
  };
}

/** An RSA key pair as a GitHub App has: the private key in `format` (`pkcs1` or `pkcs8`) PEM. */
export function appKeys(format = 'pkcs1') {
  return generateKeyPairSync('rsa', {
    modulusLength: 2048,
    publicKeyEncoding: { type: 'spki', format: 'pem' },
    privateKeyEncoding: { type: format, format: 'pem' },
  });
}

function decodePart(part) {
  return JSON.parse(Buffer.from(part, 'base64url').toString('utf8'));
}

/**
 * GitHub's API as a `fetch`: the App's installation token (the JWT
 * checked against `publicKey`), `/user` for a bot token, and issue
 * creation (`issueStatus` other than 201 fails it). Every call is in
 * `calls`, every issue in `issues`.
 */
export function fakeGithub({ publicKey, appId = '1234', installationId = '99', userLogin = 'olr-bot', issueStatus = 201, now }) {
  const calls = [];
  const issues = [];
  const reply = (status, value) =>
    new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } });
  const fetch = async (url, init = {}) => {
    const u = new URL(url);
    const auth = init.headers?.Authorization ?? '';
    calls.push({ method: init.method ?? 'GET', path: u.pathname, auth, headers: init.headers });
    if (u.hostname !== 'api.github.com') return reply(404, { message: 'Not Found' });
    if (!init.headers?.['User-Agent']) return reply(403, { message: 'User-Agent required' });
    if (u.pathname === `/app/installations/${installationId}/access_tokens` && init.method === 'POST') {
      const jwt = auth.replace(/^Bearer /, '');
      const [h, p, s] = jwt.split('.');
      const verify = createVerify('RSA-SHA256');
      verify.update(`${h}.${p}`);
      const header = decodePart(h);
      const payload = decodePart(p);
      const nowSec = Math.floor(now() / 1000);
      const good =
        header.alg === 'RS256' &&
        verify.verify(publicKey, Buffer.from(s, 'base64url')) &&
        payload.iss === appId &&
        payload.iat <= nowSec &&
        payload.exp > nowSec &&
        payload.exp - payload.iat <= 600;
      return good ? reply(201, { token: 'ghs_installation' }) : reply(401, { message: 'Bad credentials' });
    }
    if (u.pathname === '/user') {
      return auth === 'Bearer github_pat_bot' || auth === 'Bearer github_pat_owner'
        ? reply(200, { login: userLogin })
        : reply(401, { message: 'Bad credentials' });
    }
    const m = /^\/repos\/([^/]+\/[^/]+)\/issues$/.exec(u.pathname);
    if (m && init.method === 'POST') {
      if (!['Bearer ghs_installation', 'Bearer github_pat_bot'].includes(auth)) return reply(401, { message: 'Bad credentials' });
      if (issueStatus !== 201) return reply(issueStatus, { message: 'Server Error' });
      const issue = JSON.parse(init.body);
      issues.push({ repo: m[1], ...issue });
      const number = issues.length;
      return reply(201, { number, html_url: `https://github.com/${m[1]}/issues/${number}` });
    }
    return reply(404, { message: 'Not Found' });
  };
  return { fetch, calls, issues };
}

/** A report as the game's `Report::json` writes it. */
export function gameReport(over = {}) {
  return {
    format: 1,
    happened: 'The screen flickers\nwhen the match starts',
    expected: 'No flicker',
    sections: [
      { title: 'Game', facts: [{ name: 'Version', value: '0.2.0-dev.200.abc1234' }] },
      {
        title: 'System',
        facts: [
          { name: 'OS', value: 'linux x86_64' },
          { name: 'Distribution', value: 'Ubuntu 24.04.1 LTS' },
          { name: 'Window system', value: 'Wayland' },
        ],
      },
      { title: 'Video settings', facts: [{ name: 'Frame rate', value: 'VSync' }] },
    ],
    logs: [
      { name: 'This run', file: '2026-10-21_12-00-00.000Z_3.log', ended: 'running', cut: 0, text: 'line one\nline two\n' },
      { name: 'The previous run', file: '2026-10-21_11-00-00.000Z_2.log', ended: 'unclean', cut: 1024, text: 'crashed here\n' },
    ],
    replay: {
      file: 'match-000003.olrp',
      date: '2026-10-21 12:03 UTC',
      mod: 'Liero v1.0',
      level: 'Dirt Level',
      worms: ['Anna', 'Bert'],
      bytes: 10,
    },
    ...over,
  };
}

export const REPLAY = Uint8Array.from([0x4f, 0x4c, 0x52, 0x50, 4, 0, 1, 2, 3, 4]);

/**
 * The request the game sends (olr-game's `report::body`, byte for byte):
 * `multipart/form-data`, the report part, then the replay part.
 */
export function gameRequest(report, replay, { ip = '203.0.113.7', url = 'https://relay.example/report' } = {}) {
  const boundary = 'olr-report-0123456789abcdef0123456789abcdef';
  const enc = new TextEncoder();
  const parts = [
    enc.encode(
      `--${boundary}\r\nContent-Disposition: form-data; name="report"\r\nContent-Type: application/json; charset=utf-8\r\n\r\n`,
    ),
    enc.encode(typeof report === 'string' ? report : JSON.stringify(report)),
  ];
  if (replay) {
    parts.push(
      enc.encode(
        `\r\n--${boundary}\r\nContent-Disposition: form-data; name="replay"; filename="replay.olrp"\r\nContent-Type: application/octet-stream\r\n\r\n`,
      ),
      replay,
    );
  }
  parts.push(enc.encode(`\r\n--${boundary}--\r\n`));
  const body = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    body.set(p, at);
    at += p.length;
  }
  return new Request(url, {
    method: 'POST',
    body,
    headers: {
      'content-type': `multipart/form-data; boundary=${boundary}`,
      'content-length': String(body.length),
      'cf-connecting-ip': ip,
    },
  });
}
