<script setup>
import SftpFileManager from './SftpFileManager.vue';

defineProps({
  sessionId: String
});

const emit = defineEmits(['close']);
</script>

<template>
  <SftpFileManager :sessionId="sessionId" @close="emit('close')" />
</template>
