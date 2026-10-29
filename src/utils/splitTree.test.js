import assert from 'node:assert/strict';
import test from 'node:test';
import { reactive, watch } from 'vue';
import { computeSplitLayout, createSplitBranch, createSplitLeaf, getSplitLeafStyle } from './splitTree.js';

test('drag previews move a nested divider without reactive updates or snapshot changes', () => {
  const tree = reactive(createSplitBranch('vertical', createSplitLeaf('main'),
    createSplitBranch('horizontal', createSplitLeaf('top'), createSplitLeaf('bottom'))));
  const snapshot = JSON.stringify(tree);
  let updates = 0;
  const stop = watch(tree, () => { updates++; }, { deep: true, flush: 'sync' });
  for (let index = 15; index <= 85; index++) {
    const layout = computeSplitLayout(tree, { node: tree.second, ratio: index / 100 });
    assert.ok(Math.abs(layout.leaves[1].height - index) < 1e-9);
    assert.ok(Math.abs(layout.leaves[2].height - (100 - index)) < 1e-9);
    assert.equal(layout.leaves[0].width, 50);
  }
  assert.equal(updates, 0);
  assert.equal(JSON.stringify(tree), snapshot);
  tree.second.ratio = 0.7;
  assert.equal(updates, 1);
  assert.equal(computeSplitLayout(tree).leaves[1].height, 70);
  stop();
});

test('preview clamps extreme pointer positions and restores the original geometry', () => {
  const tree = createSplitBranch('vertical', createSplitLeaf('a'), createSplitLeaf('b'));
  assert.equal(computeSplitLayout(tree, { node: tree, ratio: -1 }).leaves[0].width, 15);
  assert.equal(computeSplitLayout(tree, { node: tree, ratio: 2 }).leaves[0].width, 85);
  assert.equal(computeSplitLayout(tree).leaves[0].width, 50);
  assert.deepEqual(computeSplitLayout(tree, { node: tree, ratio: 0.5 }), computeSplitLayout(tree));
});

test('preview and committed leaves share gap geometry without shrinking outside edges', () => {
  const tree = createSplitBranch('vertical', createSplitLeaf('a'), createSplitLeaf('b'));
  const [left, right] = computeSplitLayout(tree).leaves.map(getSplitLeafStyle);
  assert.equal(left.left, 'calc(0% + 0px)');
  assert.equal(left.width, 'calc(50% - 4px)');
  assert.equal(right.left, 'calc(50% + 4px)');
  assert.equal(right.width, 'calc(50% - 4px)');
  assert.equal(left.height, 'calc(100% - 0px)');
});
