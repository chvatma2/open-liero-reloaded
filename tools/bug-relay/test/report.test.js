// The report's checks and rendering, without the Worker.

import assert from 'node:assert/strict';
import { it } from 'node:test';

import { MAX_BODY_CHARS, body, code, fenced, isReplay, neutralize, parseReport } from '../src/report.js';
import { REPLAY, gameReport } from './fakes.js';

it('keeps only the fields of format 1', () => {
  const r = parseReport(JSON.stringify({ ...gameReport(), extra: 'dropped', happened: '  It broke \n' }));
  assert.equal(r.happened, 'It broke');
  assert.equal(r.extra, undefined);
  assert.deepEqual(Object.keys(r), ['format', 'happened', 'expected', 'sections', 'logs', 'replay']);
});

it('neutralizes mentions, references, comments and direction marks', () => {
  assert.equal(neutralize('@a #1 <!-- x --> GH-2 ‮evil\u0007'), '@​a #​1 <​!-- x --​> G​H-2 evil');
});

it('fences and code spans cannot be closed from inside', () => {
  assert.equal(fenced('a\n```\nb'), '````text\na\n```\nb\n````');
  assert.equal(code('x `y` | z\nw'), '``x `y` \\| z w``');
  assert.equal(code('`edge`'), '`` `edge` ``');
  assert.equal(code(''), '');
});

it('fits an issue whatever the report holds', () => {
  const huge = 'line with @someone in it\n'.repeat(12000);
  const r = parseReport(
    JSON.stringify(
      gameReport({
        happened: '@'.repeat(8000),
        logs: [
          { name: 'This run', file: 'a.log', ended: 'running', cut: 5, text: huge.slice(0, 290000) },
          { name: 'The previous run', file: 'b.log', ended: 'unclean', cut: 0, text: huge.slice(0, 290000) },
        ],
        sections: Array(16).fill({ title: 'S', facts: Array(32).fill({ name: 'n', value: 'v'.repeat(2048) }) }),
      }),
    ),
  );
  const b = body(r, { replay: 'https://r/x', logs: ['https://r/a', 'https://r/b'] });
  assert.ok(b.length <= MAX_BODY_CHARS, `${b.length}`);
  // The player's text comes first, whole.
  assert.ok(b.includes(`\`\`\`text\n${'@​'.repeat(8000)}\n\`\`\``));
});

it('shows fewer log lines when the logs are long', () => {
  const long = 'x'.repeat(1500) + '\n';
  const r = parseReport(
    JSON.stringify(gameReport({ logs: [{ name: 'This run', file: 'a.log', ended: 'running', cut: 0, text: long.repeat(200) }] })),
  );
  const b = body(r, { replay: 'https://r/x', logs: ['https://r/a'] });
  assert.ok(b.length <= MAX_BODY_CHARS);
  assert.ok(b.includes("This run's last 30 lines"), b.slice(-300));
});

it('knows a replay by its magic', () => {
  assert.ok(isReplay(REPLAY));
  assert.ok(!isReplay(Uint8Array.from([0x4f, 0x4c, 0x52])));
  assert.ok(!isReplay(new TextEncoder().encode('PK\u0003\u0004')));
});
