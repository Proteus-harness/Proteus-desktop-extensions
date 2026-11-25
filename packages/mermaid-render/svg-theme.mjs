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

function requireTheme(theme) {
  if (!theme || typeof theme !== 'object' || Array.isArray(theme)) {
    throw new Error('content.context.theme is required');
  }
}

export function themeVariables(theme) {
  requireTheme(theme);
  return buildThemeVariables(theme);
}

function buildThemeVariables(theme) {
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

export function resolveVariables(value, variables, seen = undefined) {
  // A caller-provided history keeps the original immutable copying semantics.
  return expandVariables(value, variables, seen, seen === undefined);
}

function expandVariables(value, variables, seen, privateHistory) {
  if (seen !== undefined && seen.size > 32) throw new Error('SVG variable nesting exceeds 32');
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
    if (seen !== undefined && seen.has(name)) throw new Error('cyclic SVG variable');
    const raw = variables.get(name) ?? (comma < 0 ? undefined : value.slice(comma + 1, end - 1));
    if (raw === undefined) throw new Error(`unresolved SVG variable ${name}`);
    const text = raw.trim();
    const reuse = privateHistory && seen !== undefined;
    const trail = reuse ? seen : new Set(seen === undefined ? [name] : [...seen, name]);
    if (reuse) trail.add(name);
    try {
      result += expandVariables(text, variables, trail, privateHistory);
    } finally {
      if (reuse) trail.delete(name);
    }
    position = end;
  }
}

function unescapeAttribute(value) {
  return value.replace(/&(quot|apos|amp|lt|gt);/g, (_, name) => ({
    quot: '"', apos: "'", amp: '&', lt: '<', gt: '>',
  })[name]);
}

const ATTRIBUTE_SPECIALS = /[&"<>]/g;
const ATTRIBUTE_ENTITIES = { '&': '&amp;', '"': '&quot;', '<': '&lt;', '>': '&gt;' };
const attributeEntity = character => ATTRIBUTE_ENTITIES[character];

function escapeAttribute(value) {
  return value.replace(ATTRIBUTE_SPECIALS, attributeEntity);
}

function encodeAttribute(key, value) {
  return ` ${key}="${escapeAttribute(value)}"`;
}

const NO_DECLARATIONS = Object.freeze([]);

function declarations(style) {
  if (style === '') return NO_DECLARATIONS;
  const entries = [];
  for (const declaration of style.split(';')) {
    const separator = declaration.indexOf(':');
    if (separator >= 0) entries.push([declaration.slice(0, separator).trim(), declaration.slice(separator + 1).trim()]);
  }
  return entries;
}

/** Prepare the wire render from single, validated values, not caller-owned state. */
export function prepareRenderTheme(theme) {
  requireTheme(theme);
  const snapshot = Object.create(null);
  for (const field of Object.keys(COLORS)) snapshot[field] = theme[field];
  snapshot.font_family = theme.font_family;
  snapshot.font_family_mono = theme.font_family_mono;
  const variables = buildThemeVariables(snapshot);
  const color = variables.get('--pele-fg');
  return {
    fontFamily: variables.get('--pele-font'),
    fontFamilyMono: variables.get('--pele-font-mono'),
    materialize: svg => materializeWithTheme(svg, variables, color),
  };
}

/** Materialize scoped aliases, fonts and SVG2 CSS rx into native SVG attributes. */
export function materializeSvg(svg, theme) {
  return materializeWithTheme(svg, themeVariables(theme), theme.foreground);
}

function materializeWithTheme(svg, variables, color) {
  const root = { variables, color };
  const stack = [root];
  return svg.replace(/<[^>]*>/g, tag => {
    if (/^<\//.test(tag)) {
      if (stack.length > 1) stack.pop();
      return tag;
    }
    if (/^<[!?]/.test(tag)) return tag;
    const parent = stack.at(-1);
    const attributes = [];
    let inlineStyle;
    let inlineColor;
    for (const match of tag.matchAll(/([\w:-]+)="([^"]*)"/g)) {
      const key = match[1];
      const value = unescapeAttribute(match[2]);
      attributes.push([key, value]);
      if (key === 'style') inlineStyle ??= value;
      else if (key === 'color') inlineColor ??= value;
    }
    const style = declarations(inlineStyle ?? '');
    let variables = parent.variables;
    for (const [key, value] of style) {
      if (!key.startsWith('--')) continue;
      // Inherited values are read-only until this element introduces its own.
      if (variables === parent.variables) variables = new Map(variables);
      variables.set(key, value);
    }
    let color = parent.color;
    const declaredColor = style.find(([key]) => key === 'color')?.[1] ?? inlineColor;
    if (declaredColor && declaredColor.toLowerCase() !== 'currentcolor') {
      color = resolveVariables(declaredColor, variables);
    }
    const resolve = value => resolveVariables(value, variables).replace(/\bcurrentColor\b/gi, color);
    // Most tags have no emitted style. Only allocate output scratch for
    // declarations that actually survive alias resolution.
    let extra;
    let css;
    for (const [key, value] of style) {
      if (key.startsWith('--')) continue;
      const resolved = resolve(value);
      if (key === 'rx' || key === 'ry') (extra ??= new Map()).set(key, resolved);
      else if (key === 'font') {
        // resvg need not implement the CSS font shorthand to draw Pele labels.
        const size = resolved.match(/(?:^|\s)(\d+(?:\.\d+)?px)(?:\s|$)/)?.[1];
        if (size) (extra ??= new Map()).set('font-size', size);
      } else (css ??= []).push(`${key}:${resolved}`);
    }
    const output = [];
    for (const [key, value] of attributes) {
      if (key === 'style') {
        if (css) output.push(encodeAttribute(key, css.join(';')));
      } else if (!extra?.has(key)) {
        output.push(encodeAttribute(key, PRESENTATION.has(key) ? resolve(value) : value));
      }
    }
    if (extra) {
      for (const [key, value] of extra) output.push(encodeAttribute(key, value));
    }
    const name = tag.match(/^<([\w:-]+)/)?.[1];
    if (!name) throw new Error('invalid Pele SVG tag');
    if (!tag.endsWith('/>')) {
      // An unchanged inherited scope still gets a stack slot, not a new frame.
      stack.push(variables === parent.variables && color === parent.color ? parent : { variables, color });
    }
    return `<${name}${output.join('')}${tag.endsWith('/>') ? '/>' : '>'}`;
  });
}
