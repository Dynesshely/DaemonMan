/** 自启动管理：管理员权限相关的错误识别与提权动作类型。 */

export type ElevatedAutostartAction = "enable" | "disable" | "restore" | "delete";

/** 把后端抛出的错误统一转成小写文本，便于关键字匹配。 */
function errorText(error: unknown): string {
  if (typeof error === "string") return error.toLowerCase();
  if (error instanceof Error) return error.message.toLowerCase();
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    return String(message ?? "").toLowerCase();
  }
  return String(error ?? "").toLowerCase();
}

const PERMISSION_HINTS = [
  "管理员",
  "拒绝访问",
  "access denied",
  "access is denied",
  "需要提升",
  "提升权限",
  "权限不足",
  "elevated",
  "elevation",
  "administrator",
  "privilege",
  "permission denied",
  "winerror 5",
  "0x80070005",
];

/** 错误是否表示「需要管理员权限」。 */
export function isPermissionError(error: unknown): boolean {
  const text = errorText(error);
  return PERMISSION_HINTS.some((hint) => text.includes(hint));
}

/** 错误是否表示「用户取消了 UAC 授权」。 */
export function isAdminCancelled(error: unknown): boolean {
  const text = errorText(error);
  if (text.includes("已取消管理员授权")) return true;
  return (
    text.includes("cancel") &&
    (text.includes("admin") || text.includes("elevat") || text.includes("授权"))
  );
}
