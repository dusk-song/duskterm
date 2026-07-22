const DEFAULT_HISTORY_LIMIT = 200;
const DEFAULT_HISTORY_MIN_LENGTH = 5;

const normalizeCount = (value) => Math.max(1, Math.floor(Number(value) || 1));

export const buildTerminalLineReplacementPayload = (command) => (
  `\u0015${String(command || '').trim()}`
);

export const normalizeCommandHistory = (entries = []) => {
  const normalized = [];
  const indexes = new Map();

  for (const entry of Array.isArray(entries) ? entries : []) {
    const cmd = String(entry?.cmd || '').trim();
    if (!cmd) continue;
    const existingIndex = indexes.get(cmd);
    if (existingIndex !== undefined) {
      normalized[existingIndex].count += normalizeCount(entry?.count);
      continue;
    }
    indexes.set(cmd, normalized.length);
    normalized.push({ cmd, count: normalizeCount(entry?.count) });
  }

  return normalized;
};

export const findCommandHistoryMatches = (
  history,
  rawQuery,
  { excludedCommands = [], limit = 10 } = {},
) => {
  const query = String(rawQuery || '').trim().toLowerCase();
  if (!query) return [];
  const excluded = new Set(Array.from(excludedCommands || [], (command) => String(command || '')));

  return normalizeCommandHistory(history)
    .map((entry, index) => ({ ...entry, index }))
    .filter((entry) => entry.cmd.toLowerCase().startsWith(query) && !excluded.has(entry.cmd))
    .sort((left, right) => right.count - left.count || right.index - left.index)
    .slice(0, Math.max(0, Number(limit) || 0))
    .map(({ index: _index, ...entry }) => entry);
};

export const recordCommandHistoryEntry = (
  history,
  command,
  { max = DEFAULT_HISTORY_LIMIT, minLength = DEFAULT_HISTORY_MIN_LENGTH } = {},
) => {
  const text = String(command || '').trim();
  const entries = normalizeCommandHistory(history);
  if (text.length < minLength) return entries;

  const existingIndex = entries.findIndex((entry) => entry.cmd === text);
  const count = existingIndex >= 0 ? entries[existingIndex].count + 1 : 1;
  if (existingIndex >= 0) entries.splice(existingIndex, 1);
  entries.push({ cmd: text, count });

  const cappedMax = Math.max(1, Number(max) || DEFAULT_HISTORY_LIMIT);
  return entries.slice(-cappedMax);
};

export const extractCommandFromTerminalLine = (line, fallbackInput = '') => {
  const text = String(line || '').replace(/\u00a0/g, ' ').trimEnd();
  const fallback = String(fallbackInput || '').trim();
  if (!text) return fallback;

  const promptPatterns = [
    /^\s*PS\s+[^>]+>\s*(.+)$/s,
    /^\s*[A-Za-z]:\\[^>]*>\s*(.+)$/s,
    /^\s*[\w.-]+@[\w.-]+(?::[^#$%]*)?[#$%]\s*(.+)$/s,
    /^\s*(?:~|\/|[^\s]+\/)[^#$%]*[#$%]\s*(.+)$/s,
    /^\s*[#$%>]\s*(.+)$/s,
  ];

  for (const pattern of promptPatterns) {
    const match = text.match(pattern);
    if (match?.[1]) return match[1].trim();
  }

  const firstToken = fallback.match(/^\S+/)?.[0] || '';
  if (firstToken) {
    const escaped = firstToken.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const tokenPattern = new RegExp(`(?:^|\\s)(${escaped}(?:\\s|$))`, 'g');
    let match;
    let commandStart = -1;
    while ((match = tokenPattern.exec(text)) !== null) {
      commandStart = match.index + (match[0].length - match[1].length);
    }
    if (commandStart >= 0) return text.slice(commandStart).trim();
  }

  if (fallback) {
    const fallbackIndex = text.lastIndexOf(fallback);
    if (fallbackIndex >= 0) return text.slice(fallbackIndex).trim();
  }

  return fallback;
};
