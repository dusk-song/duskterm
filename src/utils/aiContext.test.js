import test from 'node:test';
import assert from 'node:assert/strict';
import { appendTerminalContext, buildTerminalContext, cleanTerminalContext, supportsTerminalTextContext } from './aiContext.js';

const source = { id: 'terminal-a', name: '测试服务器', password: 'must-not-leak', host: 'private-host' };
test('终端清理保留脚本缩进、换行和宽字符，移除 ANSI/OSC 和控制字符', () => {
  assert.equal(cleanTerminalContext('\x1b]0;title\x07\x1b[31m错误\x1b[0m\r\n\t  code\x00'), '错误\n\t  code');
});
test('来源只取允许字段，快照保留采集时刻而非当前终端状态', () => {
  const result = buildTerminalContext('failure\n  stack', source, new Date('2026-11-06T01:02:03Z'));
  assert.match(result.text, /2026-11-06T01:02:03.000Z/);
  assert.match(result.text, /failure\n  stack/);
  assert.doesNotMatch(result.text, /must-not-leak|private-host/);
});
test('空选区和清理后为空的选区拒绝导入', () => {
  for (const text of ['', undefined, '\x1b[31m\x00']) assert.throws(() => buildTerminalContext(text, source), /选择/);
});
test('64 KiB 限制按 UTF-8 字节而非字符计算，超限不裁切', () => {
  assert.doesNotThrow(() => buildTerminalContext('a'.repeat(65536), source));
  assert.throws(() => buildTerminalContext('中'.repeat(22000), source), /64 KiB/);
});
test('插入保留完整草稿和代码，不自动执行或添加问题', () => {
  assert.equal(appendTerminalContext('原问题', '选区'), '原问题\n\n选区\n\n');
});
test('合并后同时校验输入框字符限制和后端字节限制', () => {
  assert.throws(() => appendTerminalContext('a'.repeat(127999), 'text'), /草稿过长/);
  assert.throws(() => appendTerminalContext('中'.repeat(86000), 'text'), /草稿过长/);
});
test('来源标记随正文保存，可跨来源追加而不改写旧文本', () => {
  const text = buildTerminalContext('a', source).text;
  const another = buildTerminalContext('b', { id: 'terminal-b' }).text;
  const merged = appendTerminalContext(text, another);
  assert(merged.includes('[终端来源：terminal-a]'));
  assert(merged.includes('[终端来源：terminal-b]'));
});
test('本地 PowerShell/CMD 与 SSH 共用文本链路，其他协议保留原范围', () => {
  for (const protocol of ['local', 'ssh', undefined, 'LOCAL']) assert.equal(supportsTerminalTextContext(protocol), true);
  for (const protocol of ['serial', 'telnet', 'unknown']) {
    assert.equal(supportsTerminalTextContext(protocol), false);
    assert.throws(() => buildTerminalContext('output', { ...source, protocol }), /终端类型/);
  }
});
test('本地来源区分 PowerShell 和 CMD，只保留允许的类型名称', () => {
  for (const shell of ['PowerShell', 'CMD']) {
    const local = buildTerminalContext('错误\r\n  detail', { ...source, protocol: 'local', shell, env: { TOKEN: 'env-secret' }, workingDirectory: 'private-directory' });
    assert(local.text.includes(`终端类型：本地终端 · ${shell}`));
    assert.doesNotMatch(local.text, /must-not-leak|private-host|env-secret|private-directory/);
    assert(local.text.includes('错误\n  detail'));
    assert(local.text.includes('[终端来源：terminal-a]'));
  }
  assert(buildTerminalContext('output', source).text.includes('终端类型：SSH'));
});
