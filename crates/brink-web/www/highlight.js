import { Decoration } from '@codemirror/view';
import { RangeSetBuilder } from '@codemirror/state';

const decoCache = new Map();

function getDecoForType(typeName) {
  if (!decoCache.has(typeName)) {
    decoCache.set(typeName, Decoration.mark({ class: 'tok-' + typeName }));
  }
  return decoCache.get(typeName);
}

export function buildDecorations(source, doc, typeNames, semanticTokensFn) {
  const builder = new RangeSetBuilder();

  let tokens;
  try {
    tokens = semanticTokensFn(source);
  } catch {
    return builder.finish();
  }

  if (!tokens || tokens.length === 0) {
    return builder.finish();
  }

  // Collect decorations with positions, then sort by from position
  // (RangeSetBuilder requires sorted input)
  const decos = [];
  for (const t of tokens) {
    const typeName = typeNames[t.token_type];
    if (!typeName) continue;

    // t.line is 0-based, CM6 doc.line() is 1-based
    const lineNum = t.line + 1;
    if (lineNum < 1 || lineNum > doc.lines) continue;

    const line = doc.line(lineNum);
    const from = line.from + t.start_char;
    const to = from + t.length;

    if (from < line.from || to > line.to) continue;

    decos.push({ from, to, deco: getDecoForType(typeName) });
  }

  // Sort by position (required by RangeSetBuilder)
  decos.sort((a, b) => a.from - b.from || a.to - b.to);

  for (const { from, to, deco } of decos) {
    builder.add(from, to, deco);
  }

  return builder.finish();
}
