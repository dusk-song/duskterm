const MAX_BLACKLIST_RULES = 200;
const MAX_PATTERN_LENGTH = 180;
const MAX_COMMAND_LENGTH = 4096;
const regexCache = new Map();

function sanitizeCommandText(value) {
  const text = String(value ?? '');
  const sanitized = text
    .replace(/\x1B\[[0-9;?]*[ -/]*[@-~]/g, '')
    .replace(/\x1B\][^\u0007]*(\u0007|\x1B\\)/g, '')
    .replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]/g, '')
    .trim();

  if (sanitized.length > MAX_COMMAND_LENGTH) {
    return sanitized.slice(0, MAX_COMMAND_LENGTH);
  }

  return sanitized;
}

function resolveBlacklistRules(blacklist) {
  const rules = Array.isArray(blacklist) ? blacklist : [];
  return rules
    .slice(0, MAX_BLACKLIST_RULES)
    .map((rule) => {
      if (typeof rule === 'string') {
        const pattern = rule.trim();
        if (!pattern) return null;
        if (pattern.length > MAX_PATTERN_LENGTH) return null;
        return { pattern, severity: 'warning' };
      }

      if (!rule || typeof rule !== 'object') return null;
      const pattern = String(rule.pattern || '').trim();
      if (!pattern) return null;
      if (pattern.length > MAX_PATTERN_LENGTH) return null;

      return {
        pattern,
        severity: rule.severity === 'critical' ? 'critical' : 'warning'
      };
    })
    .filter(Boolean);
}

function compileRuleRegex(pattern) {
  if (regexCache.has(pattern)) {
    return regexCache.get(pattern);
  }

  let compiled = null;
  try {
    compiled = new RegExp(pattern, 'i');
  } catch {
    compiled = null;
  }

  if (regexCache.size > 500) {
    const firstKey = regexCache.keys().next().value;
    if (firstKey) {
      regexCache.delete(firstKey);
    }
  }
  regexCache.set(pattern, compiled);
  return compiled;
}

function matchesRule(command, rule) {
  const regex = compileRuleRegex(rule.pattern);
  if (regex) {
    return regex.test(command);
  }

  return command.toLowerCase().includes(rule.pattern.toLowerCase());
}

function matchSensitiveCommand(content, blacklist) {
  const command = sanitizeCommandText(content);
  if (!command) return null;

  const rules = resolveBlacklistRules(blacklist);
  for (const rule of rules) {
    if (matchesRule(command, rule)) {
      return {
        content: command,
        severity: rule.severity
      };
    }
  }

  return null;
}

function findMatchedCommandInPayload(payload, blacklist) {
  const text = String(payload ?? '');
  const normalized = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  const lines = normalized.split('\n').map((line) => sanitizeCommandText(line)).filter(Boolean);

  for (const line of lines) {
    const matched = matchSensitiveCommand(line, blacklist);
    if (matched) {
      return matched;
    }
  }

  return null;
}

export {
  sanitizeCommandText,
  resolveBlacklistRules,
  matchSensitiveCommand,
  findMatchedCommandInPayload
};
