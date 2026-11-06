// UI-only readers registered by mounted terminals. No server/model access.
const readers = new Map();

export function registerTerminalContextReader(id, reader) {
  readers.set(id, reader);
  return () => { if (readers.get(id) === reader) readers.delete(id); };
}

export function getTerminalContextReader(id) {
  const reader = readers.get(id);
  if (!reader) throw new Error('当前活动终端尚未就绪，请选择一个终端后重试');
  return reader;
}
