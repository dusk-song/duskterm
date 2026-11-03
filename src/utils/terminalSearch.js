export function stringOffsetToColumn(line, offset) {
  let length = 0;
  for (let col = 0; col < line.length; col++) {
    if (length >= offset) return col;
    const cell = line.getCell(col);
    if (cell?.getWidth() === 0) continue;
    length += cell?.getChars().length || 1;
    if (length >= offset) return col + (cell?.getWidth() || 1);
  }
  return line.length;
}

// Public xterm APIs only. One marker tracks history trimming; cached coordinates
// use monotonically increasing row IDs instead of shifting on every scroll.
export class TerminalSearch {
  constructor({ onInvalidate = () => {}, visible = () => true, workerFactory, timeoutMs = 5000 } = {}) {
    this.onInvalidate = onInvalidate;
    this.timeoutMs = timeoutMs;
    this.visible = visible;
    this.workerFactory = workerFactory || (() => new Worker(new URL('./terminalSearchWorker.js', import.meta.url), { type: 'module' }));
    this.origin = 0;
    this.revision = 0;
    this.generation = 0;
    this.dirty = 0;
    this.matches = new Uint32Array();
    this.current = -1;
    this.disposables = [];
    this.decorations = [];
    this.id = 0;
    this.reset = true;
  }

  activate(term) {
    this.term = term;
    this.buffer = term.buffer.active;
    this.baseY = this.buffer.baseY;
    this.end = this.buffer.length;
    this.trackTrim();
    this.disposables.push(
      term.onWriteParsed(() => this.invalidate()),
      term.onResize(() => this.invalidate(true)),
      term.buffer.onBufferChange(() => this.invalidate(true)),
      term.onScroll(() => this.scheduleDecorations()),
    );
  }

  trackTrim() {
    this.marker?.dispose();
    this.marker = this.term.registerMarker(0);
    this.markerRow = this.marker?.line;
  }

  invalidate(full = false) {
    if (!this.term) return;
    const buffer = this.term.buffer.active;
    full ||= buffer !== this.buffer || (this.marker && this.marker.isDisposed);
    let trimmed = 0;
    if (!full && this.marker) trimmed = Math.max(0, this.markerRow - this.marker.line);
    this.origin += trimmed;
    this.dirty = full ? this.origin : Math.min(this.dirty, this.origin + Math.max(0, this.baseY - trimmed));
    this.end = this.origin + buffer.length;
    this.baseY = buffer.baseY;
    this.buffer = buffer;
    this.reset ||= full;
    this.revision++;
    this.key = null;
    if (full) { this.generation++; this.cancelQuery(); }
    else if (this.pending) this.pending.dirty = Math.min(this.pending.dirty, this.dirty);
    this.clearHighlights();
    this.trackTrim();
    if (this.worker) this.scheduleSync();
    this.onInvalidate();
  }

  ensureWorker() {
    if (this.worker) return;
    this.worker = this.workerFactory();
    this.dirty = this.origin;
    this.reset = true;
    const worker = this.worker;
    worker.onmessage = ({ data }) => {
      if (this.worker !== worker) return;
      if (!this.pending || data.id !== this.pending.id) return;
      const pending = this.pending;
      this.pending = null;
      clearTimeout(this.timeout);
      if (pending.generation !== this.generation) pending.resolve(null);
      else if (data.error) pending.reject(new Error(data.error));
      else {
        let matches = data.matches;
        if (data.revision !== this.revision) {
          // Never select a result on a row overwritten since the snapshot.
          const kept = new Uint32Array(matches.length);
          let used = 0;
          for (let i = 0; i < matches.length; i += 4) {
            if (matches[i] >= this.origin && matches[i + 2] < pending.dirty) {
              kept.set(matches.subarray(i, i + 4), used); used += 4;
            }
          }
          matches = kept.slice(0, used);
        }
        pending.resolve({ matches, revision: data.revision });
      }
    };
    worker.onerror = () => {
      if (this.worker === worker) this.failWorker('搜索线程异常，请重新搜索');
    };
  }

