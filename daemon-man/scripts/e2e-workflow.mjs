/**
 * DaemonMan 工作流编辑器端到端冒烟：
 * 新建工作流 → 添加延时节点 → 保存 → 立即运行 → 检查运行结果。
 *
 * 前置：应用以 --remote-debugging-port=19222 启动。
 */

import { writeFileSync, mkdirSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

const PORT = process.env.CDP_PORT ?? "19222";
const BASE = `http://127.0.0.1:${PORT}`;
const OUT_DIR = new URL("../.develop/verify/", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");

async function getPageTarget() {
  const response = await fetch(`${BASE}/json/list`);
  const targets = await response.json();
  const page = targets.find((target) => target.type === "page");
  if (!page) throw new Error("找不到 page target");
  return page.webSocketDebuggerUrl;
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 1;
    this.pending = new Map();
    this.errors = [];
    ws.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id && this.pending.has(message.id)) {
        const { resolve, reject } = this.pending.get(message.id);
        this.pending.delete(message.id);
        if (message.error) reject(new Error(JSON.stringify(message.error)));
        else resolve(message.result);
        return;
      }
      if (message.method === "Runtime.exceptionThrown") {
        this.errors.push(
          message.params.exceptionDetails?.exception?.description ??
            message.params.exceptionDetails?.text ??
            "exception",
        );
      }
      if (message.method === "Runtime.consoleAPICalled" && message.params.type === "error") {
        this.errors.push(
          message.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" "),
        );
      }
    });
  }

  send(method, params = {}) {
    const id = this.id++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`CDP 超时：${method}`));
        }
      }, 20_000);
    });
  }

  async evaluate(expression) {
    const result = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.text ?? "evaluate failed");
    }
    return result.result?.value;
  }

  async screenshot(name) {
    const shot = await this.send("Page.captureScreenshot", { format: "png" });
    writeFileSync(`${OUT_DIR}${name}.png`, Buffer.from(shot.data, "base64"));
  }
}

/** 点击文本匹配的按钮（在可见元素中找第一个）。 */
const clickByText = (text, tag = "button") => `(() => {
  const nodes = [...document.querySelectorAll(${JSON.stringify(tag)})];
  const target = nodes.find((node) => {
    const label = (node.textContent || "").replace(/\\s+/g, " ").trim();
    const rect = node.getBoundingClientRect();
    const visible = rect.width > 0 && rect.height > 0;
    return visible && label.includes(${JSON.stringify(text)});
  });
  if (!target) return { ok: false, count: nodes.length };
  target.click();
  return { ok: true, label: (target.textContent || "").trim() };
})()`;

async function main() {
  mkdirSync(OUT_DIR, { recursive: true });
  const ws = new WebSocket(await getPageTarget());
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });
  const cdp = new Cdp(ws);
  await cdp.send("Page.enable");
  await cdp.send("Runtime.enable");

  await cdp.evaluate(`(location.hash = "#/workflows", true)`);
  await sleep(1500);

  // 1. 新建工作流
  const create = await cdp.evaluate(clickByText("新建工作流"));
  console.log("新建工作流:", JSON.stringify(create));
  await sleep(1200);

  // 2. 添加延时节点
  const addDelay = await cdp.evaluate(clickByText("延时"));
  console.log("添加延时节点:", JSON.stringify(addDelay));
  await sleep(1000);

  // 3. 保存
  const save = await cdp.evaluate(clickByText("保存"));
  console.log("保存:", JSON.stringify(save));
  await sleep(1500);

  const afterSave = await cdp.evaluate(`({
    hash: location.hash,
    hasUnsaved: document.body.innerText.includes("未保存"),
    bodyText: document.body.innerText.slice(0, 1200),
  })`);
  console.log("保存后:", JSON.stringify({ hash: afterSave.hash, hasUnsaved: afterSave.hasUnsaved }));

  // 4. 立即运行
  const run = await cdp.evaluate(clickByText("立即运行"));
  console.log("立即运行:", JSON.stringify(run));
  await sleep(3500);

  const runState = await cdp.evaluate(`({
    bodyText: document.body.innerText.slice(0, 3000),
    running: document.body.innerText.includes("运行中"),
    success: document.body.innerText.includes("成功"),
    nodes: document.querySelectorAll(".vue-flow__node").length,
    edges: document.querySelectorAll(".vue-flow__edge").length,
  })`);
  await cdp.screenshot("workflow-created");
  console.log("运行状态:", JSON.stringify({
    nodes: runState.nodes,
    edges: runState.edges,
    running: runState.running,
    success: runState.success,
  }));

  // 5. 校验结构
  const validate = await cdp.evaluate(clickByText("校验结构"));
  await sleep(800);
  const validationText = await cdp.evaluate(`document.body.innerText.slice(0, 2500)`);
  await cdp.screenshot("workflow-validation");
  console.log("校验弹窗已打开:", JSON.stringify(validate));
  await cdp.evaluate(`(() => {
    const closeButtons = [...document.querySelectorAll('button')].filter((node) => node.getAttribute('aria-label') === '关闭');
    if (closeButtons.length) closeButtons[0].click();
    else document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    return true;
  })()`);
  await sleep(500);

  const result = {
    create,
    addDelay,
    save,
    run,
    nodes: runState.nodes,
    edges: runState.edges,
    runningVisible: runState.running,
    successVisible: runState.success,
    validationSnippet: validationText.slice(0, 800),
    errors: cdp.errors,
  };
  writeFileSync(`${OUT_DIR}e2e.json`, JSON.stringify(result, null, 2));
  console.log("\n前端错误:", result.errors.length ? result.errors : "无");
  console.log("截图：workflow-created.png / workflow-validation.png");
  ws.close();
}

main().catch((error) => {
  console.error("E2E 失败：", error);
  process.exitCode = 1;
});
