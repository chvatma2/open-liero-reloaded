// A report as the game sends it (olr-ui's `report.rs`, format 1): checked
// field by field, then rendered as a GitHub issue. Pure: no I/O.
//
// Everything in a report is untrusted (anyone can POST to the relay), so
// every string reaching the issue is neutralized: @-mentions,
// #-references and HTML comments cannot ping people, cross-link issues or
// hide text, and the player's text sits in code fences it cannot close.

/** The payload format this relay reads. */
export const REPORT_FORMAT = 1;
/** The whole request, at most (replays hold inputs only and are small). */
export const MAX_REQUEST_BYTES = 1024 * 1024;
/** Each of the player's texts, in characters (the game's `MAX_TEXT_CHARS`). */
export const MAX_TEXT_CHARS = 8000;
/** A log's text, in characters (the game sends at most 256 KiB). */
export const MAX_LOG_CHARS = 300 * 1024;
/** A replay file, in bytes. */
export const MAX_REPLAY_BYTES = 512 * 1024;
/** A replay file's first bytes (`olr-replay`'s `MAGIC`). */
export const REPLAY_MAGIC = [0x4f, 0x4c, 0x52, 0x50]; // "OLRP"
/** The labels every report gets (never `approved`). */
export const LABELS = ['bug', 'player-report'];
/** The title's prefix. */
export const TITLE_PREFIX = 'Player report: ';
/** The title, at most, in characters. */
export const MAX_TITLE_CHARS = 100;
/** GitHub refuses issue bodies over 65 536 characters; this stays under. */
export const MAX_BODY_CHARS = 60000;
/** The lines of each log shown in the issue itself (the whole is linked). */
export const LOG_LINES_SHOWN = 60;

const LIMITS = {
  sections: 16,
  facts: 32,
  title: 64,
  name: 64,
  value: 2048,
  logs: 2,
  file: 128,
  worms: 32,
  worm: 64,
  date: 64,
};

/** A report the relay refuses, with why (sent back as `detail`). */
export class Invalid extends Error {}

function str(v, max, what) {
  if (typeof v !== 'string') throw new Invalid(`${what} is no text`);
  if (v.length > max) throw new Invalid(`${what} is longer than ${max} characters`);
  return v;
}

function list(v, max, what) {
  if (!Array.isArray(v)) throw new Invalid(`${what} is no list`);
  if (v.length > max) throw new Invalid(`${what} has more than ${max} entries`);
  return v;
}

function count(v, what) {
  if (!Number.isSafeInteger(v) || v < 0) throw new Invalid(`${what} is no count`);
  return v;
}

function object(v, what) {
  if (v === null || typeof v !== 'object' || Array.isArray(v)) throw new Invalid(`${what} is no object`);
  return v;
}

/**
 * The report in `json` (text), checked: every field of format 1 within
 * its limits, nothing else kept. Throws `Invalid`.
 */
