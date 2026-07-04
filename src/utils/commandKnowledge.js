const KNOWLEDGE_SCHEMA_VERSION = 1;

const SAFETY_LEVELS = new Set(['normal', 'sensitive', 'dangerous']);
const EXECUTION_POLICIES = new Set(['insertOnly', 'confirmBeforeExecute', 'blockDirectExecute']);

const normalizeText = (value) => String(value ?? '').trim();
const normalizeFolded = (value) => normalizeText(value).toLowerCase();

const uniqueTags = (tags) => {
  const seen = new Set();
  const result = [];
  const source = Array.isArray(tags) ? tags : String(tags || '').split(',');
  source.forEach((tag) => {
    const normalized = normalizeText(tag);
    if (!normalized) return;
    const key = normalized.toLowerCase();
    if (seen.has(key)) return;
    seen.add(key);
    result.push(normalized);
  });
  return result;
};

const createId = () => {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `knowledge-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
};

function normalizeKnowledgeEntry(raw = {}, now = Date.now()) {
  const title = normalizeText(raw.title || raw.name);
  const command = normalizeText(raw.command || raw.pattern);
  const safetyLevel = SAFETY_LEVELS.has(raw.safetyLevel) ? raw.safetyLevel : 'normal';
  const executionPolicy = EXECUTION_POLICIES.has(raw.executionPolicy)
    ? raw.executionPolicy
    : (safetyLevel === 'dangerous'
      ? 'blockDirectExecute'
      : safetyLevel === 'sensitive'
        ? 'confirmBeforeExecute'
        : 'insertOnly');

  return {
    id: normalizeText(raw.id) || createId(),
    title,
    command,
    trigger: normalizeFolded(raw.trigger || raw.shortcut || ''),
    tags: uniqueTags(raw.tags),
    description: normalizeText(raw.description || raw.note || ''),
    favorite: !!raw.favorite,
    safetyLevel,
    executionPolicy,
    usageCount: Math.max(0, Number(raw.usageCount || 0) || 0),
    lastUsedAt: Number.isFinite(Number(raw.lastUsedAt)) ? Number(raw.lastUsedAt) : null,
    createdAt: Number.isFinite(Number(raw.createdAt)) ? Number(raw.createdAt) : now,
    updatedAt: Number.isFinite(Number(raw.updatedAt)) ? Number(raw.updatedAt) : now,
    legacySource: raw.legacySource || undefined,
  };
}

const scoreRecent = (entry) => {
  if (!entry.lastUsedAt) return 0;
  return Math.min(25, Math.max(0, Math.floor(entry.lastUsedAt / 100000000000)));
};

const entryBoost = (entry) =>
  (entry.favorite ? 30 : 0) + Math.min(20, Number(entry.usageCount || 0)) + scoreRecent(entry);

function buildCommandKnowledgeIndex(entries = []) {
  const normalizedEntries = entries
    .map((entry) => normalizeKnowledgeEntry(entry))
    .filter((entry) => entry.title && entry.command);

  const entriesById = new Map();
  const triggerExactMap = new Map();
  const triggerRecords = [];
  const searchRows = normalizedEntries.map((entry) => {
    entriesById.set(entry.id, entry);
    if (entry.trigger) {
      if (!triggerExactMap.has(entry.trigger)) {
        triggerExactMap.set(entry.trigger, []);
      }
      triggerExactMap.get(entry.trigger).push(entry.id);
      triggerRecords.push({ trigger: entry.trigger, id: entry.id });
    }

    return {
      id: entry.id,
      titleText: normalizeFolded(entry.title),
      commandText: normalizeFolded(entry.command),
      triggerText: normalizeFolded(entry.trigger),
      tagText: normalizeFolded(entry.tags.join(' ')),
      descriptionText: normalizeFolded(entry.description),
    };
  });

  triggerRecords.sort((left, right) =>
    left.trigger.localeCompare(right.trigger) || left.id.localeCompare(right.id)
  );

  return {
    schemaVersion: KNOWLEDGE_SCHEMA_VERSION,
    entries: normalizedEntries,
    entriesById,
    searchRows,
    triggerExactMap,
    triggerRecords,
  };
}

function searchKnowledgeEntries(index, rawQuery, limit = 200) {
  const query = normalizeFolded(rawQuery);
  const entries = index?.entries || [];
  if (!query) {
    return [...entries].sort((left, right) => {
      const favorite = Number(!!right.favorite) - Number(!!left.favorite);
      if (favorite) return favorite;
      return Number(right.lastUsedAt || 0) - Number(left.lastUsedAt || 0);
    }).slice(0, limit);
  }

  const scored = [];
  for (const row of index?.searchRows || []) {
    let score = 0;
    if (row.triggerText === query) score = 10000;
    else if (row.triggerText.startsWith(query)) score = 8000;
    else if (row.titleText.includes(query)) score = 6000;
    else if (row.tagText.includes(query)) score = 4500;
    else if (row.commandText.includes(query) || row.descriptionText.includes(query)) score = 2500;
    if (!score) continue;

    const entry = index.entriesById.get(row.id);
    scored.push({ entry, score: score + entryBoost(entry) });
  }

  scored.sort((left, right) =>
    right.score - left.score ||
    Number(right.entry.lastUsedAt || 0) - Number(left.entry.lastUsedAt || 0) ||
    left.entry.title.localeCompare(right.entry.title)
  );

  return scored.slice(0, limit).map((item) => item.entry);
}

const lowerBoundTrigger = (records, query) => {
  let low = 0;
  let high = records.length;
  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    if (records[mid].trigger < query) low = mid + 1;
    else high = mid;
  }
  return low;
};

function matchKnowledgeTriggers(index, rawQuery, limit = 12) {
  const query = normalizeFolded(rawQuery);
  if (!query) return [];

  const ids = [];
  const seen = new Set();
  const exact = index?.triggerExactMap?.get(query) || [];
  exact.forEach((id) => {
    if (seen.has(id)) return;
    seen.add(id);
    ids.push(id);
  });

  const records = index?.triggerRecords || [];
  for (let cursor = lowerBoundTrigger(records, query); cursor < records.length; cursor += 1) {
    const record = records[cursor];
    if (!record.trigger.startsWith(query)) break;
    if (seen.has(record.id)) continue;
    seen.add(record.id);
    ids.push(record.id);
    if (ids.length >= limit) break;
  }

  return ids
    .map((id) => index.entriesById.get(id))
    .filter(Boolean)
    .sort((left, right) => {
      const exactRank = Number(right.trigger === query) - Number(left.trigger === query);
      if (exactRank) return exactRank;
      return entryBoost(right) - entryBoost(left) || left.trigger.localeCompare(right.trigger);
    })
    .slice(0, limit);
}

function migrateLegacyKnowledgeEntries({
  existingEntries = [],
  quickCommands = [],
  blacklistRules = [],
  now = Date.now(),
} = {}) {
  const migrated = existingEntries.map((entry) => normalizeKnowledgeEntry(entry, now));
  const existingLegacyIds = new Set(migrated.map((entry) => entry.id));

  quickCommands.forEach((command, index) => {
    const title = normalizeText(command?.name || command?.title);
    const commandText = normalizeText(command?.command);
    if (!title || !commandText) return;
    const id = `legacy-quick-${command?.id || index}`;
    if (existingLegacyIds.has(id)) return;
    migrated.push(normalizeKnowledgeEntry({
      id,
      title,
      command: commandText,
      trigger: '',
      tags: ['legacy'],
      legacySource: 'quick-command',
      createdAt: now,
      updatedAt: now,
    }, now));
  });

  blacklistRules.forEach((rule, index) => {
    const pattern = normalizeText(typeof rule === 'string' ? rule : rule?.pattern);
    if (!pattern) return;
    const severity = typeof rule === 'object' && rule?.severity === 'critical' ? 'critical' : 'warning';
    const id = `legacy-sensitive-${index}-${pattern}`;
    if (existingLegacyIds.has(id)) return;
    const dangerous = severity === 'critical';
    migrated.push(normalizeKnowledgeEntry({
      id,
      title: `${dangerous ? '高危' : '敏感'}规则：${pattern}`,
      command: pattern,
      trigger: '',
      tags: ['security', 'legacy'],
      safetyLevel: dangerous ? 'dangerous' : 'sensitive',
      executionPolicy: dangerous ? 'blockDirectExecute' : 'confirmBeforeExecute',
      legacySource: 'sensitive-rule',
      createdAt: now,
      updatedAt: now,
    }, now));
  });

  return migrated;
}

function deriveKnowledgeSensitiveRules(entries = []) {
  return entries
    .map((entry) => normalizeKnowledgeEntry(entry))
    .filter((entry) => entry.command && entry.safetyLevel !== 'normal')
    .sort((left, right) => {
      const severityRank = Number(right.safetyLevel === 'dangerous') - Number(left.safetyLevel === 'dangerous');
      if (severityRank) return severityRank;
      return left.title.localeCompare(right.title);
    })
    .map((entry) => ({
      pattern: entry.command,
      severity: entry.safetyLevel === 'dangerous' ? 'critical' : 'warning',
    }));
}

export {
  KNOWLEDGE_SCHEMA_VERSION,
  buildCommandKnowledgeIndex,
  deriveKnowledgeSensitiveRules,
  matchKnowledgeTriggers,
  migrateLegacyKnowledgeEntries,
  normalizeKnowledgeEntry,
  searchKnowledgeEntries,
};
