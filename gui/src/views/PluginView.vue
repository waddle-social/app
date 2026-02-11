<script setup lang="ts">
import { computed } from 'vue';
import { useRoute } from 'vue-router';

import { usePluginsStore } from '../stores/plugins';

const route = useRoute();
const pluginsStore = usePluginsStore();

const pluginId = computed(() => String(route.params.id ?? ''));
const plugin = computed(() => pluginsStore.getById(pluginId.value));
</script>

<template>
  <section class="space-y-4">
    <header>
      <h1 class="text-2xl font-semibold">Plugin</h1>
      <p class="text-sm text-muted">Dynamic plugin container route.</p>
    </header>

    <article v-if="plugin" class="rounded-xl border border-border bg-surface p-4">
      <h2 class="font-semibold">{{ plugin.name }}</h2>
      <p class="mt-1 text-sm text-muted">{{ plugin.id }}</p>
      <p class="mt-2 text-sm">Version {{ plugin.version }} · <span class="capitalize">{{ plugin.status }}</span></p>
    </article>

    <p v-else class="rounded-xl border border-border bg-surface p-4 text-sm text-muted">
      Plugin {{ pluginId }} is not installed.
    </p>
  </section>
</template>
