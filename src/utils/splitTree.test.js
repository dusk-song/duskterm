import test from 'node:test';
import assert from 'node:assert/strict';

import { collectSplitLeafIds, computeSplitLayout, createSplitLeaf, removeSplitLeaf, splitLeaf } from './splitTree.js';

test('splitLeaf supports mixed directions without changing unrelated branches', () => {
  let tree = createSplitLeaf('root');
  tree = splitLeaf(tree, 'root', 'child-a', 'vertical');
  tree = splitLeaf(tree, 'child-a', 'child-b', 'horizontal');

  assert.equal(tree.direction, 'vertical');
  assert.equal(tree.second.direction, 'horizontal');
  assert.deepEqual(collectSplitLeafIds(tree), ['root', 'child-a', 'child-b']);
});

test('removeSplitLeaf compresses the parent into the surviving sibling', () => {
  let tree = splitLeaf(createSplitLeaf('root'), 'root', 'child-a', 'vertical');
  tree = splitLeaf(tree, 'child-a', 'child-b', 'horizontal');

  const next = removeSplitLeaf(tree, 'child-a');
  assert.deepEqual(collectSplitLeafIds(next), ['root', 'child-b']);
  assert.equal(next.second.sessionId, 'child-b');
});

test('removeSplitLeaf returns null when the final leaf closes', () => {
  assert.equal(removeSplitLeaf(createSplitLeaf('root'), 'root'), null);
});

test('computeSplitLayout returns stable leaf rectangles and one divider per branch', () => {
  let tree = splitLeaf(createSplitLeaf('root'), 'root', 'right', 'vertical');
  tree = splitLeaf(tree, 'right', 'bottom-right', 'horizontal');
  const layout = computeSplitLayout(tree);

  assert.deepEqual(layout.leaves.map(item => item.sessionId), ['root', 'right', 'bottom-right']);
  assert.equal(layout.dividers.length, 2);
  assert.deepEqual(layout.leaves[0], { sessionId: 'root', x: 0, y: 0, width: 50, height: 100 });
  assert.deepEqual(layout.leaves[2], { sessionId: 'bottom-right', x: 50, y: 50, width: 50, height: 50 });
});
