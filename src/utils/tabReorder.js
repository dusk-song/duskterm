// Rectangles use content coordinates so scrolling does not require measuring every tab again.
export function findTabDropTarget(rects, sourceId, x, y) {
  let nearest = null;
  let distance = Infinity;
  for (const rect of rects) {
    const dx = Math.max(rect.left - x, 0, x - rect.right);
    const dy = Math.max(rect.top - y, 0, y - rect.bottom);
    const score = dx * dx + dy * dy;
    if (score < distance) {
      distance = score;
      nearest = rect;
    }
  }
  if (!nearest || nearest.id === sourceId) return null;
  return { id: nearest.id, placement: x < (nearest.left + nearest.right) / 2 ? 'before' : 'after' };
}

export function moveSessionInList(sessions, sourceId, targetId, placement = 'before') {
  const sourceIndex = sessions.findIndex((session) => session.id === sourceId);
  const targetIndex = sessions.findIndex((session) => session.id === targetId);
  if (sourceIndex < 0 || targetIndex < 0 || sourceIndex === targetIndex) return false;
  if (sessions[sourceIndex].isSplitChild || sessions[targetIndex].isSplitChild) return false;
  const insertionIndex = targetIndex + (placement === 'after' ? 1 : 0) - (sourceIndex < targetIndex ? 1 : 0);
  if (insertionIndex === sourceIndex) return false;
  const [session] = sessions.splice(sourceIndex, 1);
  sessions.splice(insertionIndex, 0, session);
  return true;
}
