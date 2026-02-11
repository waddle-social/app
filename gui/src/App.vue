<script setup lang="ts">
import { useTheme } from './composables/useTheme';

useTheme();
</script>

<template>
  <div class="mx-auto grid min-h-screen max-w-6xl grid-rows-[auto,1fr] gap-4 px-4 py-6 md:px-8">
    <header class="rounded-2xl border border-border bg-surface p-4 shadow-sm">
      <div class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div>
          <p class="text-xs font-semibold uppercase tracking-[0.14em] text-muted">Waddle GUI</p>
          <h1 class="text-2xl font-semibold">Frontend Shell</h1>
        </div>
        <nav class="flex flex-wrap gap-2 text-sm">
          <RouterLink class="rounded-lg border border-border px-3 py-1.5" to="/">Conversations</RouterLink>
          <RouterLink class="rounded-lg border border-border px-3 py-1.5" to="/roster">Roster</RouterLink>
          <RouterLink class="rounded-lg border border-border px-3 py-1.5" to="/settings">Settings</RouterLink>
          <RouterLink class="rounded-lg border border-border px-3 py-1.5" to="/plugin/org.waddle.omemo">
            Plugin
          </RouterLink>
        </nav>
      </div>
    </header>

    <main class="rounded-2xl border border-border bg-surface/70 p-4 shadow-sm backdrop-blur-sm md:p-6">
      <RouterView />
    </main>
  </div>
</template>

<style scoped>
.router-link-active {
  background-color: var(--waddle-accent);
  border-color: var(--waddle-accent);
  color: #ffffff;
}
</style>