export function parseReport(json) {
  let raw;
  try {
    raw = JSON.parse(json);
  } catch {
    throw new Invalid('the report is no JSON');
  }
  object(raw, 'the report');
  if (raw.format !== REPORT_FORMAT) throw new Invalid(`the report's format is not ${REPORT_FORMAT}`);
  const happened = str(raw.happened, MAX_TEXT_CHARS, 'what happened').trim();
  if (happened === '') throw new Invalid('what happened is empty');
  const expected = str(raw.expected ?? '', MAX_TEXT_CHARS, 'what was expected').trim();
  const sections = list(raw.sections, LIMITS.sections, 'sections').map((s, i) => {
    object(s, `section ${i}`);
    return {
      title: str(s.title, LIMITS.title, `section ${i}'s title`),
      facts: list(s.facts, LIMITS.facts, `section ${i}'s facts`).map((f, j) => {
        object(f, `fact ${i}.${j}`);
        return { name: str(f.name, LIMITS.name, `fact ${i}.${j}'s name`), value: str(f.value, LIMITS.value, `fact ${i}.${j}'s value`) };
      }),
    };
  });
  const logs = list(raw.logs, LIMITS.logs, 'logs').map((l, i) => {
    object(l, `log ${i}`);
    const ended = str(l.ended, 16, `log ${i}'s end`);
    if (!['running', 'clean', 'unclean'].includes(ended)) throw new Invalid(`log ${i}'s end is unknown`);
    return {
      name: str(l.name, LIMITS.name, `log ${i}'s name`),
      file: str(l.file, LIMITS.file, `log ${i}'s file`),
      ended,
      cut: count(l.cut, `log ${i}'s cut`),
      text: str(l.text, MAX_LOG_CHARS, `log ${i}'s text`),
    };
  });
  let replay = null;
  if (raw.replay !== null && raw.replay !== undefined) {
    const r = object(raw.replay, 'the replay');
    replay = {
      file: str(r.file, LIMITS.file, "the replay's file"),
      date: str(r.date, LIMITS.date, "the replay's date"),
      mod: str(r.mod, LIMITS.file, "the replay's mod"),
      level: str(r.level, LIMITS.file, "the replay's level"),
      worms: list(r.worms, LIMITS.worms, "the replay's worms").map((w, i) => str(w, LIMITS.worm, `worm ${i}`)),
      bytes: count(r.bytes, "the replay's size"),
    };
  }
  return { format: REPORT_FORMAT, happened, expected, sections, logs, replay };
}

/** Whether `bytes` (a Uint8Array) start as a replay file. */
export function isReplay(bytes) {
  return bytes.length >= REPLAY_MAGIC.length && REPLAY_MAGIC.every((b, i) => bytes[i] === b);
}

const ZWSP = '​';

/**
 * `text` that cannot ping, cross-link or hide: a zero-width space after
 * every `@` and `#` (so `@user`, `#12`, `owner/repo#12` and `GH-12` stay
 * plain), `<!--` broken up, and no control characters but line ends and
 * tabs.
 */
