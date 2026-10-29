// Pele's public theme tokens and private aliases are described in
// https://github.com/obsidianmd/pele/blob/main/src/svg/{theme,root}.ts.
// Resolve presentation attributes only: diagram labels must remain untouched.
const COLORS = {
  foreground: 'fg', background: 'bg', muted: 'muted', border: 'border',
  accent: 'accent', danger: 'critical', surface: 'surface', surface_alt: 'surface-alt',
};
const PRESENTATION = new Set([
  'fill', 'stroke', 'color', 'font-family', 'font-weight', 'font-size',
  'rx', 'ry', 'stop-color', 'flood-color', 'lighting-color',
]);

export function themeVariables(theme) {
  if (!theme || typeof theme !== 'object' || Array.isArray(theme)) {
    throw new Error('content.context.theme is required');
  }
  const variables = new Map();
  for (const [field, token] of Object.entries(COLORS)) {
    if (typeof theme[field] !== 'string' || !/^#[\da-f]{6}(?:[\da-f]{2})?$/i.test(theme[field])) {
      throw new Error(`theme.${field} must be #rrggbb or #rrggbbaa`);
    }
    variables.set(`--pele-${token}`, theme[field]);
  }
  variables.set('--pele-line', theme.foreground);
  const series = [theme.accent, theme.muted, theme.danger, theme.border];
  for (let index = 0; index < 8; index++) {
    variables.set(`--pele-series-${index + 1}`, series[index % series.length]);
  }
  for (const [field, token, fallback] of [
    ['font_family', 'font', 'sans-serif'], ['font_family_mono', 'font-mono', 'monospace'],
  ]) {
    const value = theme[field] ?? fallback;
    if (typeof value !== 'string' || !value || value.length > 200 || !/^[\p{L}\p{N} _.,'"-]+$/u.test(value)) {
      throw new Error(`invalid theme.${field}`);
    }
    variables.set(`--pele-${token}`, value);
  }
  return variables;
}

export function resolveVariables(value, variables, seen = new Set()) {
  if (seen.size > 32) throw new Error('SVG variable nesting exceeds 32');
  let result = '';
  let position = 0;
  for (;;) {
    const start = value.indexOf('var(', position);
    if (start < 0) return result + value.slice(position);
    result += value.slice(position, start);
    let depth = 1;
    let end = start + 4;
    let comma = -1;
    for (; end < value.length && depth; end++) {
      if (value[end] === '(') depth++;
      else if (value[end] === ')') depth--;
      else if (value[end] === ',' && depth === 1 && comma < 0) comma = end;
    }
    if (depth) throw new Error('unterminated SVG variable');
    const name = value.slice(start + 4, comma < 0 ? end - 1 : comma).trim();
    if (!/^--[\w-]+$/.test(name)) throw new Error('invalid SVG variable name');
    if (seen.has(name)) throw new Error('cyclic SVG variable');
    const raw = variables.get(name) ?? (comma < 0 ? undefined : value.slice(comma + 1, end - 1));
    if (raw === undefined) throw new Error(`unresolved SVG variable ${name}`);
    result += resolveVariables(raw.trim(), variables, new Set([...seen, name]));
    position = end;
  }
}

function unescapeAttribute(value) {
  return value.replace(/&(quot|apos|amp|lt|gt);/g, (_, name) => ({
    quot: '"', apos: "'", amp: '&', lt: '<', gt: '>',
  })[name]);
}

function escapeAttribute(value) {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function declarations(style) {
  return style.split(';').flatMap(declaration => {
    const separator = declaration.indexOf(':');
    return separator < 0 ? [] : [[declaration.slice(0, separator).trim(), declaration.slice(separator + 1).trim()]];
  });
}

/** Materialize scoped aliases, fonts and SVG2 CSS rx into native SVG attributes. */
export function materializeSvg(svg, theme) {
  const root = { variables: themeVariables(theme), color: theme.foreground };
  const stack = [root];
  return svg.replace(/<[^>]*>/g, tag => {
    if (/^<\//.test(tag)) {
      if (stack.length > 1) stack.pop();
      return tag;
    }
    if (/^<[!?]/.test(tag)) return tag;
    const parent = stack.at(-1);
    const variables = new Map(parent.variables);
    const attributes = [...tag.matchAll(/([\w:-]+)="([^"]*)"/g)].map(match => [match[1], unescapeAttribute(match[2])]);
    const style = declarations(attributes.find(([key]) => key === 'style')?.[1] ?? '');
    for (const [key, value] of style) if (key.startsWith('--')) variables.set(key, value);
    let color = parent.color;
    const declaredColor = style.find(([key]) => key === 'color')?.[1]
      ?? attributes.find(([key]) => key === 'color')?.[1];
    if (declaredColor && declaredColor.toLowerCase() !== 'currentcolor') {
      color = resolveVariables(declaredColor, variables);
    }
    const resolve = value => resolveVariables(value, variables).replace(/\bcurrentColor\b/gi, color);
    const extra = new Map();
    const css = [];
    for (const [key, value] of style) {
      if (key.startsWith('--')) continue;
      const resolved = resolve(value);
      if (key === 'rx' || key === 'ry') extra.set(key, resolved);
      else if (key === 'font') {
        // resvg need not implement the CSS font shorthand to draw Pele labels.
        const size = resolved.match(/(?:^|\s)(\d+(?:\.\d+)?px)(?:\s|$)/)?.[1];
        if (size) extra.set('font-size', size);
      } else css.push(`${key}:${resolved}`);
    }
    const output = attributes.flatMap(([key, value]) => {
      if (key === 'style') return css.length ? [[key, css.join(';')]] : [];
      if (extra.has(key)) return [];
      return [[key, PRESENTATION.has(key) ? resolve(value) : value]];
    });
    output.push(...extra);
    const name = tag.match(/^<([\w:-]+)/)?.[1];
    if (!name) throw new Error('invalid Pele SVG tag');
    if (!tag.endsWith('/>')) stack.push({ variables, color });
    return `<${name}${output.map(([key, value]) => ` ${key}="${escapeAttribute(value)}"`).join('')}${tag.endsWith('/>') ? '/>' : '>'}`;
  });
}
