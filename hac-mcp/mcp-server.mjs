#!/usr/bin/env node
// mcp-server.mjs — MCP stdio server exposing game-control tools so an AI can
// play "Humans are Cats: Investigation" (https://cats.renchengzhang.com/).
//
// Transport: stdio (JSON-RPC 2.0, MCP 1.x). Browser: Microsoft Edge over CDP
// via puppeteer-core (works with any Chromium-based browser).
//
// Tools:
//   game_launch      open Edge on the game page
//   navigate         goto a URL
//   screenshot       PNG of the current frame (returned as image content)
//   key_down/key_up  hold-based input the platformer actually needs
//   key_tap          press with hold duration
//   click            mouse click at viewport coords
//   eval_js          run JS inside the page (read state, poke localStorage…)
//   storage_get/set/clear   localStorage helpers (highscores live here)
//   console_logs     recent browser console output
//   page_info        url/title/viewport
//   close_browser    shut Edge down

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";
import {
  ensurePage, getPage, shutdown, GAME_URL, findBrowserBinary,
  consoleRing,
} from "./browser.js";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const DEAD = /detached|Target closed|Session closed|Browser closed/i;

// 页面被销毁(崩溃/手动关窗)时自动重启浏览器并重试一次
async function resilient(fn, { relaunchUrl = true } = {}) {
  try { return await fn(getPage()); }
  catch (e) {
    if (!DEAD.test(e.message)) throw e;
    await shutdown();
    const p = await ensurePage({});
    if (relaunchUrl) await p.goto(GAME_URL, { waitUntil: "domcontentloaded", timeout: 45000 }).catch(() => {});
    await sleep(2500);
    return await fn(p);
  }
}

const server = new McpServer(
  { name: "hac-mcp", version: "1.0.0" },
  { capabilities: { tools: {} } }
);

/* ---------------------------------------------------------------- launch */

server.tool(
  "game_launch",
  "Launch Microsoft Edge (CDP) and open the game. Do this first.",
  {
    url: z.string().optional().describe("Defaults to the official game URL"),
    width: z.number().optional(),
    height: z.number().optional(),
    headless: z.boolean().optional().describe("Headed window is best for canvas games"),
  },
  async ({ url, width, height, headless }) => {
    const p = await ensurePage({ width, height, headless });
    await p.goto(url || GAME_URL, { waitUntil: "domcontentloaded", timeout: 45000 });
    // The bundle is one ESM file; give React a moment to mount.
    await p.waitForFunction(() => !!document.querySelector("canvas"), { timeout: 20000 }).catch(() => {});
    const bin = findBrowserBinary();
    return { content: [{ type: "text", text: `Launched ${bin}\nURL: ${p.url()}` }] };
  }
);

server.tool("navigate", "Navigate the current tab.", { url: z.string() },
  async ({ url }) => {
    const p = getPage();
    await p.goto(url, { waitUntil: "domcontentloaded" });
    return { content: [{ type: "text", text: `Now at ${url}` }] };
  });

/* ------------------------------------------------------------ perception */

server.tool("screenshot", "Capture the game frame as PNG (AI reads it visually).", {},
  async () => {
    const p = getPage();
    const buf = await p.screenshot({ type: "png" });
    const b64 = Buffer.from(buf).toString("base64"); // puppeteer returns Uint8Array
    return { content: [
      { type: "image", data: b64, mimeType: "image/png" },
      { type: "text", text: `PNG ${b64.length} b64chars @ ${new Date().toISOString()} viewport=${JSON.stringify(p.viewport())}` },
    ] };
  });

server.tool("page_info", "Current URL/title/viewport/canvas size.", {},
  async () => {
    const p = getPage();
    const info = await p.evaluate(() => ({
      title: document.title,
      url: location.href,
      canvas: (() => { const c = document.querySelector("canvas"); return c ? { w: c.width, h: c.height } : null; })(),
      focused: document.hasFocus(),
    }));
    info.viewport = p.viewport();
    return { content: [{ type: "text", text: JSON.stringify(info, null, 2) }] };
  });

