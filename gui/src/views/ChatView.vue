<script setup lang="ts">
import { computed } from 'vue';
import { useRoute } from 'vue-router';

import { useConversationsStore } from '../stores/conversations';

const route = useRoute();
const conversationsStore = useConversationsStore();

const jid = computed(() => String(route.params.jid ?? ''));
const messages = computed(() => conversationsStore.messagesFor(jid.value));
</script>

<template>
  <section class="space-y-4">
    <header>
      <h1 class="text-2xl font-semibold">Chat</h1>
      <p class="text-sm text-muted">{{ jid }}</p>
    </header>

    <ol class="space-y-2">
      <li
        v-for="message in messages"
        :key="message.id"
        class="rounded-xl border border-border bg-surface p-3"
      >
        <p class="text-sm font-medium">{{ message.from }}</p>
        <p class="mt-1 text-sm">{{ message.body }}</p>
      </li>
    </ol>

    <p v-if="messages.length === 0" class="text-sm text-muted">No messages loaded for this conversation.</p>
  </section>
</template>
