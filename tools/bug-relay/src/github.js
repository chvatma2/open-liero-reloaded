// Filing the issue as the relay's own bot identity (issue #3's hard rule):
// a GitHub App installed only on the repository (Issues: write), whose
// installation token is got with an RS256 JWT signed by WebCrypto; or a
// separate bot account's fine-grained token, refused when it belongs to
// the repository's owner or a login in FORBIDDEN_LOGINS. Never the
// maintainer's: the issue runner trusts an issue body only when the
// maintainer wrote it, so a report posted as the maintainer would pass a
// stranger's text as trusted.

export const API = 'https://api.github.com';
const USER_AGENT = 'olr-bug-relay';

/** A failure on GitHub's side, or a setup that may not post. */
export class Upstream extends Error {}

function b64url(bytes) {
  let s = '';
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function utf8(text) {
  return new TextEncoder().encode(text);
}

/** The DER of a PEM block. */
function pemBody(pem) {
  const b64 = pem.replace(/-----[^-]+-----/g, '').replace(/\s+/g, '');
  const bin = atob(b64);
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

function derLength(n) {
  if (n < 0x80) return [n];
  const bytes = [];
  for (let v = n; v > 0; v >>= 8) bytes.unshift(v & 0xff);
  return [0x80 | bytes.length, ...bytes];
}

function der(tag, content) {
  return Uint8Array.from([tag, ...derLength(content.length), ...content]);
}

/**
 * A private key's PKCS#8 DER from its PEM: GitHub hands App keys out as
 * PKCS#1 (`BEGIN RSA PRIVATE KEY`), which WebCrypto cannot import, so
 * those are wrapped (version 0, rsaEncryption, the key as an octet string).
 */
export function pkcs8(pem) {
  const body = pemBody(pem);
  if (!pem.includes('BEGIN RSA PRIVATE KEY')) return body;
  // SEQUENCE { OID 1.2.840.113549.1.1.1, NULL }
  const algorithm = [0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00];
  const inner = [0x02, 0x01, 0x00, ...algorithm, ...der(0x04, body)];
  return der(0x30, inner);
}

/** The App's JWT for `nowSec`: issued a minute back (clock drift), valid nine minutes. */
export async function appJwt(appId, pem, nowSec) {
  const key = await crypto.subtle.importKey(
    'pkcs8',
    pkcs8(pem),
    { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' },
    false,
    ['sign'],
  );
  const header = b64url(utf8(JSON.stringify({ alg: 'RS256', typ: 'JWT' })));
  const payload = b64url(utf8(JSON.stringify({ iat: nowSec - 60, exp: nowSec + 540, iss: String(appId) })));
  const signed = `${header}.${payload}`;
  const sig = new Uint8Array(await crypto.subtle.sign('RSASSA-PKCS1-v1_5', key, utf8(signed)));
  return `${signed}.${b64url(sig)}`;
}

function headers(token) {
  return {
    Accept: 'application/vnd.github+json',
    Authorization: `Bearer ${token}`,
    'User-Agent': USER_AGENT,
    'X-GitHub-Api-Version': '2022-11-28',
  };
}

async function call(fetch, url, init, what) {
  let res;
  try {
    res = await fetch(url, init);
  } catch (e) {
    throw new Upstream(`${what}: ${e.message}`);
  }
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Upstream(`${what}: GitHub answered ${res.status} ${text.slice(0, 200)}`);
  }
  return res.json();
}

/**
 * A token to file issues with: the App's installation token, else
 * BOT_TOKEN once its login is checked not to be the owner's (or one in
 * FORBIDDEN_LOGINS, comma-separated). Throws `Upstream` without either.
 */
export async function botToken(env, fetch, nowSec) {
  if (env.GITHUB_APP_ID && env.GITHUB_APP_INSTALLATION_ID && env.GITHUB_APP_PRIVATE_KEY) {
    let jwt;
    try {
      jwt = await appJwt(env.GITHUB_APP_ID, env.GITHUB_APP_PRIVATE_KEY, nowSec);
    } catch (e) {
      throw new Upstream(`the GitHub App's key: ${e.message}`);
    }
    const url = `${API}/app/installations/${encodeURIComponent(env.GITHUB_APP_INSTALLATION_ID)}/access_tokens`;
    const got = await call(fetch, url, { method: 'POST', headers: headers(jwt) }, 'the installation token');
    if (typeof got.token !== 'string') throw new Upstream('the installation token: none in the answer');
    return got.token;
  }
  if (env.BOT_TOKEN) {
    const user = await call(fetch, `${API}/user`, { headers: headers(env.BOT_TOKEN) }, "the bot token's user");
    const owner = String(env.GITHUB_REPO ?? '').split('/')[0];
    const forbidden = [owner, ...String(env.FORBIDDEN_LOGINS ?? '').split(',')]
      .map((l) => l.trim().toLowerCase())
      .filter(Boolean);
    const login = String(user.login ?? '').toLowerCase();
    if (login === '' || forbidden.includes(login)) {
      throw new Upstream(`BOT_TOKEN belongs to ${user.login}, who may not file reports: use a bot account`);
    }
    return env.BOT_TOKEN;
  }
  throw new Upstream('no GitHub App and no BOT_TOKEN set up');
}

/** Files the issue (`labels`, no milestone, no assignee); gives `{ url, number }`. */
export async function createIssue(env, fetch, token, issue) {
  const repo = String(env.GITHUB_REPO ?? '');
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) throw new Upstream('GITHUB_REPO is not owner/name');
  const got = await call(
    fetch,
    `${API}/repos/${repo}/issues`,
    {
      method: 'POST',
      headers: { ...headers(token), 'Content-Type': 'application/json' },
      body: JSON.stringify({ title: issue.title, body: issue.body, labels: issue.labels }),
    },
    'the issue',
  );
  if (typeof got.html_url !== 'string') throw new Upstream('the issue: no address in the answer');
  return { url: got.html_url, number: got.number };
}
