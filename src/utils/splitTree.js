export const createSplitLeaf = (sessionId) => ({ type: 'leaf', sessionId });

export const createSplitBranch = (direction, first, second, ratio = 0.5) => ({
  type: 'split',
  direction,
  ratio,
  first,
  second
});

export function collectSplitLeafIds(node, result = []) {
  if (!node) return result;
  if (node.type === 'leaf') {
    result.push(node.sessionId);
    return result;
  }
  collectSplitLeafIds(node.first, result);
  collectSplitLeafIds(node.second, result);
  return result;
}

export function replaceSplitLeaf(node, targetId, replacement) {
  if (!node) return null;
  if (node.type === 'leaf') return node.sessionId === targetId ? replacement : node;
  return {
    ...node,
    first: replaceSplitLeaf(node.first, targetId, replacement),
    second: replaceSplitLeaf(node.second, targetId, replacement)
  };
}

export function splitLeaf(node, targetId, newSessionId, direction) {
  return replaceSplitLeaf(
    node,
    targetId,
    createSplitBranch(direction, createSplitLeaf(targetId), createSplitLeaf(newSessionId))
  );
}

export function removeSplitLeaf(node, targetId) {
  if (!node) return null;
  if (node.type === 'leaf') return node.sessionId === targetId ? null : node;
  const first = removeSplitLeaf(node.first, targetId);
  const second = removeSplitLeaf(node.second, targetId);
  if (!first) return second;
  if (!second) return first;
  if (first === node.first && second === node.second) return node;
  return { ...node, first, second };
}

export function computeSplitLayout(node) {
  const leaves = [];
  const dividers = [];

  const visit = (current, rect) => {
    if (!current) return;
    if (current.type === 'leaf') {
      leaves.push({ sessionId: current.sessionId, ...rect });
      return;
    }
    const ratio = Number.isFinite(current.ratio) ? Math.min(0.85, Math.max(0.15, current.ratio)) : 0.5;
    if (current.direction === 'vertical') {
      const firstWidth = rect.width * ratio;
      visit(current.first, { x: rect.x, y: rect.y, width: firstWidth, height: rect.height });
      visit(current.second, { x: rect.x + firstWidth, y: rect.y, width: rect.width - firstWidth, height: rect.height });
      dividers.push({ direction: 'vertical', x: rect.x + firstWidth, y: rect.y, width: 0, height: rect.height, node: current, bounds: { ...rect } });
      return;
    }
    const firstHeight = rect.height * ratio;
    visit(current.first, { x: rect.x, y: rect.y, width: rect.width, height: firstHeight });
    visit(current.second, { x: rect.x, y: rect.y + firstHeight, width: rect.width, height: rect.height - firstHeight });
    dividers.push({ direction: 'horizontal', x: rect.x, y: rect.y + firstHeight, width: rect.width, height: 0, node: current, bounds: { ...rect } });
  };

  visit(node, { x: 0, y: 0, width: 100, height: 100 });
  return { leaves, dividers };
}
