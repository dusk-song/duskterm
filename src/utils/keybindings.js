const MOUSE_BUTTON_NAMES = Object.freeze({
  3: 'Mouse4',
  4: 'Mouse5'
});

export function normalizeMouseBindingEvent(event) {
  const buttonName = MOUSE_BUTTON_NAMES[event?.button];
  if (!buttonName) return '';

  const parts = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.shiftKey) parts.push('Shift');
  if (event.altKey) parts.push('Alt');
  if (event.metaKey) parts.push('Meta');
  parts.push(buttonName);
  return parts.join('+');
}
