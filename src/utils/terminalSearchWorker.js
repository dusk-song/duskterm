// Only text and integer coordinates cross the worker boundary. Terminal cells,
// DOM decorations and selected command contents never leave the webview.
const SEPARATORS = " ~!@#$%^&*()+`-=[]{}|\\;:\"',./<>?";

export function createSearchPattern(term, options) {
  const source = options.regex ? term : term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(source, options.caseSensitive ? 'g' : 'gi');
}

export function matchLogicalLine(text, regex, wholeWord, emit) {
  regex.lastIndex = 0;
  let match;
  while ((match = regex.exec(text))) {
    const length = match[0].length;
    if (length && (!wholeWord || (
      (match.index === 0 || SEPARATORS.includes(text[match.index - 1])) &&
      (match.index + length === text.length || SEPARATORS.includes(text[match.index + length]))
    ))) emit(match.index, length);
    // Preserve overlapping matches and ensure zero-length regexes terminate.
    regex.lastIndex = match.index + 1;
    if (regex.lastIndex > text.length) break;
  }
}

export function locateTextOffset(offsets, position) {
  let low = 0, high = offsets.length - 1;
  while (low < high) {
    const mid = Math.ceil((low + high) / 2);
    if (offsets[mid] <= position) low = mid;
    else high = mid - 1;
  }
  return [low, position - offsets[low]];
}

if (typeof WorkerGlobalScope !== 'undefined' && self instanceof WorkerGlobalScope) {
  let rows = new Map();
  let origin = 0, end = 0, revision = 0, job = 0;
  self.onmessage = ({ data }) => {
    if (data.type === 'patch') {
      if (data.reset) job++;
      if (data.reset) rows = new Map();
      for (let i = origin; i < data.origin; i++) rows.delete(i);
      origin = data.origin;
      if (data.end < end) for (let i = data.end; i < end; i++) rows.delete(i);
      end = data.end;
      revision = data.revision;
      for (let i = 0; i < data.rows.length; i++) rows.set(data.start + i, data.rows[i]);
      return;
    }
    if (data.type === 'cancel') { job++; return; }
    if (data.type !== 'search') return;
    const token = ++job;
    const resultRevision = revision;
    // A query sees an immutable snapshot while new output updates the live cache.
    const searchRows = new Map(rows);
    const searchEnd = end;
    let regex;
    try { regex = createSearchPattern(data.term, data.options); }
    catch { self.postMessage({ id: data.id, revision, error: 'invalid-regex' }); return; }
    let row = origin;
    let parts = [], offsets = [], logicalStart = row, length = 0;
    // Chunked typed storage avoids millions of boxed coordinate numbers.
    let block = new Uint32Array(16384), used = 0;
    const blocks = [];
    let count = 0;
    const push = (a, b, c, d) => {
      if (used === block.length) { blocks.push(block); block = new Uint32Array(16384); used = 0; }
      block.set([a, b, c, d], used); used += 4; count++;
    };
    const slice = () => {
      if (token !== job) return;
      const started = performance.now();
      while (row < searchEnd && performance.now() - started < 6) {
        const entry = searchRows.get(row);
        if (!entry) { row++; continue; }
        if (!parts.length) logicalStart = row;
        offsets.push(length);
        parts.push(entry.text);
        length += entry.text.length;
        row++;
        if (row < searchEnd && searchRows.get(row)?.wrapped) continue;
        const text = parts.join('');
        matchLogicalLine(text, regex, data.options.wholeWord, (index, size) => {
          const [sr, sc] = locateTextOffset(offsets, index);
          const [er, ec] = locateTextOffset(offsets, index + size);
          push(logicalStart + sr, sc, logicalStart + er, ec);
        });
        parts = []; offsets = []; length = 0;
      }
      if (row < searchEnd) { setTimeout(slice, 0); return; }
      const matches = new Uint32Array(count * 4);
      let offset = 0;
      for (const chunk of blocks) { matches.set(chunk, offset); offset += chunk.length; }
      matches.set(block.subarray(0, used), offset);
      self.postMessage({ id: data.id, revision: resultRevision, matches }, [matches.buffer]);
    };
    slice();
  };
}
