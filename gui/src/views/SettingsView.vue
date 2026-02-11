<script setup lang="ts">
import { storeToRefs } from 'pinia';

import { useSettingsStore, type ThemeChoice } from '../stores/settings';

const settingsStore = useSettingsStore();
const { locale, notificationsEnabled, theme } = storeToRefs(settingsStore);

function onThemeChange(event: Event): void {
  const element = event.target as HTMLSelectElement;
  settingsStore.setTheme(element.value as ThemeChoice);
}
</script>

<template>
  <section class="space-y-4">
    <header>
      <h1 class="text-2xl font-semibold">Settings</h1>
      <p class="text-sm text-muted">Appearance and notification preferences.</p>
    </header>

    <div class="grid gap-4 md:grid-cols-2">
      <label class="rounded-xl border border-border bg-surface p-4 text-sm">
        <span class="mb-2 block text-muted">Locale</span>
        <input v-model="locale" class="w-full rounded border border-border px-2 py-1" />
      </label>

      <label class="rounded-xl border border-border bg-surface p-4 text-sm">
        <span class="mb-2 block text-muted">Theme</span>
        <select
          :value="theme"
          class="w-full rounded border border-border bg-surface px-2 py-1"
          @change="onThemeChange"
        >
          <option value="light">Light</option>
          <option value="dark">Dark</option>
          <option value="system">System</option>
        </select>
      </label>

      <label class="rounded-xl border border-border bg-surface p-4 text-sm md:col-span-2">
        <span class="flex items-center justify-between">
          <span>Enable desktop notifications</span>
          <input v-model="notificationsEnabled" type="checkbox" class="h-4 w-4" />
        </span>
      </label>
    </div>
  </section>
</template>
