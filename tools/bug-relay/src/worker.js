// Open Liero Reloaded's bug relay (issue #3): a Cloudflare Worker that
// takes the game's Report a bug and files it as a GitHub issue under the
// relay's own bot identity (`github.js`).
//
//   POST /report       multipart/form-data: part `report` (the JSON of
//                      olr-ui's `Report`), part `replay` (the replay file,
//                      when one is attached). Answers 201
//                      {"ok":true,"url":…,"number":…}, or
//                      {"ok":false,"error":"too_large"|"invalid"|
//                      "rate_limited"|"upstream","detail":…}.
//   GET /files/<id>/<name>   a stored replay or log, linked from the issue.
//
// Order: the size cap, the form, the report's checks, the rate limits
// (only a valid report is counted), the files to R2 under a random id,
// the issue. When GitHub fails, the files are deleted again.

import { Upstream, botToken, createIssue } from './github.js';
import { admit } from './limits.js';
import { Invalid, LABELS, MAX_REPLAY_BYTES, MAX_REQUEST_BYTES, body, isReplay, parseReport, title } from './report.js';

const FILE_NAMES = { replay: 'replay.olrp', logs: ['this-run.log', 'previous-run.log'] };

function json(status, value) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store' },
  });
}

function fail(status, error, detail) {
  return json(status, detail ? { ok: false, error, detail } : { ok: false, error });
}

class TooLarge extends Error {}

/** The request's body, refused past `cap` bytes (declared or streamed). */
async function readCapped(request, cap) {
  const declared = Number(request.headers.get('content-length'));
  if (Number.isFinite(declared) && declared > cap) throw new TooLarge();
  if (!request.body) return new Uint8Array(0);
  const reader = request.body.getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > cap) {
      await reader.cancel().catch(() => {});
      throw new TooLarge();
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.byteLength;
  }
  return out;
}

function randomId() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function report(request, env, deps) {
  let bytes;
  try {
    bytes = await readCapped(request, MAX_REQUEST_BYTES);
  } catch (e) {
    if (e instanceof TooLarge) return fail(413, 'too_large', `at most ${MAX_REQUEST_BYTES} bytes`);
    throw e;
  }
  const type = request.headers.get('content-type') ?? '';
  if (!type.toLowerCase().startsWith('multipart/form-data')) return fail(400, 'invalid', 'not multipart/form-data');
  let form;
  try {
    form = await new Response(bytes, { headers: { 'content-type': type } }).formData();
  } catch {
    return fail(400, 'invalid', 'a malformed form');
  }
  const part = form.get('report');
  if (part === null) return fail(400, 'invalid', 'no report');
  let parsed;
  let replay = null;
  try {
    parsed = parseReport(typeof part === 'string' ? part : await part.text());
    const file = form.get('replay');
    if (file !== null) {
      if (typeof file === 'string') throw new Invalid('the replay is no file');
      replay = new Uint8Array(await file.arrayBuffer());
      if (replay.byteLength > MAX_REPLAY_BYTES) throw new Invalid('the replay is too large');
      if (!isReplay(replay)) throw new Invalid('the replay is no OLR replay');
    }
    if ((replay === null) !== (parsed.replay === null)) throw new Invalid('the replay and its description differ');
  } catch (e) {
    if (e instanceof Invalid) return fail(400, 'invalid', e.message);
    throw e;
  }
  const ip = request.headers.get('cf-connecting-ip') ?? 'unknown';
  if (!(await admit(env.LIMITS, env, ip, deps.now()))) {
    return fail(429, 'rate_limited', 'too many reports right now');
  }
  const id = randomId();
  const base = (env.PUBLIC_URL || new URL(request.url).origin).replace(/\/+$/, '');
  const stored = [];
  const links = { replay: null, logs: [] };
  const put = async (name, data, contentType) => {
    const key = `${id}/${name}`;
    await env.FILES.put(key, data, { httpMetadata: { contentType } });
    stored.push(key);
    return `${base}/files/${key}`;
  };
  try {
    if (replay) links.replay = await put(FILE_NAMES.replay, replay, 'application/octet-stream');
    for (const [i, l] of parsed.logs.entries()) {
      links.logs.push(await put(FILE_NAMES.logs[i], l.text, 'text/plain; charset=utf-8'));
    }
    const token = await botToken(env, deps.fetch, Math.floor(deps.now() / 1000));
    const issue = await createIssue(env, deps.fetch, token, {
      title: title(parsed),
      body: body(parsed, links),
      labels: LABELS,
    });
    return json(201, { ok: true, url: issue.url, number: issue.number });
  } catch (e) {
    await Promise.all(stored.map((k) => env.FILES.delete(k).catch(() => {})));
    if (e instanceof Upstream) {
      console.error(`bug relay: ${e.message}`);
      return fail(502, 'upstream', 'GitHub did not take the report');
    }
    throw e;
  }
}

async function file(path, env) {
  const m = /^\/files\/([0-9a-f]{32})\/(replay\.olrp|this-run\.log|previous-run\.log)$/.exec(path);
  if (!m) return fail(404, 'not_found');
  const object = await env.FILES.get(`${m[1]}/${m[2]}`);
  if (!object) return fail(404, 'not_found');
  const isLog = m[2].endsWith('.log');
  return new Response(object.body, {
    headers: {
      'Content-Type': isLog ? 'text/plain; charset=utf-8' : 'application/octet-stream',
      'Content-Disposition': `${isLog ? 'inline' : 'attachment'}; filename="${m[2]}"`,
      'X-Content-Type-Options': 'nosniff',
      'Content-Security-Policy': "default-src 'none'",
    },
  });
}

/** The relay's handler; `deps` (`fetch`, `now`) are the tests' fakes. */
export async function handle(request, env, deps = {}) {
  const d = { fetch: deps.fetch ?? ((...a) => fetch(...a)), now: deps.now ?? Date.now };
  const { pathname } = new URL(request.url);
  if (pathname === '/report') {
    if (request.method !== 'POST') return fail(405, 'method_not_allowed');
    return report(request, env, d);
  }
  if (pathname.startsWith('/files/')) {
    if (request.method !== 'GET' && request.method !== 'HEAD') return fail(405, 'method_not_allowed');
    return file(pathname, env);
  }
  return fail(404, 'not_found');
}

export default {
  async fetch(request, env) {
    try {
      return await handle(request, env);
    } catch (e) {
      console.error(`bug relay: ${e?.stack ?? e}`);
      return fail(500, 'internal');
    }
  },
};
