// A single writer preserves byte/escape-sequence order. Parsing is independent
// of animation frames; xterm already schedules rendering and yields its parser.
export function createTerminalOutput({ write, onParsed = () => {} }) {
  let chunks = [];
  let head = 0;
  let queued = 0;
  let writing = false;
  let disposed = false;
  let limit = 32 * 1024;
  let idleWaiters = [];
  let timer = null;
  const stats = { writes: 0, chars: 0, maxQueuedChars: 0, lastParseMs: 0 };

  function flush() {
    timer = null;
    if (disposed || writing || !queued) return;
    const parts = [];
    let size = 0;
    while (head < chunks.length && size < limit) {
      const chunk = chunks[head];
      let take = Math.min(chunk.length, limit - size);
      if (take < chunk.length && /[\uD800-\uDBFF]/.test(chunk[take - 1])) take--;
      if (!take) break;
      parts.push(chunk.slice(0, take));
      size += take;
      queued -= take;
      if (take === chunk.length) chunks[head++] = null;
      else chunks[head] = chunk.slice(take);
    }
    if (head === chunks.length) { chunks = []; head = 0; }
    else if (head > 256) { chunks = chunks.slice(head); head = 0; }
    writing = true;
    const started = performance.now();
    write(parts.join(''), () => {
      writing = false;
      if (disposed) return;
      stats.writes++;
      stats.chars += size;
      stats.lastParseMs = performance.now() - started;
      if (stats.lastParseMs < 6) limit = Math.min(128 * 1024, limit * 2);
      else if (stats.lastParseMs > 12) limit = Math.max(8 * 1024, limit / 2);
      onParsed();
      if (queued) timer = setTimeout(flush, 0);
      else { const waiters = idleWaiters; idleWaiters = []; waiters.forEach(resolve => resolve()); }
    });
  }

  return {
    stats,
    enqueue(text) {
      if (disposed || !text) return;
      chunks.push(text);
      queued += text.length;
      stats.maxQueuedChars = Math.max(stats.maxQueuedChars, queued);
      // A short echo enters xterm immediately, without a frame-sized delay.
      if (!writing && timer === null) flush();
    },
    idle() {
      if (disposed || (!writing && !queued)) return Promise.resolve();
      return new Promise(resolve => idleWaiters.push(resolve));
    },
    dispose() {
      disposed = true;
      clearTimeout(timer);
      chunks = [];
      queued = 0;
      idleWaiters.forEach(resolve => resolve());
      idleWaiters = [];
    },
  };
}

// Notifications coalesce while data is pending. Do not pull the next binary
// batch until the previous batch has been consumed, providing renderer credit.
export async function attachTerminalOutput({ sessionId, invoke, listen, consume }) {
  let disposed = false;
  let reading = null;
  let notified = false;
  function drain() {
    notified = true;
    if (disposed) return Promise.resolve();
    if (reading) return reading;
    reading = (async () => {
      try {
        do {
          notified = false;
          while (!disposed) {
            const result = await invoke('read_terminal_output', { sessionId });
            if (disposed) break;
            const bytes = result instanceof Uint8Array ? result : new Uint8Array(result);
            if (!bytes.length) break;
            await consume(bytes);
          }
        } while (notified && !disposed);
      } finally { reading = null; }
    })();
    return reading;
  }
  const unlisten = await listen(`terminal-output-ready-${sessionId}`, () => {
    void drain().catch(error => console.error('Terminal output read failed:', error));
  });
  try {
    await invoke('attach_terminal_output', { sessionId });
    void drain().catch(error => console.error('Terminal output read failed:', error));
  } catch (error) { unlisten(); throw error; }
  const detach = async () => {
    disposed = true;
    unlisten();
    await invoke('detach_terminal_output', { sessionId });
  };
  detach.flush = drain;
  return detach;
}
