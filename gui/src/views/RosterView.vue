<script setup lang="ts">
import { storeToRefs } from 'pinia';

import { useRosterStore } from '../stores/roster';

const rosterStore = useRosterStore();
const { grouped } = storeToRefs(rosterStore);
</script>

<template>
  <section class="space-y-4">
    <header>
      <h1 class="text-2xl font-semibold">Roster</h1>
      <p class="text-sm text-muted">Contacts grouped by roster group.</p>
    </header>

    <div class="space-y-4">
      <article
        v-for="(entries, groupName) in grouped"
        :key="groupName"
        class="rounded-xl border border-border bg-surface p-4"
      >
        <h2 class="text-sm font-semibold uppercase tracking-wide text-muted">{{ groupName }}</h2>
        <ul class="mt-2 space-y-2">
          <li v-for="entry in entries" :key="entry.jid" class="flex items-center justify-between gap-3 text-sm">
            <span>{{ entry.name }} <span class="text-muted">({{ entry.jid }})</span></span>
            <span class="capitalize text-muted">{{ entry.presence }}</span>
          </li>
        </ul>
      </article>
    </div>
  </section>
</template>
