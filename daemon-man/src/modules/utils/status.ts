import type { NodeStatus, RunStatus } from "@/api";

export type BadgeVariant =
  | "neutral"
  | "accent"
  | "success"
  | "warn"
  | "danger"
  | "info"
  | "running";

export function runStatusVariant(status: RunStatus): BadgeVariant {
  switch (status) {
    case "running":
      return "running";
    case "success":
      return "success";
    case "failed":
      return "danger";
    case "cancelled":
      return "warn";
    default:
      return "neutral";
  }
}

export function nodeStatusVariant(status: NodeStatus): BadgeVariant {
  switch (status) {
    case "running":
      return "running";
    case "success":
      return "success";
    case "failed":
      return "danger";
    case "cancelled":
      return "warn";
    case "waiting":
      return "info";
    case "skipped":
      return "neutral";
    default:
      return "neutral";
  }
}

export function statusKey(
  status: RunStatus | NodeStatus | string,
): string {
  return `status.${status}`;
}
