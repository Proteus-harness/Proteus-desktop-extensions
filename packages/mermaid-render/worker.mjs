import { pathToFileURL } from 'node:url';
import { render, forgetTextWidths } from './vendor/pele/pele.mjs';
import { materializeSvg, themeVariables } from './svg-theme.mjs';

export const MAX_SOURCE = 50_000;
export const MAX_SVG = 1024 * 1024;
const MAX_REQUEST = 512 * 1024;

export function checkedSvg(source) {
  if (Buffer.byteLength(source, 'utf8') > MAX_SVG) throw new Error('SVG output exceeds 1 MiB');
  return source;
}

export function renderDiagram(request) {
  const source = request?.source;
  if (typeof source !== 'string' || !source.trim()) throw new Error('Mermaid source is empty');
  if (Buffer.byteLength(source, 'utf8') > MAX_SOURCE) throw new Error('Mermaid source exceeds 50,000 bytes');
  const variables = themeVariables(request.theme);
  try {
    const result = render(source, {
      fontFamily: variables.get('--pele-font'),
      fontFamilyMono: variables.get('--pele-font-mono'),
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
      svg: checkedSvg(materializeSvg(result.svg, request.theme)),
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
    const result = renderDiagram(JSON.parse(line));
    process.stdout.write(`${JSON.stringify({ ok: true, ...result })}\n`);
  } catch (error) {
    // Never log source or a parser stack. The addressed content view gets the error.
    const message = error instanceof Error ? error.message : 'Mermaid rendering failed';
    process.stdout.write(`${JSON.stringify({ ok: false, error: message.slice(0, 300) })}\n`);
  }
}

async function main() {
  if (Number(process.versions.node.split('.')[0]) < 20) throw new Error('Node.js >= 20 is required');
  let pending = Buffer.alloc(0);
  for await (const chunk of process.stdin) {
    let start = 0;
    for (let end = 0; end < chunk.length; end++) {
      if (chunk[end] !== 10) continue;
      const piece = chunk.subarray(start, end);
      if (pending.length + piece.length > MAX_REQUEST) throw new Error('worker request is too large');
      reply(Buffer.concat([pending, piece]).toString('utf8'));
      pending = Buffer.alloc(0);
      start = end + 1;
    }
    if (pending.length + chunk.length - start > MAX_REQUEST) throw new Error('worker request is too large');
    pending = Buffer.concat([pending, chunk.subarray(start)]);
  }
  if (pending.length) throw new Error('worker request must end with a newline');
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(() => { process.exitCode = 1; });
}
