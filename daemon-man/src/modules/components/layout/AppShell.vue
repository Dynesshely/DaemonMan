<script setup lang="ts">
import { RouterView } from "vue-router";

import Sidebar from "./Sidebar.vue";
import StatusBar from "./StatusBar.vue";
import TopBar from "./TopBar.vue";
import ToastStack from "@/modules/components/ui/ToastStack.vue";
</script>

<template>
  <div
    class="flex h-screen w-screen overflow-hidden bg-zinc-100 text-zinc-900 dark:bg-zinc-950 dark:text-zinc-100"
  >
    <Sidebar />
    <div class="flex min-w-0 flex-1 flex-col">
      <TopBar />
      <main class="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <RouterView v-slot="{ Component }">
          <Transition name="fade" mode="out-in">
            <component :is="Component" />
          </Transition>
        </RouterView>
      </main>
      <StatusBar />
    </div>
    <ToastStack />
  </div>
</template>
