const DEFAULT_HISTORY_LIMIT = 200;
const DEFAULT_HISTORY_MIN_LENGTH = 5;

const normalizeCount = (value) => Math.max(1, Math.floor(Number(value) || 1));

const toCharacters = (value) => Array.from(String(value ?? ''));

export const createTerminalInputState = (text = '') => {
  const characters = toCharacters(text);
  return {
    text: characters.join(''),
    cursor: characters.length,
    reliable: true,
  };
};

export const updateTerminalInputState = (state, data) => {
  const current = state && typeof state === 'object' ? state : createTerminalInputState();
  const characters = toCharacters(current.text);
  let cursor = Math.max(0, Math.min(characters.length, Number(current.cursor) || 0));
  let reliable = current.reliable !== false;
  const payload = String(data ?? '');

  if (payload === '\u007f' || payload === '\b') {
    if (cursor > 0) characters.splice(--cursor, 1);
  } else if (payload === '\x1b[3~') {
    if (cursor < characters.length) characters.splice(cursor, 1);
  } else if (payload === '\x1b[D') {
    cursor = Math.max(0, cursor - 1);
  } else if (payload === '\x1b[C') {
    cursor = Math.min(characters.length, cursor + 1);
  } else if (['\x1b[H', '\x1bOH', '\x1b[1~', '\u0001'].includes(payload)) {
    cursor = 0;
  } else if (['\x1b[F', '\x1bOF', '\x1b[4~', '\u0005'].includes(payload)) {
    cursor = characters.length;
  } else if (payload === '\u0017') {
    while (cursor > 0 && /\s/u.test(characters[cursor - 1])) characters.splice(--cursor, 1);
    while (cursor > 0 && !/\s/u.test(characters[cursor - 1])) characters.splice(--cursor, 1);
  } else if (payload === '\u0015') {
    characters.splice(0, cursor);
    cursor = 0;
  } else if (payload === '\u000b') {
    characters.splice(cursor);
  } else if (payload === '\u0003') {
    return createTerminalInputState();
  } else if (payload === '\x1b[A' || payload === '\x1b[B' || payload === '\t') {
    reliable = false;
  } else if (!payload.startsWith('\x1b') && !/[\u0000-\u001F\u007F]/u.test(payload)) {
    const inserted = toCharacters(payload);
    characters.splice(cursor, 0, ...inserted);
    cursor += inserted.length;
  } else {
    reliable = false;
  }

  return { text: characters.join(''), cursor, reliable };
};

export const replaceTerminalInputState = (text, reliable = true) => ({
  ...createTerminalInputState(text),
  reliable,
});

export const extractAnchoredTerminalInput = (line, prefix) => {
  const text = String(line ?? '').replace(/\u00a0/g, ' ');
  const anchor = String(prefix ?? '').replace(/\u00a0/g, ' ');
  if (!text.startsWith(anchor)) return { text: '', reliable: false };
  return { text: text.slice(anchor.length), reliable: true };
};

const SENSITIVE_HISTORY_PATTERNS = [
  /(?:^|\s)--?(?:password|passwd|passphrase|token|api[-_]?key|secret|authorization)(?:\s|=|:|$)/iu,
  /(?:^|\s)[A-Z0-9_]*(?:PASSWORD|PASSWD|PASSPHRASE|TOKEN|API_KEY|SECRET|AUTHORIZATION)[A-Z0-9_]*\s*=/iu,
  /\b(?:authorization|proxy-authorization|x-api-key)\s*:/iu,
  /(?:^|\s)(?:mysql|mariadb)\b[^\r\n]*\s-p\S+/iu,
  /(?:^|\s)curl\b[^\r\n]*(?:-u|--user)\s+[^\s:]+:[^\s]+/iu,
];

export const isRecordableCommandHistory = (command, maxChars = 4096) => {
  const text = String(command ?? '').trim();
  if (!text || /[\r\n]/u.test(text) || Array.from(text).length > maxChars) return false;
  return !SENSITIVE_HISTORY_PATTERNS.some((pattern) => pattern.test(text));
};

export const buildTerminalLineReplacementPayload = (command, currentInput = '') => {
  const eraseCurrentInput = '\u007f'.repeat(toCharacters(currentInput).length);
  return `\u0005${eraseCurrentInput}${String(command || '').trim()}`;
};

export const normalizeCommandHistory = (entries = []) => {
  const normalized = [];
  const indexes = new Map();

  for (const entry of Array.isArray(entries) ? entries : []) {
    const cmd = String(entry?.cmd || '').trim();
    if (!cmd) continue;
    const normalizedEntry = {
      id: Number(entry?.id || 0),
      cmd,
      count: normalizeCount(entry?.count),
      lastUsedAt: Math.max(0, Number(entry?.lastUsedAt || 0)),
    };
    const existingIndex = indexes.get(cmd);
    if (existingIndex !== undefined) {
      normalized[existingIndex].count += normalizedEntry.count;
      if (normalizedEntry.lastUsedAt >= normalized[existingIndex].lastUsedAt) {
        normalized[existingIndex].id = normalizedEntry.id || normalized[existingIndex].id;
        normalized[existingIndex].lastUsedAt = normalizedEntry.lastUsedAt;
      }
      continue;
    }
    indexes.set(cmd, normalized.length);
    normalized.push(normalizedEntry);
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
    .sort((left, right) => (
      right.count - left.count
      || right.lastUsedAt - left.lastUsedAt
      || right.index - left.index
    ))
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
  const existingId = existingIndex >= 0 ? entries[existingIndex].id || 0 : 0;
  if (existingIndex >= 0) entries.splice(existingIndex, 1);
  entries.push({
    id: existingId,
    cmd: text,
    count,
    lastUsedAt: Date.now(),
  });

  const cappedMax = Math.max(1, Number(max) || DEFAULT_HISTORY_LIMIT);
  return entries.slice(-cappedMax);
};
