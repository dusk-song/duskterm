import { cleanTerminalContext, MAX_AI_CONTEXT_BYTES } from './aiContext.js';

export const DEFAULT_CONTEXT_ROWS = 200;
export const MAX_CONTEXT_ROWS = 500;
const MAX_SCAN_CELLS = 2_000_000;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function utf8Tail(bytes, limit) {
  let start = Math.max(0, bytes.length - limit);
  while (start < bytes.length && (bytes[start] & 0xc0) === 0x80) start++;
  return decoder.decode(bytes.subarray(start));
}

// Public xterm buffer APIs only. Synchronous, bounded copying keeps a snapshot
// coherent: do not yield between rows while the terminal is still printing.
export function captureTerminalSnapshot(term, { mode = 'recent', lines = DEFAULT_CONTEXT_ROWS } = {}) {
  if (!['recent', 'visible'].includes(mode)) throw new Error('终端采集范围无效');
  if (!Number.isInteger(lines) || lines < 1 || lines > MAX_CONTEXT_ROWS) throw new Error('采集行数需在 1–500 行之间');
  const buffer = term?.buffer?.active;
  if (!buffer || !buffer.length) throw new Error('终端内容暂不可用');
  const alternate = buffer.type === 'alternate';
  // Alternate screens have no ordinary scrollback. Include the whole visible
  // picture even when the app has moved its cursor back to the top-left corner.
  const visible = mode === 'visible' || alternate;
  const end = Math.min(buffer.length - 1, visible
    ? buffer.viewportY + term.rows - 1 : buffer.baseY + buffer.cursorY);
  const requestedStart = visible ? buffer.viewportY : Math.max(0, end - lines + 1);
  const start = Math.max(requestedStart, end - MAX_CONTEXT_ROWS + 1);
  const capturedAt = new Date();
  const rows = [];
  let used = 0, scanned = 0, cropped = start > requestedStart;
  const nextWrapped = !!buffer.getLine(end + 1)?.isWrapped;
  let followingWrapped = nextWrapped;
  for (let index = end; index >= start; index--) {
    const line = buffer.getLine(index);
    if (!line) throw new Error('终端画面已变化，请重新采集');
    scanned += line.length;
    if (scanned > MAX_SCAN_CELLS) { cropped = true; break; }
    const separator = rows.length && !followingWrapped ? 1 : 0;
    const room = MAX_AI_CONTEXT_BYTES - used - separator;
    if (room <= 0) { cropped = true; break; }
    // A wide glyph can wrap early, leaving a null cell at the right edge.
    // Remove only null padding, keeping real spaces and wide-cell continuations.
    let endColumn = line.length;
    if (followingWrapped) {
      while (endColumn > 0) {
        const cell = line.getCell?.(endColumn - 1);
        if (!cell || cell.getChars() || cell.getWidth() === 0) break;
        endColumn--;
      }
    }
    const bytes = encoder.encode(cleanTerminalContext(line.translateToString(!followingWrapped, 0, endColumn)));
    const clipped = bytes.length > room;
    const text = clipped ? utf8Tail(bytes, room) : decoder.decode(bytes);
    if (clipped && !text) { cropped = true; break; }
    rows.push({ index, text, isWrapped: !!line.isWrapped });
    used += encoder.encode(text).length + separator;
    followingWrapped = !!line.isWrapped;
    if (clipped) { cropped = true; break; }
  }
  if (!rows.length) throw new Error('终端内容过宽，请缩小范围或使用文本选区');
  rows.reverse();
  const text = rows.map((row, i) => `${i && !row.isWrapped ? '\n' : ''}${row.text}`).join('').replace(/\s+$/u, '');
  if (!text.trim()) throw new Error('当前采集范围没有可用文本');
  const scope = alternate ? '备用屏幕当前画面' : mode === 'visible' ? '当前可见区域' : `最近 ${lines} 显示行（截至光标行）`;
  const notes = [];
  if (cropped) notes.push('受行数、字节或扫描上限限制，较早部分已截断');
  else if (!visible && requestedStart > 0) notes.push('更早输出未包含');
  if (rows[0].isWrapped) notes.push('首行从折行中段开始');
  if (nextWrapped) notes.push('末行的后续折行未包含');
  if (alternate) notes.push('不是普通命令历史');
  const range = `${scope}；第 ${rows[0].index + 1}–${end + 1} 显示行，共 ${rows.length} 行${notes.length ? `；${notes.join('；')}` : ''}`;
  return { text, range, capturedAt, truncated: cropped, start: rows[0].index, end, alternate };
}
