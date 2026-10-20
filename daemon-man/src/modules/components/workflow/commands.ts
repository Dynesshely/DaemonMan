import { invoke } from "@tauri-apps/api/core";

import { api } from "@/api";
import type { Workflow, WorkflowRun } from "@/api";

/** `run_single_node` 的契约（由 Lead 提供，参数为当前 draft 与节点 id）。 */
export type RunSingleNodeFn = (
  workflow: Workflow,
  nodeId: string,
) => Promise<WorkflowRun>;

/**
 * 单点执行某个节点（调试用）。
 * 优先走 `api.runSingleNode(workflow, nodeId)`；若该封装尚未在 api 层落地，
 * 退回直接 invoke 同名命令，保证功能不会因为 API 层延迟而失效。
 */
export function runSingleNode(
  workflow: Workflow,
  nodeId: string,
): Promise<WorkflowRun> {
  const fn = (api as unknown as { runSingleNode?: RunSingleNodeFn })
    .runSingleNode;
  if (typeof fn === "function") {
    return fn(workflow, nodeId);
  }
  return invoke<WorkflowRun>("run_single_node", { workflow, nodeId });
}
