import { createRouter, createWebHashHistory } from "vue-router";
import type { RouteRecordRaw } from "vue-router";

const routes: RouteRecordRaw[] = [
  {
    path: "/",
    name: "overview",
    component: () => import("@/modules/views/OverviewView.vue"),
    meta: { titleKey: "nav.overview" },
  },
  {
    path: "/workflows",
    name: "workflows",
    component: () => import("@/modules/views/WorkflowsView.vue"),
    meta: { titleKey: "nav.workflows" },
  },
  {
    path: "/startup",
    name: "startup",
    component: () => import("@/modules/views/StartupView.vue"),
    meta: { titleKey: "nav.startup" },
  },
  {
    path: "/services",
    name: "services",
    component: () => import("@/modules/views/ServicesView.vue"),
    meta: { titleKey: "nav.services" },
  },
  {
    path: "/logs",
    name: "logs",
    component: () => import("@/modules/views/LogsView.vue"),
    meta: { titleKey: "nav.logs" },
  },
  {
    path: "/settings",
    name: "settings",
    component: () => import("@/modules/views/SettingsView.vue"),
    meta: { titleKey: "nav.settings" },
  },
  { path: "/:pathMatch(.*)*", redirect: "/" },
];

const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

export default router;
