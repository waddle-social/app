import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router';

import ChatView from '../views/ChatView.vue';
import ConversationListView from '../views/ConversationListView.vue';
import PluginView from '../views/PluginView.vue';
import RosterView from '../views/RosterView.vue';
import SettingsView from '../views/SettingsView.vue';

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    name: 'conversations',
    component: ConversationListView,
  },
  {
    path: '/chat/:jid',
    name: 'chat',
    component: ChatView,
    props: true,
  },
  {
    path: '/roster',
    name: 'roster',
    component: RosterView,
  },
  {
    path: '/settings',
    name: 'settings',
    component: SettingsView,
  },
  {
    path: '/plugin/:id',
    name: 'plugin',
    component: PluginView,
    props: true,
  },
];

export const router = createRouter({
  history: createWebHistory(),
  routes,
});