  failWorker(message) {
    const pending = this.pending;
    this.pending = null;
    clearTimeout(this.timeout);
    this.worker?.terminate();
    this.worker = null;
    this.key = null;
    this.dirty = this.origin;
    this.reset = true;
    this.syncReady?.forEach(resolve => resolve());
    this.syncReady = [];
    pending?.reject(new Error(message));
  }

  cancelQuery() {
    clearTimeout(this.timeout);
    if (this.pending) {
      this.pending.resolve(null);
      this.pending = null;
      this.worker?.postMessage({ type: 'cancel' });
    }
  }

  scheduleSync() {
    if (this.syncTimer || !this.worker) return;
    this.syncTimer = true;
    if (typeof MessageChannel !== 'undefined') {
      if (!this.syncChannel) {
        this.syncChannel = new MessageChannel();
        this.syncChannel.port1.onmessage = () => {
          this.syncTimer = false;
          this.syncSlice();
          if (!this.syncTimer) {
            this.syncChannel?.port1.unref?.();
            this.syncChannel?.port2.unref?.();
          }
        };
      }
      this.syncChannel.port1.ref?.();
      this.syncChannel.port2.ref?.();
      this.syncChannel.port2.postMessage(null);
    } else {
      this.syncTimer = setTimeout(() => { this.syncTimer = null; this.syncSlice(); }, 0);
    }
  }

  syncSlice() {
    if (!this.worker || !this.term) return;
    const start = Math.max(this.origin, this.dirty);
    let row = start;
    const rows = [];
    const began = performance.now();
    while (row < this.end && rows.length < 2048 && performance.now() - began < 4) {
      const index = row - this.origin;
      const line = this.buffer.getLine(index);
      const next = this.buffer.getLine(index + 1);
      const wraps = !!next?.isWrapped;
      let text = line?.translateToString(!wraps) || '';
      if (wraps && line) {
        const last = line.getCell(line.length - 1);
        if (last?.getCode() === 0 && last.getWidth() === 1 && next.getCell(0)?.getWidth() === 2) text = text.slice(0, -1);
      }
      rows.push({ text, wrapped: !!line?.isWrapped });
      row++;
    }
    this.worker.postMessage({ type: 'patch', origin: this.origin, end: this.end, start, rows, reset: this.reset, revision: this.revision });
    this.reset = false;
    this.dirty = row;
    if (row < this.end) this.scheduleSync();
    else { const ready = this.syncReady; this.syncReady = []; ready?.forEach(resolve => resolve()); }
  }

  async synchronize() {
    this.ensureWorker();
    if (this.dirty >= this.end && !this.reset) return;
    await new Promise(resolve => {
      (this.syncReady ||= []).push(resolve);
      this.scheduleSync();
    });
  }

  async find(term, options, direction = 0) {
    const request = ++this.id;
    const key = JSON.stringify([term, options.caseSensitive, options.regex, options.wholeWord, this.revision]);
    if (this.key !== key) {
      this.cancelQuery();
      await this.synchronize();
      if (request !== this.id || !this.term) return null;
      const response = await new Promise((resolve, reject) => {
        this.pending = { id: request, resolve, reject, generation: this.generation, dirty: Infinity };
        this.timeout = setTimeout(() => this.failWorker('搜索耗时过长，请简化正则表达式后重试'), this.timeoutMs);
        this.worker.postMessage({ type: 'search', id: request, term, options: {
          caseSensitive: options.caseSensitive, regex: options.regex, wholeWord: options.wholeWord,
        } });
      });
      if (!response || request !== this.id) return null;
      const { matches, revision } = response;
      this.matches = matches;
      this.key = JSON.stringify([term, options.caseSensitive, options.regex, options.wholeWord, revision]);
      const selection = this.term.getSelectionPosition()?.start;
      const startRow = (selection?.y ?? this.buffer.viewportY) + this.origin;
      const startColumn = selection?.x ?? 0;
      this.current = this.lowerBound(startRow);
      // Keep the selected match when refreshing an existing query.
      while (this.current < matches.length / 4 && matches[this.current * 4] === startRow
        && this.coordinate(this.current).col < startColumn) this.current++;
      if (direction < 0) this.current--;
      if (this.current >= matches.length / 4) this.current = 0;
      if (this.current < 0) this.current = matches.length / 4 - 1;
    } else if (direction && this.matches.length) {
      const count = this.matches.length / 4;
      this.current = (this.current + direction + count) % count;
    }
    const count = this.matches.length / 4;
    this.colors = options.decorations;
    // Output-driven refreshes update counts/highlights without disturbing input
    // or a user's selection. Check ownership when async results arrive, since
    // the user may have returned to the terminal while the worker was busy.
    const reveal = typeof options.reveal === 'function' ? options.reveal() : options.reveal !== false;
    if (reveal) {
      if (count) {
        const match = this.coordinate(this.current);
        this.term.select(match.col, match.row, match.size);
        if (match.row < this.buffer.viewportY || match.row >= this.buffer.viewportY + this.term.rows) {
          this.term.scrollToLine(Math.max(0, match.row - Math.floor(this.term.rows / 2)));
        }
      } else this.term.clearSelection();
    }
    this.drawDecorations();
    return { count, current: count ? this.current + 1 : 0, stale: this.key !== JSON.stringify([term, options.caseSensitive, options.regex, options.wholeWord, this.revision]) };
  }

