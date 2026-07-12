<script setup>
import SftpFileManager from './SftpFileManager.vue';

defineProps({
  sessionId: String,
  followSessionId: String,
  visible: Boolean
});

const emit = defineEmits(['close']);
</script>

<template>
  <SftpFileManager :sessionId="sessionId" :follow-session-id="followSessionId" :visible="visible"
    @close="emit('close')" />
</template>
