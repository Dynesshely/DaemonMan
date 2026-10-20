/**
 * DaemonMan UI 冒烟脚本（通过 WebView2 远程调试协议驱动真实窗口）。
 *
 * 前置：
 *   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=19222"
 *   启动 DaemonMan.exe
 * 运行：
 *   node scripts/ui-smoke.mjs
 *
 * 输出：.develop/verify/*.png、控制台错误列表、每个路由的文本摘要。
 */

import { mkdirSync, writeFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

const PORT = process.env.CDP_PORT ?? "19222";
const BASE = `http://127.0.0.1:${PORT}`;
const OUT_DIR = new URL("../.develop/verify/", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");

async function getTarget() {
  const response = await fetch(`${BASE}/json/list`);
  const targets = await response.json();
  const page = targets.find((target) => target.type === "page");
  if (!page) {
    throw new Error(`未找到 page target：${JSON.stringify(targets)}`);
  }
  return page.webSocketDebuggerUrl;
}

class CdpClient {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 1;
    this.pending = new Map();
    this.events = [];
    ws.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (message.id && this.pending.has(message.id)) {
        const { resolve, reject } = this.pending.get(message.id);
        this.pending.delete(message.id);
        if (message.error) reject(new Error(JSON.stringify(message.error)));
        else resolve(message.result);
      } else if (message.method) {
        this.events.push(message);
      }
    });
  }

  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
      setTimeout(() => {
        if (this.pending.has(id)) {
          this.pending.delete(id);
          reject(new Error(`CDP 超时：${method}`));
        }
      }, 15_000);
    });
  }
}

function collectProblems(events) {
  const problems = [];
  for (const event of events) {
    if (event.method === "Runtime.consoleAPICalled" && event.params.type === "error") {
      problems.push({
        kind: "console.error",
        text: event.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" "),
      });
    }
    if (event.method === "Runtime.exceptionThrown") {
      problems.push({
        kind: "exception",
        text: event.params.exceptionDetails?.exception?.description ?? event.params.exceptionDetails?.text ?? "",
      });
    }
    if (event.method === "Log.entryAdded" && event.params.entry.level === "error") {
      problems.push({ kind: "log", text: event.params.entry.text });
    }
    if (event.method === "Network.loadingFailed" && !event.params.canceled) {
      problems.push({
        kind: "network",
        text: `${event.params.type} ${event.params.errorText}`,
      });
    }
  }
  return problems.filter(
    (problem, index, list) =>
      index === list.findIndex((item) => item.kind === problem.kind && item.text === problem.text),
  );
}

async function main() {
  mkdirSync(OUT_DIR, { recursive: true });
  const wsUrl = await getTarget();
  const ws = new WebSocket(wsUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", reject, { once: true });
  });

  const client = new CdpClient(ws);
  await client.send("Page.enable");
  await client.send("Runtime.enable");
  await client.send("Log.enable");
  await client.send("Network.enable");

  const evaluate = async (expression) => {
    const result = await client.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.text ?? "evaluate failed");
    }
    return result.result?.value;
  };

  await sleep(2000);

  const viewport = await evaluate(`({
    href: location.href,
    width: window.innerWidth,
    height: window.innerHeight,
    title: document.title,
  })`);
  console.log("viewport:", JSON.stringify(viewport));

  const routes = [
    ["overview", "/"],
    ["workflows", "/workflows"],
    ["startup", "/startup"],
    ["services", "/services"],
    ["logs", "/logs"],
    ["settings", "/settings"],
  ];

  const pages = [];
  for (const [name, path] of routes) {
    await evaluate(`(location.hash = ${JSON.stringify(`#${path}`)}, true)`);
    await sleep(1600);
    const info = await evaluate(`({
      hash: location.hash,
      text: document.body.innerText.slice(0, 6000),
      bodyWidth: document.body.scrollWidth,
      bodyHeight: document.body.scrollHeight,
      viewportWidth: window.innerWidth,
      nodeCount: document.querySelectorAll("*").length,
    })`);
    const screenshot = await client.send("Page.captureScreenshot", {
      format: "png",
      captureBeyondViewport: false,
    });
    writeFileSync(`${OUT_DIR}${name}.png`, Buffer.from(screenshot.data, "base64"));
    pages.push({
      name,
      path,
      hash: info.hash,
      textLength: info.text.length,
      bodyWidth: info.bodyWidth,
      bodyHeight: info.bodyHeight,
      viewportWidth: info.viewportWidth,
      nodeCount: info.nodeCount,
      text: info.text,
    });
  }

  // 检查中英文切换是否正常工作（验证后切回中文，保持用户配置干净）
  await evaluate(`(location.hash = "#/settings", true)`);
  await sleep(800);
  const localeSwitch = await evaluate(`(async () => {
    const select = document.querySelector("select");
    if (!select) return { ok: false, reason: "no select" };
    select.value = "en";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await new Promise((r) => setTimeout(r, 700));
    return { ok: true, text: document.body.innerText.slice(0, 400) };
  })()`);
  if (localeSwitch.ok) {
    await evaluate(`(async () => {
      const select = document.querySelector("select");
      if (!select) return false;
      select.value = "zh-CN";
      select.dispatchEvent(new Event("change", { bubbles: true }));
      await new Promise((r) => setTimeout(r, 600));
      return true;
    })()`);
  }

  const problems = collectProblems(client.events);
  const result = { viewport, pages, localeSwitch, problems };
  writeFileSync(`${OUT_DIR}smoke.json`, JSON.stringify(result, null, 2));

  console.log("\n=== 页面摘要 ===");
  for (const page of pages) {
    console.log(
      `${page.name.padEnd(10)} hash=${page.hash.padEnd(12)} text=${String(page.textLength).padStart(5)} nodes=${String(page.nodeCount).padStart(4)} body=${page.bodyWidth}x${page.bodyHeight}`,
    );
  }
  console.log("\n=== 前端问题 ===");
  if (problems.length === 0) {
    console.log("无 console/网络错误");
  } else {
    for (const problem of problems) console.log(`[${problem.kind}] ${problem.text}`);
  }
  console.log("\n截图目录：.develop/verify/");
  console.log(JSON.stringify({ localeSwitch: localeSwitch.ok }, null, 2));

  ws.close();
}

main().catch((error) => {
  console.error("UI 冒烟失败：", error);
  process.exitCode = 1;
});
