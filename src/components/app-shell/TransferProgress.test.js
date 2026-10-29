import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { compileScript, parse } from 'vue/compiler-sfc';
import { createRenderer, h, nextTick, ref } from 'vue';

const { descriptor } = parse(readFileSync(new URL('./TransferProgress.vue', import.meta.url), 'utf8'));
const compiled = compileScript(descriptor, { id: 'transfer-progress-test', inlineTemplate: true }).content
  .replace(/from ['"]vue['"]/g, `from ${JSON.stringify(import.meta.resolve('vue'))}`)
  .replace(/from ['"]@\/utils\/([^'"]+)['"]/g, (_, name) => (
    `from ${JSON.stringify(new URL(`../../utils/${name}.js`, import.meta.url).href)}`
  ));
const { default: TransferProgress } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

async function mountProgress(t, initial) {
  const frames = new Map();
  const listeners = new Set();
  const original = Object.fromEntries(['document', 'requestAnimationFrame', 'cancelAnimationFrame'].map((key) => [key, globalThis[key]]));
  let nextFrameId = 0;
  let stamp = performance.now();
  let patches = 0;
  globalThis.document = {
    hidden: false,
    addEventListener: (_, callback) => listeners.add(callback),
    removeEventListener: (_, callback) => listeners.delete(callback),
  };
  globalThis.requestAnimationFrame = (callback) => { frames.set(++nextFrameId, callback); return nextFrameId; };
  globalThis.cancelAnimationFrame = (id) => frames.delete(id);
  const makeNode = (tag) => ({ tag, children: [], style: { display: '' }, attrs: {}, textContent: '',
    setAttribute(key, value) { this.attrs[key] = value; } });
  const renderer = createRenderer({
    createElement: makeNode, createText: (text) => ({ text }), createComment: () => ({}),
    setText: (node, text) => { node.text = text; },
    setElementText: (node, text) => { node.textContent = text; },
    insert(node, parent) { parent.children.push(node); node.parent = parent; },
    remove(node) { if (node.parent) node.parent.children = node.parent.children.filter((child) => child !== node); },
    parentNode: (node) => node.parent, nextSibling: () => null,
    patchProp(node, key, _, value) { patches++; node.attrs[key] = value; },
  });
  const item = ref({ name: 'demo.bin', loaded: 0, total: 100, progress: 0, status: 'waiting', ...initial });
  const root = makeNode('root');
  const app = renderer.createApp({ setup: () => () => h(TransferProgress, { item: item.value }) });
  app.mount(root);
  const row = root.children[0];
  t.after(() => {
    app.unmount();
    for (const [key, value] of Object.entries(original)) {
      if (value === undefined) delete globalThis[key];
      else globalThis[key] = value;
    }
  });
  return {
    app, item, row, frames, listeners,
    get patches() { return patches; },
    step(count = 1) {
      for (let index = 0; index < count; index++) {
        stamp += 16;
        const queued = [...frames.values()];
        frames.clear();
        queued.forEach((callback) => callback(stamp));
      }
    },
    async update(patch) { item.value = { ...item.value, ...patch }; await nextTick(); },
  };
}

test('component smooths first samples, caps finalizing, and finishes without changing backend state', async (t) => {
  const view = await mountProgress(t);
  await view.update({ status: 'transferring', loaded: 100, progress: 100 });
  const patches = view.patches;
  view.step();
  assert.equal(view.row.children[1].textContent, '2%');
  view.step(150);
  assert.equal(view.row.children[1].textContent, '99%');
  assert.equal(view.frames.size, 0);
  assert.equal(view.patches, patches, 'animation frames must not patch the Vue subtree');
  await view.update({ status: 'finalizing' });
  assert.equal(view.row.children[1].textContent, '99%');
  await view.update({ status: 'success' });
  assert.equal(view.item.value.status, 'success');
  view.step(150);
  assert.equal(view.row.children[1].textContent, '100%');
  await new Promise((resolve) => setTimeout(resolve, 220));
  await nextTick();
  assert.equal(view.row.style.display, 'none');
});

test('failure and cancellation stop animation immediately', async (t) => {
  const view = await mountProgress(t);
  for (const status of ['failed', 'cancelled', 'skipped']) {
    await view.update({ status: 'transferring', loaded: 80 });
    assert.equal(view.frames.size, 1);
    await view.update({ status });
    assert.equal(view.frames.size, 0);
    assert.equal(view.row.style.display, 'none');
  }
});

test('historical completed rows never animate', async (t) => {
  const view = await mountProgress(t, { status: 'success', loaded: 100 });
  assert.equal(view.frames.size, 0);
  assert.equal(view.listeners.size, 0);
  assert.equal(view.row.style.display, 'none');
});

test('hidden windows snap to the last target and unmount releases all work', async (t) => {
  const view = await mountProgress(t);
  await view.update({ status: 'transferring', loaded: 80 });
  document.hidden = true;
  view.listeners.forEach((callback) => callback());
  assert.equal(view.frames.size, 0);
  assert.equal(view.row.children[1].textContent, '80%');
  document.hidden = false;
  view.listeners.forEach((callback) => callback());
  await view.update({ loaded: 90 });
  assert.equal(view.frames.size, 1);
  view.app.unmount();
  assert.equal(view.frames.size, 0);
  assert.equal(view.listeners.size, 0);
});