  lowerBound(row) {
    let lo = 0, hi = this.matches.length / 4;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (this.matches[mid * 4] < row) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  coordinate(index) {
    const i = index * 4;
    const row = this.matches[i] - this.origin;
    const endRow = this.matches[i + 2] - this.origin;
    const line = this.buffer.getLine(row), endLine = this.buffer.getLine(endRow);
    const col = line ? stringOffsetToColumn(line, this.matches[i + 1]) : 0;
    const endCol = endLine ? stringOffsetToColumn(endLine, this.matches[i + 3]) : 0;
    return { row, col, size: (endRow - row) * this.term.cols + endCol - col };
  }

  scheduleDecorations() {
    if (this.raf || !this.key || !this.visible()) return;
    this.raf = requestAnimationFrame(() => { this.raf = null; this.drawDecorations(); });
  }

  drawDecorations() {
    this.clearHighlights();
    if (!this.term || !this.colors || !this.key || !this.visible()) return;
    const first = this.buffer.viewportY, last = first + this.term.rows;
    // Include a bounded number of earlier matches that may wrap into view.
    let index = Math.max(0, this.lowerBound(this.origin + first) - 200);
    while (index < this.matches.length / 4 && this.decorations.length < 200) {
      const match = this.coordinate(index);
      if (match.row >= last) break;
      let row = match.row, col = match.col, remaining = match.size;
      const active = index === this.current;
      if (row < first) {
        remaining -= (first - row) * this.term.cols - col;
        row = first; col = 0;
      }
      while (remaining > 0 && row < last && this.decorations.length < 200) {
        const width = Math.min(this.term.cols - col, remaining);
        if (row >= first && width > 0) {
          const marker = this.term.registerMarker(row - this.buffer.baseY - this.buffer.cursorY);
          if (marker) {
            const decoration = this.term.registerDecoration({ marker, x: col, width,
              backgroundColor: active ? this.colors.activeMatchBackground : this.colors.matchBackground });
            if (decoration) this.decorations.push({ dispose() { decoration.dispose(); marker.dispose(); } });
            else marker.dispose();
          }
        }
        remaining -= width; row++; col = 0;
      }
      index++;
    }
  }

  clearHighlights() {
    this.decorations.forEach(item => item.dispose());
    this.decorations = [];
  }

  clearDecorations() {
    this.id++;
    this.cancelQuery();
    this.clearHighlights();
    this.key = null;
    this.matches = new Uint32Array();
    this.current = -1;
  }

  dispose() {
    this.clearDecorations();
    if (!this.syncChannel) clearTimeout(this.syncTimer);
    this.syncChannel?.port1.close();
    this.syncChannel?.port2.close();
    if (this.raf) cancelAnimationFrame(this.raf);
    this.disposables.forEach(item => item.dispose());
    this.marker?.dispose();
    this.worker?.terminate();
    this.syncReady?.forEach(resolve => resolve());
    this.worker = null;
    this.term = null;
  }
}
