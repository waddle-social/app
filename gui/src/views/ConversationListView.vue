<script setup lang="ts">
import { storeToRefs } from 'pinia';

import { useConversationsStore } from '../stores/conversations';

const conversationsStore = useConversationsStore();
const { orderedConversations } = storeToRefs(conversationsStore);
</script>

<template>
  <section class="space-y-4">
    <header>
      <h1 class="text-2xl font-semibold">Conversations</h1>
      <p class="text-sm text-muted">Recent direct messages and rooms.</p>
    </header>

    <ul class="space-y-2">
      <li
        v-for="conversation in orderedConversations"
        :key="conversation.jid"
        class="rounded-xl border border-border bg-surface p-4"
      >
        <RouterLink :to="`/chat/${conversation.jid}`" class="flex items-center justify-between gap-3">
          <span class="font-medium">{{ conversation.title }}</span>
          <span
            v-if="conversation.unreadCount > 0"
            class="rounded-full bg-accent px-2 py-0.5 text-xs font-semibold text-white"
          >
            {{ conversation.unreadCount }}
          </span>
        </RouterLink>
        <p class="mt-2 text-sm text-muted">{{ conversation.preview }}</p>
      </li>
    </ul>
  </section>
</template>
