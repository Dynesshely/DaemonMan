import type { AutostartItem } from "@/api";

/** 自启动管理页的「类别」筛选维度。 */
export type CategoryFilter =
  | "all"
  | "registry"
  | "startup-folder"
  | "scheduled-task"
  | "service";

/** 自启动管理页的「位置」筛选维度。 */
export type LocationFilter = "all" | "user" | "machine";

export type AutostartCategory = Exclude<CategoryFilter, "all">;
export type AutostartLocation = Exclude<LocationFilter, "all">;

export const CATEGORY_VALUES: CategoryFilter[] = [
  "all",
  "registry",
  "startup-folder",
  "scheduled-task",
  "service",
];

export const LOCATION_VALUES: LocationFilter[] = ["all", "user", "machine"];

/** 把后端 `AutostartSource` 归并成界面上的类别。 */
export function categoryOf(item: AutostartItem): AutostartCategory {
  switch (item.source) {
    case "registry-run":
    case "registry-run-once":
      return "registry";
    case "startup-folder":
      return "startup-folder";
    case "scheduled-task":
      return "scheduled-task";
    case "service":
      return "service";
    default:
      return "registry";
  }
}

/**
 * 推导自启动项的作用域位置：
 * - registry-*：scope 含 "machine"（machine / machine32）→ 系统级，否则当前用户
 * - startup-folder：scope === "machine" → 系统级
 * - scheduled-task：需要管理员权限的视为系统级
 * - service：一律系统级
 */
export function locationOf(item: AutostartItem): AutostartLocation {
  const scope = (item.scope ?? "").toLowerCase();
  switch (item.source) {
    case "registry-run":
    case "registry-run-once":
      return scope.includes("machine") ? "machine" : "user";
    case "startup-folder":
      return scope === "machine" ? "machine" : "user";
    case "scheduled-task":
      return item.requiresAdmin ? "machine" : "user";
    case "service":
      return "machine";
    default:
      return "user";
  }
}
