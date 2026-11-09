import test from 'node:test';
import assert from 'node:assert/strict';
import { captureTerminalSnapshot } from './terminalContextSnapshot.js';
import { buildTerminalContext, MAX_AI_CONTEXT_BYTES } from './aiContext.js';

function terminal(values, { cols = 80, cursor = values.length - 1, viewport = 0, rows = 3, type = 'normal' } = {}) {
  const reads = [];
  const buffer = { length: values.length, baseY: 0, cursorY: cursor, viewportY: viewport, type,
    getLine(index) {
      reads.push(index);
      if (index < 0 || index >= values.length) return;
      const value = typeof values[index] === 'string' ? { text: values[index] } : values[index];
      return { length: cols, isWrapped: !!value.wrapped,
        getCell: col => ({ getChars: () => value.text[col] || '', getWidth: () => 1 }),
        translateToString: (trim, start = 0, end = cols) => {
          const text = value.text.padEnd(cols, ' ').slice(start, end);
          return trim ? text.trimEnd() : text;
        } };
    } };
  return { rows, cols, buffer: { active: buffer }, reads };
}

test('近期输出仅读取最后限定行数，不扫描全部历史', () => {
  const t = terminal(Array.from({ length: 100000 }, (_, i) => `row-${i}`));
  const result = captureTerminalSnapshot(t);
  assert.equal(result.start, 99800);
  assert(result.text.startsWith('row-99800\n'));
  assert(result.text.endsWith('row-99999'));
  assert(t.reads.length <= 201);
  assert.match(result.range, /最近 200.*更早输出未包含/);
});
test('可见区域使用滚动位置而不是当前输出底部', () => {
  const t = terminal(Array.from({ length: 100 }, (_, i) => `row-${i}`), { viewport: 10, rows: 3 });
  assert.equal(captureTerminalSnapshot(t, { mode: 'visible' }).text, 'row-10\nrow-11\nrow-12');
});
test('普通近期输出截至光标，排除未使用的屏幕空行', () => {
  const t = terminal(['one', 'two', '', '', ''], { cursor: 1, rows: 5 });
  assert.equal(captureTerminalSnapshot(t).text, 'one\ntwo');
});
test('折行拼接保留空格和宽字符，独立行保留换行', () => {
  const t = terminal(['中文  ', { text: '继续', wrapped: true }, '  stack'], { cols: 8 });
  assert.equal(captureTerminalSnapshot(t).text, '中文  继续\n  stack');
});
test('首尾处于折行中段时说明边界，不向范围外追溯整条长行', () => {
  const t = terminal(['left', { text: 'middle', wrapped: true }, { text: 'right', wrapped: true }], { viewport: 1, rows: 1, cols: 6 });
  const result = captureTerminalSnapshot(t, { mode: 'visible' });
  assert.equal(result.text, 'middle');
  assert.match(result.range, /首行从折行中段开始.*末行的后续折行未包含/);
  assert(!t.reads.includes(0));
});
test('宽字符提前折行留下的空单元格不变成正文空格，真实空格仍保留', () => {
  assert.equal(captureTerminalSnapshot(terminal(['abc', { text: '中', wrapped: true }], { cols: 4 })).text, 'abc中');
  assert.equal(captureTerminalSnapshot(terminal(['abc ', { text: '中', wrapped: true }], { cols: 4 })).text, 'abc 中');
});
test('备用屏幕即使光标在顶部也采集当前画面，不读取普通历史', () => {
  const t = terminal(['header', 'process-a', 'process-b'], { type: 'alternate', cursor: 0, rows: 3 });
  const result = captureTerminalSnapshot(t);
  assert.equal(result.text, 'header\nprocess-a\nprocess-b');
  assert.match(result.range, /备用屏幕当前画面.*不是普通命令历史/);
});
test('64 KiB 截断保留最近文本，不切坏 UTF-8 汉字或 emoji', () => {
  const t = terminal(Array.from({ length: 500 }, (_, i) => `${i}:` + '中🙂'.repeat(100)), { cols: 400 });
  const result = captureTerminalSnapshot(t, { lines: 500 });
  assert(new TextEncoder().encode(result.text).length <= MAX_AI_CONTEXT_BYTES);
  assert(result.text.includes('499:'));
  assert(result.truncated);
  assert(!result.text.includes('\ufffd'));
  assert.match(result.range, /已截断/);
  assert.doesNotThrow(() => buildTerminalContext(result.text, { id: 'test' }, result.capturedAt, result.range));
});
test('超高的可见区域仍受 500 行上限限制', () => {
  const t = terminal(Array.from({ length: 1000 }, (_, i) => `${i}`), { rows: 1000 });
  const result = captureTerminalSnapshot(t, { mode: 'visible' });
  assert.equal(result.start, 500);
  assert(t.reads.length <= 501);
  assert(result.truncated);
});
test('极宽画面的扫描也有上限，不因空白行无限处理', () => {
  const t = terminal(Array(500).fill('x'), { cols: 10000 });
  const result = captureTerminalSnapshot(t, { lines: 500 });
  assert(t.reads.length <= 202);
  assert(result.truncated);
});
test('空白范围和消失的行明确失败', () => {
  assert.throws(() => captureTerminalSnapshot(terminal(['', '  '])), /没有可用文本/);
  const t = terminal(['a', 'b']);
  t.buffer.active.getLine = () => undefined;
  assert.throws(() => captureTerminalSnapshot(t), /已变化/);
});
test('非法行数和范围不会触发全历史读取', () => {
  const t = terminal(['a']);
  for (const lines of [0, 501, 1.5]) assert.throws(() => captureTerminalSnapshot(t, { lines }), /行数/);
  assert.throws(() => captureTerminalSnapshot(t, { mode: 'all' }), /范围/);
  assert.equal(t.reads.length, 0);
});
test('再次采集生成新快照，不修改旧文本和范围', () => {
  const values = ['before'];
  const t = terminal(values);
  const before = captureTerminalSnapshot(t);
  values[0] = 'after';
  const after = captureTerminalSnapshot(t);
  assert.equal(before.text, 'before');
  assert.equal(after.text, 'after');
  assert(before.capturedAt instanceof Date);
});