server.tool("console_logs", "Recent browser console messages.", { n: z.number().optional() },
  async ({ n }) => {
    const out = consoleRing.slice(-(n || 40));
    return { content: [{ type: "text", text: out.map(l => `[${l.kind}] ${l.text}`).join("\n") || "(empty)" }] };
  });

/* --------------------------------------------------------------- control */
// The game listens on window keydown/keyup and tracks keys by
// `event.key.toLowerCase()` ("a","d","w","shift","arrowleft", …), so raw CDP
// keyboard events map straight onto its internal state table.

const KeySchema = {
  key: z.string().describe("DOM key name, e.g. a d w s shift arrowleft space enter"),
};
const sleepMs = z.number().optional().describe("Hold duration in ms before releasing");

server.tool("key_down", "Press and HOLD a key (movement is hold-based).", KeySchema,
  async ({ key }) => { await getPage().keyboard.down(key); return { content: [{ type: "text", text: `down ${key}` }] }; });

server.tool("key_up", "Release a held key.", KeySchema,
  async ({ key }) => { await getPage().keyboard.up(key); return { content: [{ type: "text", text: `up ${key}` }] }; });

server.tool("key_tap", "Tap a key, holding it for `ms`.", {...KeySchema, ms: sleepMs},
  async ({ key, ms }) => {
    const k = getPage().keyboard;
    await k.down(key); await sleep(Math.max(30, ms ?? 120)); await k.up(key);
    return { content: [{ type: "text", text: `tap ${key} (${ms ?? 120}ms)` }] };
  });

server.tool("click", "Mouse click at viewport coordinates.",
  { x: z.number(), y: z.number(), button: z.enum(["left","right","middle"]).optional(), delay: z.number().optional() },
  async ({ x, y, button = "left", delay = 60 }) => {
    await getPage().mouse.click(x, y, { button, delay });
    return { content: [{ type: "text", text: `clicked ${button}@${x},${y}` }] };
  });

server.tool("drag", "Press, move through points, release — for swipe-style input.",
  { points: z.array(z.object({ x: z.number(), y: z.number() })).min(2), steps: z.number().optional() },
  async ({ points, steps = 12 }) => {
    const m = getPage().mouse;
    await m.move(points[0].x, points[0].y);
    await m.down();
    for (let i = 1; i < points.length; i++)
      await m.move(points[i].x, points[i].y, { steps });
    await m.up();
    return { content: [{ type: "text", text: `dragged ${points.length} pts` }] };
  });

/* ------------------------------------------------------------- scripting */

server.tool("eval_js", "Evaluate JS in the page context and JSON-stringify the result.",
  { expression: z.string() },
  async ({ expression }) => {
    try {
      const r = await resilient((p) => p.evaluate(expression));
      return { content: [{ type: "text", text: JSON.stringify(r, null, 2)?.slice(0, 8000) ?? "undefined" }] };
    } catch (e) {
      return { content: [{ type: "text", text: `EvalError: ${e.message}` }], isError: true };
    }
  });

const st = (name, desc, fn) => server.tool(name, desc, {}, fn);
st("storage_get_all", "Dump localStorage of the game origin.", async () => {
  const r = await getPage().evaluate(() =>
    Object.fromEntries(Object.entries(localStorage)));
  return { content: [{ type: "text", text: JSON.stringify(r, null, 2) }] };
});
server.tool("storage_set", "Write a localStorage entry (e.g. highscore).",
  { key: z.string(), value: z.string() }, async ({ key, value }) => {
    await getPage().evaluate((k, v) => localStorage.setItem(k, v), key, value);
    return { content: [{ type: "text", text: `${key}=${value}` }] };
  });
st("storage_clear", "Clear localStorage (fresh start).", async () => {
  await getPage().evaluate(() => localStorage.clear());
  return { content: [{ type: "text", text: "cleared" }] };
});

st("close_browser", "Close Edge.", async () => {
  await shutdown(); return { content: [{ type: "text", text: "closed" }] };
});

/* ------------------------------------------------------------------ run */

process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

await server.connect(new StdioServerTransport());
