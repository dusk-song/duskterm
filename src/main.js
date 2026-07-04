import { createPinia } from "pinia";
import { createApp } from "vue";
import App from "./App.vue";
import './assets/styles/tailwind.css';
import './components/settings/settingsPaneShared.css';
import './styles/scrollbar.css';
import './styles/theme.css';
import { initPerformanceMode } from './utils/performance';

initPerformanceMode();

if (import.meta.env.PROD) {
  window.addEventListener('contextmenu', (event) => {
    // 只阻止原生右键菜单，放过 reka-ui context-menu 组件
    const target = event.target;
    const insideContextMenu = target.closest('[data-slot="context-menu"]')
      || target.closest('[data-slot="context-menu-content"]')
      || target.closest('[data-slot="context-menu-trigger"]');
    if (!insideContextMenu && event.cancelable) event.preventDefault();
  }, { capture: true });
}


const app = createApp(App);
app.use(createPinia());
app.mount("#app");

(async () => {
  try {
    const { getCurrentWindow, LogicalSize } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    await win.setMinSize(new LogicalSize(460, 250));
  } catch (e) {
    console.error('setMinSize failed:', e);
  }
})();
