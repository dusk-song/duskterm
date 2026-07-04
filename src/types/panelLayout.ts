export type LayoutMode = 'tabs' | 'tile';

export interface PanelModel {
  panelId: string;
  title: string;
  sessionId?: string;
  terminalId?: string;
  cwd?: string;
  scrollback?: number;
  connectionState: 'idle' | 'connecting' | 'connected' | 'disconnected' | 'error';
}

export interface ConnectionState {
  panelId: string;
  status: 'idle' | 'connecting' | 'connected' | 'disconnected' | 'error';
  errorMessage?: string;
}

export interface LayoutNode {
  id: string;
  type: 'split' | 'panel';
  direction?: 'horizontal' | 'vertical';
  ratio?: number;
  first?: LayoutNode;
  second?: LayoutNode;
  panelId?: string;
}

export interface BroadcastGroup {
  enabled: boolean;
  sourcePanelId?: string | null;
  targetPanelIds: string[];
}
