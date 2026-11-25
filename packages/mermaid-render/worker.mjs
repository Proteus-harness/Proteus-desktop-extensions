import { pathToFileURL } from 'node:url';
import { render, forgetTextWidths } from './vendor/pele/pele.mjs';
import { materializeSvg, prepareRenderTheme, themeVariables } from './svg-theme.mjs';

export const MAX_SOURCE = 50_000;
export const MAX_SVG = 1024 * 1024;
const MAX_REQUEST = 512 * 1024;

export function checkedSvg(source) {
  if (Buffer.byteLength(source, 'utf8') > MAX_SVG) throw new Error('SVG output exceeds 1 MiB');
  return source;
}

export function renderDiagram(request) {
  return renderRequest(request, false);
}

function renderRequest(request, fromWire) {
  const source = request?.source;
  if (typeof source !== 'string' || !source.trim()) throw new Error('Mermaid source is empty');
  if (Buffer.byteLength(source, 'utf8') > MAX_SOURCE) throw new Error('Mermaid source exceeds 50,000 bytes');
  const prepared = fromWire ? prepareRenderTheme(request.theme) : null;
  const variables = prepared ? null : themeVariables(request.theme);
  try {
    const result = render(source, {
      fontFamily: prepared ? prepared.fontFamily : variables.get('--pele-font'),
      fontFamilyMono: prepared ? prepared.fontFamilyMono : variables.get('--pele-font-mono'),
      fontSize: 16,
      idPrefix: 'proteus-mermaid',
      responsive: false,
      maxWidth: 640,
      limit: MAX_SOURCE,
      outputLimit: 256_000,
      maxEdges: 500,
      links: false,
      images: false,
    });
    if (![result.width, result.height].every(value => Number.isFinite(value) && value > 0)) {
      throw new Error('invalid Mermaid dimensions');
    }
    const scale = Math.min(1, 640 / result.width, 4096 / result.height);
    return {
      svg: checkedSvg(prepared ? prepared.materialize(result.svg) : materializeSvg(result.svg, request.theme)),
      width: Math.max(1, result.width * scale),
      height: Math.max(1, result.height * scale),
    };
  } finally {
    // Pele's process-global text cache must not retain labels of closed views.
    forgetTextWidths();
  }
}

function reply(line) {
  try {
    const result = renderRequest(JSON.parse(line), true);
    process.stdout.write(`${JSON.stringify({ ok: true, ...result })}\n`);
  } catch (error) {
    // Never log source or a parser stack. The addressed content view gets the error.
    const message = error instanceof Error ? error.message : 'Mermaid rendering failed';
    process.stdout.write(`${JSON.stringify({ ok: false, error: message.slice(0, 300) })}\n`);
  }
}

async function main() {
  if (Number(process.versions.node.split('.')[0]) < 20) throw new Error('Node.js >= 20 is required');
  let pending;
  let pendingLength = 0;
  function appendPending(chunk, start, end, complete = false) {
    const nextLength = pendingLength + end - start;
    const capacity = pending?.length ?? 0;
    if (nextLength > capacity) {
      // Grow while waiting for more bytes, but a complete line needs no slack.
      const next = Buffer.alloc(complete
        ? nextLength : Math.min(MAX_REQUEST, Math.max(nextLength, capacity * 2)));
      if (pendingLength) pending.copy(next, 0, 0, pendingLength);
      pending = next;
    }
    chunk.copy(pending, pendingLength, start, end);
    pendingLength = nextLength;
  }
  for await (const chunk of process.stdin) {
    let start = 0;
    while (start < chunk.length) {
      const end = chunk.indexOf(10, start);
      if (end < 0) break;
      if (pendingLength + end - start > MAX_REQUEST) throw new Error('worker request is too large');
      if (pendingLength) {
        if (end !== start) appendPending(chunk, start, end, true);
        reply(pending.toString('utf8', 0, pendingLength));
        pending = undefined;
        pendingLength = 0;
      } else {
        reply(chunk.toString('utf8', start, end));
      }
      start = end + 1;
    }
    if (pendingLength + chunk.length - start > MAX_REQUEST) throw new Error('worker request is too large');
    if (start < chunk.length) appendPending(chunk, start, chunk.length);
  }
  if (pendingLength) throw new Error('worker request must end with a newline');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(() => { process.exitCode = 1; });
}
