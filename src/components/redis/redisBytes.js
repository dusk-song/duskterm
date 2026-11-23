const utf8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });
const utf8Encoder = new TextEncoder();
const maxStringBytes = 65536;

export const decodeBytes = text => Uint8Array.from(atob(text), char => char.charCodeAt(0));
export function complete(value) {
  if (!value) return false;
  const padding = value.base64.endsWith('==') ? 2 : value.base64.endsWith('=') ? 1 : 0;
  return value.base64.length / 4 * 3 - padding === value.length;
}
function encodeBytes(bytes) {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}
export const encodeText = text => encodeBytes(utf8Encoder.encode(text));
export function readText(value) {
  if (!value) return null;
  try {
    const bytes = decodeBytes(value.base64);
    if (bytes.length === value.length) {
      const text = utf8.decode(bytes);
      if (!/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/.test(text)) return text;
    }
  } catch { /* The byte viewer provides the lossless representation. */ }
  return null;
}
export function byteLabel(value, text = readText(value)) {
  return text != null && !/[\x00-\x1f\x7f]/.test(text) ? text || '（空）' : '二进制 · ' + (value?.length ?? 0) + ' B';
}
export const decodeText = bytes => utf8.decode(bytes);

export function formatStringInput(value, format) {
  if (format === 'base64') return value.base64;
  if (format === 'hex') {
    return Array.from(decodeBytes(value.base64), byte => byte.toString(16).padStart(2, '0')).join(' ');
  }
  const text = readText(value);
  return text === null || text.includes('\r') ? null : text;
}

export function validateStringInput({ format, text }) {
  const tooLarge = { error: 'String 值最多支持写入 64 KiB。' };
  if (text.length > maxStringBytes * 4) return tooLarge;
  let bytes;
  if (format === 'utf8') {
    bytes = utf8Encoder.encode(text);
  } else {
    const compact = text.replace(/[ \t\r\n]/g, '');
    if (format === 'hex') {
      if (compact.length > maxStringBytes * 2) return tooLarge;
      if (compact.length % 2 || /[^0-9a-f]/i.test(compact)) {
        return { error: '请输入完整的十六进制字节对。' };
      }
      bytes = new Uint8Array(compact.length / 2);
      for (let index = 0; index < bytes.length; index += 1) {
        bytes[index] = Number.parseInt(compact.slice(index * 2, index * 2 + 2), 16);
      }
    } else {
      if (compact.length > Math.ceil(maxStringBytes / 3) * 4) return tooLarge;
      const invalid = { error: '请输入标准 Base64，并保留末尾所需的等号。' };
      if (compact.length % 4 || !/^[A-Za-z0-9+/]*={0,2}$/.test(compact)) return invalid;
      try {
        bytes = decodeBytes(compact);
      } catch {
        return invalid;
      }
      if (encodeBytes(bytes) !== compact) return invalid;
      if (bytes.length > maxStringBytes) return tooLarge;
      return { base64: compact, length: bytes.length };
    }
  }
  if (bytes.length > maxStringBytes) return tooLarge;
  return { base64: encodeBytes(bytes), length: bytes.length };
}

export function hex(bytes) {
  const rows = [];
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const row = [...bytes.subarray(offset, offset + 16)];
    rows.push([
      offset.toString(16).padStart(8, '0'),
      row.map(byte => byte.toString(16).padStart(2, '0')).join(' ').padEnd(47),
      row.map(byte => byte >= 32 && byte < 127 ? String.fromCharCode(byte) : '.').join(''),
    ].join('  '));
  }
  return rows.join('\n');
}

export function formatJson(text) {
  JSON.parse(text);
  const tokens = text.match(/"(?:\\.|[^"\\])*"|[^{}\[\],:\s"]+|[{}\[\],:]/g);
  let depth = 0;
  let size = text.length;
  const indent = () => '\n' + '  '.repeat(depth);
  return tokens.map((token, index) => {
    size += depth * 2 + 2;
    if (depth > 64 || size > 524288) throw new Error('JSON 格式化超出预览上限');
    if (token === '{' || token === '[') {
      depth += 1;
      return token + (['}', ']'].includes(tokens[index + 1]) ? '' : indent());
    }
    if (token === '}' || token === ']') {
      depth -= 1;
      return (['{', '['].includes(tokens[index - 1]) ? '' : indent()) + token;
    }
    if (token === ',') return token + indent();
    if (token === ':') return ': ';
    return token;
  }).join('');
}

export async function decompressGzip(bytes) {
  const reader = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip')).getReader();
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let size = 0;
  let text = '';
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) return text + decoder.decode();
      size += value.byteLength;
      if (size > 65536) throw new Error('解压结果超过预览上限');
      text += decoder.decode(value, { stream: true });
    }
  } finally {
    await reader.cancel().catch(() => {});
  }
}

export function commandArgs(text) {
  const args = [];
  let quote = '';
  let escape = false;
  let started = false;
  let token = '';
  for (const char of text) {
    if (escape) { token += char; escape = false; started = true; }
    else if (char === '\\') { escape = true; }
    else if (quote) {
      if (char === quote) quote = '';
      else token += char;
    } else if (char === "'" || char === '"') { quote = char; started = true; }
    else if (/\s/.test(char)) {
      if (started) args.push(token);
      started = false;
      token = '';
    } else { token += char; started = true; }
  }
  if (quote || escape) throw new Error('命令包含未闭合的引号或转义');
  if (started) args.push(token);
  return args;
}
