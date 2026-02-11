<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useRoute } from 'vue-router';

import PluginContainer from '../components/PluginContainer.vue';
import { getPluginInfo } from '../composables/usePluginLoader';
import { usePluginsStore } from '../stores/plugins';
import type { PluginInfo } from '../composables/useWaddle';

const route = useRoute();
const pluginsStore = usePluginsStore();

const pluginId = computed(() => String(route.params.id ?? ''));
const plugin = computed(() => pluginsStore.getById(pluginId.value));
const pluginInfo = ref<PluginInfo | null>(null);

const guiComponents = computed<string[]>(() => {
  if (!pluginInfo.value) {
    return [];
  }
  return pluginInfo.value.capabilities
    .filter((cap) => cap === 'gui-metadata')
    .length > 0 && plugin.value
    ? [`${plugin.value.name}Settings.vue`]
    : [];
});

onMounted(async () => {
  if (!pluginId.value) {
    return;
  }
  try {
    pluginInfo.value = await getPluginInfo(pluginId.value);
  } catch {
    // Plugin info unavailable — display static info from store
  }
});
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

    <PluginContainer
      v-for="componentName in guiComponents"
      :key="componentName"
      :plugin-id="pluginId"
      :component-name="componentName"
    />
  </section>
</template>