export function neutralize(text) {
  return String(text)
    .replace(/[\u0000-\u0008\u000b-\u001f\u007f‪-‮⁦-⁩]/g, '')
    .replace(/@/g, `@${ZWSP}`)
    .replace(/#/g, `#${ZWSP}`)
    .replace(/GH-/gi, (m) => `${m[0]}${ZWSP}${m.slice(1)}`)
    .replace(/<!--/g, `<${ZWSP}!--`)
    .replace(/-->/g, `--${ZWSP}>`);
}

/** The longest run of `ch` in `text`. */
function longestRun(text, ch) {
  let best = 0;
  let run = 0;
  for (const c of text) {
    run = c === ch ? run + 1 : 0;
    best = Math.max(best, run);
  }
  return best;
}

/** `text` in a fenced block it cannot close (a fence longer than any backtick run in it). */
export function fenced(text) {
  const safe = neutralize(text);
  const fence = '`'.repeat(Math.max(3, longestRun(safe, '`') + 1));
  return `${fence}text\n${safe}\n${fence}`;
}

/** `text` as inline code on one line, fit for a table cell. */
export function code(text) {
  const safe = neutralize(text).replace(/\s*[\r\n]+\s*/g, ' ').replace(/\|/g, '\\|');
  if (safe === '') return '';
  const ticks = '`'.repeat(longestRun(safe, '`') + 1);
  const pad = safe.startsWith('`') || safe.endsWith('`') ? ' ' : '';
  return `${ticks}${pad}${safe}${pad}${ticks}`;
}

/** The issue's title: the prefix and the first line of what happened, cut. */
export function title(report) {
  const first = report.happened.split('\n').find((l) => l.trim() !== '') ?? '';
  let line = neutralize(first.trim().replace(/\s+/g, ' ')).replace(/[`]/g, "'");
  const room = MAX_TITLE_CHARS - TITLE_PREFIX.length;
  if ([...line].length > room) line = `${[...line].slice(0, room - 1).join('')}…`;
  return `${TITLE_PREFIX}${line}`;
}

function fact(report, section, name) {
  return report.sections.find((s) => s.title === section)?.facts.find((f) => f.name === name)?.value;
}

const ENDED = { running: 'still running', clean: 'ended cleanly', unclean: 'ended unexpectedly (a crash or a kill)' };

function kib(n) {
  return Math.ceil(n / 1024);
}

/** The last `n` lines of `text`. */
function lastLines(text, n) {
  const lines = text.replace(/\n$/, '').split('\n');
  return lines.slice(Math.max(0, lines.length - n)).join('\n');
}

/**
 * The issue's body: the shape of `.github/ISSUE_TEMPLATE/bug.yml` (What
 * happened, What you expected, Steps to reproduce, OLR version, Operating
 * system, Mod and level), then every section as a table, the replay's and
 * the logs' download links (`links`: `{ replay, logs: [url per log] }`)
 * and the end of each log.
 */
export function body(report, links, logLines = LOG_LINES_SHOWN) {
  const none = '_No response_';
  const out = [];
  out.push(
    "_Filed from the game's **Report a bug** by the bug relay. The texts below are the player's, unedited and untrusted; the details after them were gathered by the game and shown to the player before sending._",
    '',
    '### What happened',
    '',
    fenced(report.happened),
    '',
    '### What you expected',
    '',
    report.expected ? fenced(report.expected) : none,
    '',
    '### Steps to reproduce',
    '',
    report.replay ? 'See the attached replay below.' : none,
    '',
    '### OLR version',
    '',
    code(fact(report, 'Game', 'Version') ?? '') || none,
    '',
    '### Operating system',
    '',
  );
  const os = ['OS', 'Distribution', 'Window system']
    .map((n) => fact(report, 'System', n))
    .filter((v) => v && v !== 'unknown');
  out.push(os.length ? code(os.join(', ')) : none, '', '### Mod and level, if relevant', '');
  out.push(report.replay ? `${code(report.replay.mod)}, ${code(report.replay.level)} (the replay's)` : none, '');
  out.push('### System details', '');
  for (const s of report.sections) {
    out.push(`#### ${neutralize(s.title).replace(/[\r\n]+/g, ' ')}`, '', '| | |', '|---|---|');
    for (const f of s.facts) out.push(`| ${code(f.name)} | ${code(f.value)} |`);
    out.push('');
  }
  out.push('### Replay', '');
  if (report.replay && links.replay) {
    const r = report.replay;
    out.push(
      `[Download the replay](${links.replay}): ${code(r.file)}, ${kib(r.bytes)} KiB, ${code(r.date)}, ${code(r.mod)} on ${code(r.level)}.`,
      '',
      `Worms' names in it: ${r.worms.map(code).join(', ') || none}`,
      '',
    );
  } else {
    out.push('None attached.', '');
  }
  out.push('### Logs', '');
  if (report.logs.length === 0) out.push('None sent.', '');
  report.logs.forEach((l, i) => {
    const cut = l.cut > 0 ? `, its last ${kib(l.text.length)} KiB` : '';
    const warn = l.ended === 'unclean' ? ' **(ended unexpectedly)**' : '';
    const link = links.logs?.[i] ? `[${neutralize(l.name)}'s log](${links.logs[i]})` : neutralize(l.name);
    out.push(`- ${link}${warn}: ${code(l.file)}, ${ENDED[l.ended]}${cut}`);
  });
  out.push('');
  const head = out.join('\n');
  // Each log's end, as much as the body has room for.
  let tails = '';
  for (const n of [logLines, 30, 15, 5, 0].filter((n) => n <= logLines)) {
    tails = report.logs
      .filter(() => n > 0)
      .map((l) => {
        const end = lastLines(l.text, n);
        return `<details><summary>${neutralize(l.name)}'s last ${n} lines</summary>\n\n${fenced(end)}\n\n</details>\n`;
      })
      .join('\n');
    if (head.length + tails.length <= MAX_BODY_CHARS) break;
  }
  const all = `${head}\n${tails}`;
  return all.length <= MAX_BODY_CHARS ? all : `${all.slice(0, MAX_BODY_CHARS - 40)}\n\n_(cut to fit an issue)_\n`;
}
