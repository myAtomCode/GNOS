# hac-mcp — AI 操作《Humans are Cats: Investigation》的 MCP 工具包

让 AI 通过 **MCP 协议 + CDP + Microsoft Edge** 自动游玩网页游戏
https://cats.renchengzhang.com/ （猫化行动 / Endless Case）。

## 组成

| 路径 | 说明 |
|---|---|
| `mcp-server.mjs` | MCP stdio 服务器（15 个工具） |
| `browser.js` | Edge/CDP 启动器（自动找 Edge；本机路径 `/opt/apps/com.browser.softedge.stable-pre/...`） |
| `game-src/index.html` + `assets/` | 官方前端资源快照 |
| `game-src/assets/index.beauty.js` | 反压缩美化后的主 bundle（16348 行，可 grep 游戏逻辑） |

## 安装

```bash
cd ~/gnos/hac-mcp && npm install
```

## 接入 AI 客户端（示例）

```json
{
  "mcpServers": {
    "hac": {
      "command": "node",
      "args": ["/home/elaina/gnos/hac-mcp/mcp-server.mjs"],
      "env": {
        "EDGE_PATH": "/opt/apps/com.browser.softedge.stable-pre/files/microsoft/msedge/microsoft-edge",
        "HAC_URL": "https://cats.renchengzhang.com/"
      }
    }
  }
}
```

## 工具一览

| 工具 | 用途 |
|---|---|
| `game_launch` | 启动 Edge 打开游戏（先调用） |
| `screenshot` | 抓当前帧 PNG（AI 视觉读帧的核心感知手段） |
| `key_down` / `key_up` | **按住/松开**——跑酷类必须用按住式输入 |
| `key_tap` | 点按并指定按住时长 |
| `click` / `drag` | 鼠标点击 / 划动 |
| `eval_js` | 页面内执行 JS（读状态、改 localStorage 等） |
| `storage_get_all` / `storage_set` / `storage_clear` | 存档操作（排行榜分数存这里） |
| `page_info` / `console_logs` / `navigate` / `close_browser` | 辅助 |

## 游戏速查（实测）

- 进入对局：菜单点「开始调查」(≈897,525) → 点击任意处跳过简报 → canvas 挂载 (1280x720)
- 按键（游戏监听 `e.key.toLowerCase()`）：`a/d` 移动 · `空格/w/↑` 跳 · `s/↓` 滑行 · `shift/d` 冲刺 Dash · `f/鼠标左键` 声波探测
- HUD：DIST 距离分、DATA 收集数、NEAR 擦弹、HP、COMBO/MULT 连击倍率、DASH READY
- 死亡后结算进排行榜（localStorage）

## 已知环境注意事项

- 本机 GPU 驱动会让页面崩溃，已默认加 `--use-angle=swiftshader`（软件渲染，正常游玩）
- puppeteer 返回的截图是 Uint8Array，Node 26 下必须 `Buffer.from(buf).toString('base64')`
