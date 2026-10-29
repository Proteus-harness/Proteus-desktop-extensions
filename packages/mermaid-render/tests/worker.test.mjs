import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { checkedSvg, MAX_SOURCE, MAX_SVG, renderDiagram } from '../worker.mjs';
import { materializeSvg, resolveVariables } from '../svg-theme.mjs';

const theme = {
  foreground: '#e8e8ed', background: '#11131a', muted: '#8b8d98',
  border: '#424454', accent: '#8aabff', danger: '#ff7e88',
  surface: '#242633', surface_alt: '#303344',
  font_family: 'Inter, sans-serif', font_family_mono: 'monospace',
};
const flowchart = 'flowchart LR\n A[Привет] --> B[Мир]';

test('flowchart with Cyrillic produces theme-materialized self-contained SVG', () => {
  const result = renderDiagram({ source: flowchart, theme });
  assert.ok(result.width > 0 && result.height > 0);
  assert.match(result.svg, /Привет/);
  assert.match(result.svg, /Мир/);
  assert.match(result.svg, /fill="#e8e8ed"/);
  assert.match(result.svg, /fill="#242633"/);
  assert.match(result.svg, /stroke="#424454"/);
  assert.match(result.svg, /rx="4px"/);
  assert.match(result.svg, /font-size="16px"/);
  assert.doesNotMatch(result.svg, /var\(|currentColor|--_fg|foreignObject|<script/);
});

test('sequence diagrams preserve actors and messages', () => {
  const result = renderDiagram({ source: 'sequenceDiagram\n Аня->>Борис: Привет\n Борис-->>Аня: Готово', theme });
  assert.match(result.svg, /pele-sequence/);
  assert.match(result.svg, /Готово/);
  assert.ok(result.width <= 4096 && result.height <= 4096);
});

test('unsupported source and absent host theme report errors', () => {
  assert.throws(() => renderDiagram({ source: 'not-a-diagram Привет', theme }));
  assert.throws(() => renderDiagram({ source: flowchart }), /theme is required/);
  assert.throws(() => renderDiagram({ source: flowchart, theme: { ...theme, foreground: 'red;url(file)' } }), /foreground/);
});

test('nested fallbacks and variable cycles are handled without changing labels', () => {
  const variables = new Map([['--a', 'var(--b, var(--c, #123456))']]);
  assert.equal(resolveVariables('fill:var(--a)', variables), 'fill:#123456');
  assert.throws(() => resolveVariables('var(--a)', new Map([['--a', 'var(--a)']])), /cyclic/);
  const input = '<svg style="--_fg:var(--pele-fg,#000);color:var(--_fg)" fill="var(--_fg)"><g style="--local:#ffee00;color:var(--local)"><path fill="currentColor"/><text>var(--_fg)</text></g><path fill="currentColor"/></svg>';
  const result = materializeSvg(input, theme);
  assert.match(result, /<path fill="#ffee00"\/>/);
  assert.match(result, /<text>var\(--_fg\)<\/text>/);
  assert.match(result, /<\/g><path fill="#e8e8ed"\/>/);
  assert.doesNotMatch(result, /--local:|--_fg:/);
});

test('source and UTF-8 SVG output have explicit byte limits', () => {
  assert.throws(() => renderDiagram({ source: 'x'.repeat(MAX_SOURCE + 1), theme }), /source exceeds/);
  assert.throws(() => checkedSvg('Ж'.repeat(MAX_SVG / 2 + 1)), /output exceeds/);
  const source = 'flowchart TB\n' + Array.from({ length: 502 }, (_, index) => `N${index}-->N${index + 1}`).join('\n');
  assert.throws(() => renderDiagram({ source, theme }), /edge|limit/i);
});

test('one worker handles multiple wire requests and exits when its pipe closes', { timeout: 5000 }, async t => {
  const child = spawn(process.execPath, [fileURLToPath(new URL('../worker.mjs', import.meta.url))], { stdio: ['pipe', 'pipe', 'pipe'] });
  t.after(() => { child.kill(); });
  let output = '';
  let errors = '';
  child.stdout.setEncoding('utf8').on('data', chunk => { output += chunk; });
  child.stderr.setEncoding('utf8').on('data', chunk => { errors += chunk; });
  const exited = once(child, 'exit');
  child.stdin.end([
    JSON.stringify({ source: flowchart, theme }),
    JSON.stringify({ source: 'sequenceDiagram\n A->>B: done', theme }),
    JSON.stringify({ source: 'unsupported', theme }),
  ].join('\n') + '\n');
  assert.equal((await exited)[0], 0);
  const replies = output.trim().split('\n').map(line => JSON.parse(line));
  assert.equal(replies.length, 3);
  assert.deepEqual(replies.map(reply => reply.ok), [true, true, false]);
  assert.equal(errors, '');
});

test('oversized unterminated wire input exits without growing a line buffer', { timeout: 5000 }, async t => {
  const child = spawn(process.execPath, [fileURLToPath(new URL('../worker.mjs', import.meta.url))], { stdio: ['pipe', 'pipe', 'pipe'] });
  t.after(() => { child.kill(); });
  child.stdin.on('error', () => {});
  const exited = once(child, 'exit');
  child.stdin.end('x'.repeat(512 * 1024 + 1));
  assert.equal((await exited)[0], 1);
});
