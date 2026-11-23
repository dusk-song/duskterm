import { defineStore } from 'pinia';
import { ref } from 'vue';
import { moveSessionInList } from '../utils/tabReorder';

export const useWorkspaceStore = defineStore('workspace', () => {
  const panels = ref([]);
  const activeId = ref(null);

  function add(id, kind, activate = true) {
    panels.value.push({ id, kind });
    if (activate) activeId.value = id;
  }

  function remove(id) {
    const index = panels.value.findIndex(panel => panel.id === id);
    if (index < 0) return;
    panels.value.splice(index, 1);
    if (activeId.value === id) {
      activeId.value = (panels.value[index] || panels.value[index - 1])?.id ?? null;
    }
  }

  function move(id, target, placement) {
    moveSessionInList(panels.value, id, target, placement);
  }

  return { panels, activeId, add, remove, move };
});
