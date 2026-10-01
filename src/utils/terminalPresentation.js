const TERMINAL_SESSION_PRESENTATIONS = {
  CURRENT: 'current',
  CLASSIC_TABS: 'classic-tabs'
};

function resolveTerminalSessionPresentation(presentation) {
  return presentation === TERMINAL_SESSION_PRESENTATIONS.CLASSIC_TABS
    ? TERMINAL_SESSION_PRESENTATIONS.CLASSIC_TABS
    : TERMINAL_SESSION_PRESENTATIONS.CURRENT;
}

export {
  TERMINAL_SESSION_PRESENTATIONS,
  resolveTerminalSessionPresentation
};
