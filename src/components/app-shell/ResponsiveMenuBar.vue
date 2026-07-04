<script setup>
import { nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import './responsive-menu.css';
import { executeMenuAction } from '@/composables/useMenu';

const props = defineProps({
  // `collapsed` 控制是否显示折叠按钮（由 App.vue 管理断点）
  collapsed: { type: Boolean, default: false },
  menuData: { type: Array, default: () => [] }
});

const open = ref(false);
const btnRef = ref(null);
const panelRef = ref(null);

onMounted(() => {
  document.addEventListener('click', onDocClick);
  document.addEventListener('keydown', onKeydown);
});
onBeforeUnmount(() => {
  document.removeEventListener('click', onDocClick);
  document.removeEventListener('keydown', onKeydown);
});

function onDocClick(e) {
  if (!open.value) return;
  const target = e.target;
  if (!panelRef.value || !btnRef.value) return;
  if (panelRef.value.contains(target) || btnRef.value.contains(target)) return;
  open.value = false;
}

function onKeydown(e) {
  if (e.key === 'Escape') open.value = false;
}

function triggerAction(actionOrKey) {
  if (!actionOrKey) return;
  if (typeof actionOrKey === 'function') return actionOrKey();
  // if it's a key for executeMenuAction, prefer that
  if (typeof actionOrKey === 'string') {
    // try to execute via existing menu handler first
    const handled = executeMenuAction(actionOrKey);
    if (!handled) {
      // fallback to dispatching as CustomEvent for legacy handlers
      window.dispatchEvent(new CustomEvent(actionOrKey));
    }
  }
}

function toggleOpen() {
  open.value = !open.value;
}

// panel position
const panelStyle = ref({});
async function updatePanelPos() {
  await nextTick();
  if (!btnRef.value) return;
  const rect = btnRef.value.getBoundingClientRect();
  panelStyle.value = {
    position: 'absolute',
    top: `${rect.bottom + 8}px`,
    left: `${rect.left}px`,
    minWidth: '220px',
    maxHeight: '60vh',
    zIndex: 'var(--z-dropdown)'
  };
}

const openSubMenus = reactive({});
const toggleSubMenu = (key) => { openSubMenus[key] = !openSubMenus[key]; };

watch(open, (v) => { if (v) updatePanelPos(); });
</script>

<template>
  <div class="responsive-menu-root">
    <div v-if="props.collapsed" class="menu-collapse-container">
      <button ref="btnRef" class="icon-button collapse-button" aria-label="菜单" :aria-expanded="open"
        @click="toggleOpen">
        <span class="collapse-icon">☰</span>
      </button>

      <teleport to="body">
        <div v-if="open" class="responsive-menu-panel" :style="panelStyle" ref="panelRef" role="dialog"
          aria-label="菜单面板">
          <div class="responsive-menu-panel-inner">
            <div class="flex flex-col gap-0.5">
              <template v-for="menu in props.menuData">
                <template v-if="menu.items">
                  <button :key="'submenu-' + menu.key" type="button"
                    class="w-full text-left px-3 py-2 text-sm rounded hover:bg-accent transition-colors flex items-center justify-between"
                    @click="toggleSubMenu(menu.key)">
                    <span>{{ menu.label }}</span>
                    <span class="text-xs" :class="{ 'rotate-90': openSubMenus[menu.key] }">▶</span>
                  </button>
                  <div v-if="openSubMenus[menu.key]" class="pl-4 flex flex-col gap-0.5">
                    <button v-for="entry in menu.items.filter(e => e.type !== 'divider')" :key="entry.key" type="button"
                      class="text-left px-3 py-1.5 text-sm rounded hover:bg-accent transition-colors"
                      @click="() => { triggerAction(entry.key || entry.action); open = false }">{{ entry.label }}</button>
                  </div>
                </template>
                <button v-else :key="'menu-' + menu.key" type="button"
                  class="w-full text-left px-3 py-2 text-sm rounded hover:bg-accent transition-colors"
                  @click="() => { triggerAction(menu.key || menu.action); open = false }">{{ menu.label }}</button>
              </template>
            </div>
          </div>
        </div>
      </teleport>
    </div>
  </div>
</template>

<style scoped>
@import './responsive-menu.css';
</style>
