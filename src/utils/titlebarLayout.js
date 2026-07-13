const MENU_KEYS = ['file', 'edit', 'view', 'connection', 'help'];

export function resolveTitlebarVisibility(width) {
  const value = Number(width) || 0;
  let menuCount = 5;
  if (value < 940) menuCount = 4;
  if (value < 860) menuCount = 3;
  if (value < 780) menuCount = 2;
  if (value < 700) menuCount = 1;
  if (value < 620) menuCount = 0;
  return {
    monitor: value >= 1280,
    transfer: value >= 1080,
    menus: MENU_KEYS.slice(0, menuCount),
  };
}
