// browser.js — Edge (CDP) lifecycle shared by the MCP server.
//
// The game is a canvas-rendered React app, so almost everything the AI does
// goes through real browser input: CDP mouse/keyboard events + screenshots.
// Any Chromium-based browser works; Microsoft Edge is preferred and found
// automatically in common locations (this machine keeps it under /opt).

import puppeteer from "puppeteer-core";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export const GAME_URL = process.env.HAC_URL || "https://cats.renchengzhang.com/";

const EDGE_CANDIDATES = [
  // Deepin/UOS style packaging used on this machine
  "/opt/apps/com.browser.softedge.stable-pre/files/microsoft/msedge/microsoft-edge",
  "/opt/apps/com.browser.softedge.stable-pre/files/microsoft/msedge/msedge",
  "/opt/microsoft/msedge/msedge",
  "/opt/google/chrome/chrome",
  "/usr/bin/microsoft-edge-stable",
  "/usr/bin/microsoft-edge",
  "/usr/bin/chromium-browser",
  "/usr/bin/chromium",
];

export function findBrowserBinary() {
  if (process.env.EDGE_PATH && fs.existsSync(process.env.EDGE_PATH))
    return process.env.EDGE_PATH;
  for (const p of EDGE_CANDIDATES) if (fs.existsSync(p)) return p;
  throw new Error(
    "No Chromium-based browser found. Set EDGE_PATH to msedge executable."
  );
}

let browser = null;
let page = null;
const consoleRing = []; // last N console messages
const MAX_LOGS = 200;

export function pushLog(kind, text) {
  consoleRing.push({ t: Date.now(), kind, text: String(text).slice(0, 500) });
  if (consoleRing.length > MAX_LOGS) consoleRing.shift();
}

export async function ensurePage(opts = {}) {
  if (browser && page && !page.isClosed()) return page;

  const executablePath = opts.executablePath || findBrowserBinary();
  const width = Number(opts.width || 1280);
  const height = Number(opts.height || 720);

  browser = await puppeteer.launch({
    executablePath,
    headless: opts.headless === true,
    defaultViewport: { width, height },
    args: [
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-gpu-driver-bug-workarounds",
      "--use-angle=swiftshader", // this machine's GPU driver crashes the page otherwise
      "--autoplay-policy=no-user-gesture-required",
      "--disable-blink-features=AutomationControlled",
      `--window-size=${width},${height}`,
      ...(opts.extraArgs || []),
    ],
    userDataDir: path.join(os.tmpdir(), `hac-edge-profile-${Date.now()}`),
  });

  page = (await browser.pages())[0] || (await browser.newPage());
  page.on("console", (m) => pushLog(m.type(), m.text()));
  page.on("pageerror", (e) => pushLog("pageerror", e.message));
  return page;
}

export function getPage() {
  if (!page || page.isClosed()) throw new Error("Browser not launched. Call game_launch first.");
  return page;
}

export async function shutdown() {
  try { if (browser) await browser.close(); } catch {}
  browser = null; page = null;
}

export { consoleRing };
