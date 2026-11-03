// Keep the hard limit aligned with src-tauri/src/terminal_input.rs.
export const MAX_TERMINAL_INPUT_BYTES = 4 * 1024 * 1024;
export const LARGE_PASTE_BYTES = 64 * 1024;
const encoder = new TextEncoder();

export function describeLargeTerminalPaste(data) {
  if (typeof data !== 'string' || data.length < LARGE_PASTE_BYTES / 4) return null;
  const bytes = encoder.encode(data).length;
  if (bytes < LARGE_PASTE_BYTES) return null;
  const text = data.replace(/^\x1b\[200~/, '').replace(/\x1b\[201~$/, '');
  let lines = 1;
  for (let i = 0; i < text.length; i++) {
    if (text[i] === '\r') { lines++; if (text[i + 1] === '\n') i++; }
    else if (text[i] === '\n') lines++;
  }
  return {
    bytes,
    lines,
    tooLarge: bytes > MAX_TERMINAL_INPUT_BYTES,
    preview: text.slice(0, 1200).replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]/g, ''),
    truncated: text.length > 1200,
  };
}
