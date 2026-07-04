<script setup>
import { ref } from 'vue';
const props = defineProps({
  items: { type: Array, required: true }
});
const emit = defineEmits(['item-click']);

function onItemClick(item) {
  emit('item-click', item);
}
</script>

<template>
  <div class="tree-menu" role="tree">
    <TreeNode v-for="it in items" :key="it.key || it.id || it.label" :item="it" :level="1" @item-click="onItemClick" />
  </div>
</template>

<script>
import { defineComponent, ref } from 'vue';

export default defineComponent({
  name: 'TreeMenu',
  components: {
    TreeNode: defineComponent({
      name: 'TreeNode',
      props: {
        item: { type: Object, required: true },
        level: { type: Number, default: 1 }
      },
      emits: ['item-click'],
      setup(props, { emit }) {
        const open = ref(false);
        const hasChildren = () => Array.isArray(props.item.children) && props.item.children.length > 0;
        function toggle(e) {
          e?.stopPropagation();
          if (hasChildren()) open.value = !open.value;
          else emit('item-click', props.item);
        }
        return { open, hasChildren, toggle, emit };
      },
      template: `
        <div class="tree-node" role="treeitem" :aria-expanded="hasChildren()?open:undefined" :aria-level="level">
          <div class="tree-node-row" tabindex="0" @click="toggle" @keydown.enter.prevent="toggle" @keydown.space.prevent="toggle">
            <button v-if="hasChildren()" class="tree-expand-btn" @click.stop="toggle">{{ open ? '▾' : '▸' }}</button>
            <div class="tree-node-label" @click.stop="!hasChildren() && $emit('item-click', item)">{{ item.label }}</div>
          </div>
          <div v-if="hasChildren() && open" class="tree-children" role="group">
            <TreeNode v-for="c in item.children" :key="c.key || c.id || c.label" :item="c" :level="level+1" @item-click="$emit('item-click', $event)" />
          </div>
        </div>
      `
    })
  }
});
</script>
