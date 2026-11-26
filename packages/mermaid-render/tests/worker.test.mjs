import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { checkedSvg, MAX_SOURCE, MAX_SVG, renderDiagram } from '../worker.mjs';
import { materializeSvg, prepareRenderTheme, resolveVariables, themeVariables } from '../svg-theme.mjs';
import { parse, render } from '../vendor/pele/pele.mjs';

const theme = {
  foreground: '#e8e8ed', background: '#11131a', muted: '#8b8d98',
  border: '#424454', accent: '#8aabff', danger: '#ff7e88',
  surface: '#242633', surface_alt: '#303344',
  font_family: 'Inter, sans-serif', font_family_mono: 'monospace',
};
const flowchart = 'flowchart LR\n A[Привет] --> B[Мир]';

test('theme keeps the palette order and snapshots original fields once', () => {
  const fields = ['foreground', 'background', 'muted', 'border', 'accent', 'danger', 'surface',
    'surface_alt', 'font_family', 'font_family_mono'];
  const reads = [];
  const getters = Object.fromEntries(fields.map(field => [
    field, { get() { reads.push(field); return theme[field]; } },
  ]));
  const prepared = prepareRenderTheme(Object.defineProperties({}, getters));
  assert.equal(prepared.fontFamily, theme.font_family);
  assert.equal(prepared.fontFamilyMono, theme.font_family_mono);
  assert.equal(
    prepared.materialize('<svg fill="var(--pele-surface-alt)" stroke="var(--pele-fg)"/>'),
    '<svg fill="#303344" stroke="#e8e8ed"/>',
  );
  assert.deepEqual(reads, fields);
  assert.deepEqual([...themeVariables(theme)], [
    ['--pele-fg', theme.foreground], ['--pele-bg', theme.background],
    ['--pele-muted', theme.muted], ['--pele-border', theme.border],
    ['--pele-accent', theme.accent], ['--pele-critical', theme.danger],
    ['--pele-surface', theme.surface], ['--pele-surface-alt', theme.surface_alt],
    ['--pele-line', theme.foreground],
    ['--pele-series-1', theme.accent], ['--pele-series-2', theme.muted],
    ['--pele-series-3', theme.danger], ['--pele-series-4', theme.border],
    ['--pele-series-5', theme.accent], ['--pele-series-6', theme.muted],
    ['--pele-series-7', theme.danger], ['--pele-series-8', theme.border],
    ['--pele-font', theme.font_family], ['--pele-font-mono', theme.font_family_mono],
  ]);
  assert.throws(
    () => prepareRenderTheme({ ...theme, foreground: null, background: null }),
    /theme\.foreground must be #rrggbb or #rrggbbaa/,
  );
});

test('Gantt X seconds and x milliseconds produce the same time geometry', () => {
  const seconds = render('gantt\n dateFormat X\n axisFormat %Y\n A :1700000000,1700003600');
  const millis = render('gantt\n dateFormat x\n axisFormat %Y\n A :1700000000000,1700003600000');
  assert.equal(seconds.svg, millis.svg);
  assert.match(seconds.svg, /2023/);
  assert.doesNotMatch(seconds.svg, /1970/);
});

test('positive pie slices retain geometry below one percent', () => {
  const { svg } = render('pie\n "Large":999\n "Small":1');
  assert.equal((svg.match(/class="pele-slice"/g) ?? []).length, 2);
  assert.match(svg, /class="pele-slice" data-id="Small" d="M[^\"]+"/);
});

test('sequence autonumber preserves explicit zero start and zero step', () => {
  const { svg } = render('sequenceDiagram\n autonumber 0 0\n A->>B: First\n B->>A: Second');
  assert.equal((svg.match(/>0<\/text>/g) ?? []).length, 2);
});

test('treemap numeric separators are fully consumed', () => {
  const model = parse('treemap-beta\n "A":1_000\n "B":1000\n "C":1,000');
  assert.deepEqual(model.roots.map(item => item.value), [1000, 1000, 1000]);
  assert.throws(() => render('treemap-beta\n "Bad":1.2.3'));
});

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

test('a nested variable override does not mutate siblings or inherited definitions', () => {
  const source = '<svg style="--tone:#123456"><g><path fill="var(--tone)"/>'
    + '<g style="--tone:#abcdef"><path fill="var(--tone)"/></g>'
    + '<path fill="var(--tone)"/></g><path fill="var(--tone)"/></svg>';
  const result = materializeSvg(source, theme);
  const fills = [...result.matchAll(/<path fill="(#[a-f0-9]+)"/g)].map(match => match[1]);
  assert.deepEqual(fills, ['#123456', '#abcdef', '#123456', '#123456']);
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
