// ==UserScript==
// @name         猫化行动 · 训练器 (Humans are Cats Trainer)
// @namespace    hac-trainer
// @version      1.0.0
// @description  悬浮球面板：锁血/自定义生命/一击秒杀/攻击率(无冷却+自动连发)/无限冲刺/分数倍率等
// @author       you
// @match        https://cats.renchengzhang.com/*
// @run-at       document-idle
// @grant        none
// ==/UserScript==

(() => {
  "use strict";
  if (window.__HAC_TRAINER__) return;
  window.__HAC_TRAINER__ = true;

  /* ================= 配置状态 ================= */
  const cfg = {
    lockHpOn: false, lockHpValue: 100,
    zeroPanic: false,
    oneKill: false,
    noAtkCd: false,         // 攻击无冷却
    autoFireRate: 0,        // 自动连发 (次/秒), 0=关
    infDash: false,
    magnetAlways: false,
    shieldAlways: false,
    scoreMul: 1,            // 分数倍率 (覆盖 multiplier)
    comboLockN: 0,          // >0 时连击不低于该值
    autoPlay: false,        // 自动挂机 (移动+跳跃避障)
    autoCatify: false,      // 半径内敌人自动猫化
    catRadius: 300,         // 猫化判定半径 (px, 约5米)
    stealth: true,          // 隐蔽模式: 锁血改为"低于则回充", 不钉满值不刷无敌帧
    autoDismiss: true,      // 自动关聊天/弹窗 (等一下/继续/跳过)
    speedMode: false,       // 极速奔跑
    speedMult: 13.5,        // 相对冲刺的速度倍率 (1350%)
  };
  const dieNow = () => {
    scan();                          // 拿最新 ref, 游戏可能整体替换过对象
    const P = cache.player;
    if (!P || typeof P.hp !== "number") return false;
    cfg.lockHpOn = false;            // 否则隐蔽回血会把死亡打断
    P.shieldTime = 0; P.invulnerableTime = 0;
    P.isDashing = false; P.isSliding = false; P.slideTime = 0;
    P.hp = 0; P.panic = 100;
    let ok = false;
    if (cache.death && isDeath(cache.death)) {
      cache.death.active = true;     // 触发掉落动画 -> 超时后自动进结算
      cache.death.startedAt = Date.now();
      ok = true;
    }
    P.vx = 2.6; P.vy = -16;          // 与 ft() 致死分支一致
    return ok || P.hp <= 0;
  };
  window.HACTRAINER = { cfg, dieNow, net }; // 外部/MCP 调试接口

  /* ================= React Fiber 内存扫描 ================= */
  let cache = { player: null, npcs: null, hazards: null, stats: null, death: null, tick: 0 };

  const isPlayer = (o) => o && typeof o === "object" &&
    typeof o.hp === "number" && typeof o.maxHp === "number" &&
    "panic" in o && "isDashing" in o && "dashCooldown" in o;
  const isStats = (o) => o && typeof o === "object" &&
    "combo" in o && "multiplier" in o && "score" in o;
  const isNpcList = (a) => Array.isArray(a) && a.length > 0 &&
    a[0] && typeof a[0] === "object" &&
    ("labelKey" in a[0] || "scanHits" in a[0] || "chatKind" in a[0]);
  const isDeath = (o) => {
    if (!o || typeof o !== "object") return false;
    const ks = Object.keys(o);
    return ks.length === 2 && "active" in o && "startedAt" in o &&
      typeof o.active === "boolean" && typeof o.startedAt === "number";
  };
  const isHazList = (a) => Array.isArray(a) && a.length > 0 &&
    a[0] && typeof a[0] === "object" && "id" in a[0] &&
    !("labelKey" in a[0]) && !("collected" in a[0]);

  function collectHooks(fiber, out, depth = 0) {
    while (fiber && depth < 80) {
      let h = fiber.memoizedState, n = 0;
      while (h && n++ < 120) {
        const v = h.memoizedState;
        if (v && typeof v === "object") {
          if (!out.player && isPlayer(v.current)) out.player = v.current;
          if (!out.stats && isStats(v.current)) out.stats = v.current;
          if (!out.npcs && isNpcList(v.current)) out.npcs = v.current;
          if (!out.death && isDeath(v.current)) out.death = v.current;
          if (!out.hazards && isHazList(v.current)) out.hazards = v.current;
        }
        h = h.next;
      }
      fiber = fiber.return; depth++;
      if (out.player && out.stats) break;
    }
  }

  function scan() {
    const c = document.querySelector("canvas");
    if (!c) return false;
    const fk = Object.keys(c).find((k) => k.startsWith("__reactFiber$"));
    if (!fk) return false;
    const out = { player: null, npcs: null, hazards: null, stats: null, death: null };
    collectHooks(c[fk], out);
    if (!out.player) return false;      // player 是硬性要求, 其余允许延后命中
    for (const k of ["npcs", "hazards", "stats"]) if (out[k]) cache[k] = out[k];
    if (out.death) cache.death = out.death;
    cache.player = out.player;
    return true;
  }

  /* ================= 网络哨兵: runToken 追踪 + 防重复提交 ================= */
  const net = { lastToken: null, lastRunStartAt: 0, lastSubmitAt: 0 };
  const __origFetch = window.fetch.bind(window);
  window.fetch = async function (...a) {
    const url = typeof a[0] === "string" ? a[0] : (a[0] && a[0].url) || "";
    if (url.includes("/api/runs/start")) {
      net.lastRunStartAt = Date.now();
      try {
        const res = await __origFetch(...a); const j = await res.clone().json();
        if (j && j.runToken) net.lastToken = j.runToken;
        return res;
      } catch (e) {}
    }
    if (url.includes("/api/leaderboard/submit")) {
      net.lastSubmitAt = Date.now();   // 仅遥测, 不拦截 (拦截会误伤正常重传)
    }
    return __origFetch(...a);
  };

  /* ================= 合成输入 (挂机/连发共用) ================= */
  const heldKeys = new Set();
  function sendKey(key, down) {
    const type = down ? "keydown" : "keyup";
    if (down && heldKeys.has(key)) return;
    if (!down && !heldKeys.has(key)) return;
    down ? heldKeys.add(key) : heldKeys.delete(key);
    window.dispatchEvent(new KeyboardEvent(type, { key, bubbles: true }));
  }
  function tapKey(key, ms = 140) {
    sendKey(key, true);
    setTimeout(() => sendKey(key, false), ms);
  }
  // 失焦/切页看门狗: 游戏会清空它的按键表, 我们必须同步松开,
  // 否则恢复后"以为还按着却没动" -> 原地卡死乱跳
  function releaseAll() { for (const k of [...heldKeys]) sendKey(k, false); }
  window.addEventListener("blur", releaseAll);
  document.addEventListener("visibilitychange", () => { if (document.hidden) releaseAll(); });

  // 失焦/切页时游戏会清空自己的按键表, 我们的合成键状态必须同步释放,
  // 否则恢复后会出现"以为还按着却没在动 -> 卡死乱跳"
  function releaseAll() { for (const k of [...heldKeys]) sendKey(k, false); }
  window.addEventListener("blur", releaseAll);
  document.addEventListener("visibilitychange", () => { if (document.hidden) releaseAll(); });

  /* ================= 自动连发 (合成 F 键事件) ================= */
  let lastFire = 0;
  function autoFire(now) {
    if (!cfg.autoFireRate) return;
    if (now - lastFire < 1000 / cfg.autoFireRate) return;
    lastFire = now;
    tapKey("f", 60);
  }

  /* ================= 每帧强化循环 ================= */
  function tick() {
    try {
      const needScan = !(cache.player && typeof cache.player.hp === "number");
      if (needScan || ((cache.tick++ & 63) === 0)) scan();

      const P = cache.player;
      if (P && typeof P.hp === "number") {
        if (cfg.lockHpOn) {
          if (P.maxHp < cfg.lockHpValue) P.maxHp = cfg.lockHpValue;
          if (cfg.stealth) {
            // 隐蔽: 只在低于目标时回充, 允许真实受击发生, 服务端看到的是"会掉血但很能奶"
            if (P.hp < cfg.lockHpValue) P.hp = Math.min(cfg.lockHpValue, P.hp + Math.max(0.5, P.maxHp * 0.02));
          } else {
            P.hp = cfg.lockHpValue;
            P.invulnerableTime = Math.max(P.invulnerableTime | 0, 60);
          }
        }
        if (cfg.zeroPanic) P.panic = 0;
        if (cfg.noAtkCd && "attackCooldown" in P) P.attackCooldown = 0;
        if (cfg.infDash) { P.dashCooldown = 0; }
        if (cfg.magnetAlways) P.magnetTime = 9999;
        if (cfg.shieldAlways) P.shieldTime = 9999;
      }

      const S = cache.stats;
      if (S) {
        if (cfg.scoreMul > 1) S.multiplier = cfg.scoreMul;
        if (cfg.comboLockN > 0 && S.combo < cfg.comboLockN) {
          S.combo = cfg.comboLockN; S.lastComboAt = performance.now();
        }
      }

      if (cfg.oneKill) {
        const L = cache.npcs;
        if (Array.isArray(L)) for (const e of L) {
          if (!e || e.scanned) continue;
          e.alertLevel = 0;
          if ("visionRange" in e) { e.visionRange = 0; e.visionHeight = 0; }
          if (typeof e.maxScanHits === "number") e.maxScanHits = 1;
          if (typeof e.scanHits === "number") e.scanHits = 0;
        }
        const H = cache.hazards;
        if (Array.isArray(H)) for (const h of H) if (h && "hp" in h) h.hp = 0.01;
      }

      // ---- 极速奔跑: 劫持 vx 赋值, 游戏写多少就放大多少 ----
      if (P && cfg.speedMode && !P.__hacVx) {
        try {
          let v = P.vx;
          Object.defineProperty(P, "vx", {
            configurable: true,
            get() { return v; },
            set(nv) {
              const m = cfg.speedMode ? Math.min(6, Math.max(1.1, cfg.speedMult || 1)) : 1;
              v = nv * m;
            }
          });
          P.__hacVx = true;
        } catch (e) {}
      }

      const nowT = performance.now();

      // ---- 自动猫化: 半径内未扫描 NPC -> 一波带走 ----
      let catWanted = false;
      if (cfg.autoCatify && P && Array.isArray(cache.npcs)) {
        const pcx = P.x + P.width / 2, pcy = P.y + P.height / 2;
        for (const e of cache.npcs) {
          if (!e || e.scanned) continue;
          const dx = e.x + (e.width || 40) / 2 - pcx, dy = e.y + (e.height || 60) / 2 - pcy;
          if (dx * dx + dy * dy <= cfg.catRadius * cfg.catRadius) {
            if (typeof e.maxScanHits === "number") e.maxScanHits = 1;
            e.alertLevel = 0;
            if ("visionRange" in e) { e.visionRange = 0; e.visionHeight = 0; }
            catWanted = true;
          }
        }
        if (catWanted && nowT - lastFire > 130) { lastFire = nowT; tapKey("f", 50); }
      }

      // ---- 自动挂机: 前进 + 遇障跳跃 + 卡死自愈 ----
      if (cfg.autoPlay && P && typeof P.x === "number") {
        // 移动键看门狗: 每 ~0.8s 重发一次 keydown, 对抗游戏侧丢键/失焦清表
        P._reassert = (P._reassert || 0) + 1;
        if ((P._reassert % 48) === 0 && !heldKeys.has("__cool")) {
          heldKeys.delete("d"); sendKey("d", true);
        }
        if (!heldKeys.has("d")) sendKey("d", true);

        let jump = false;
        const look = 150 + Math.max(0, P.vx) * 55;
        const pyTop = P.y, pyBot = P.y + P.height;
        const H = cache.hazards;
        if (Array.isArray(H)) for (const h of H) {
          if (!h || h.occupied || typeof h.x !== "number") continue;
          const ahead = h.x - (P.x + P.width);
          if (ahead > -34 && ahead < look) {
            const hh = h.height || 40;
            if (!(h.y + hh < pyTop + 12 || h.y > pyBot - 8)) { jump = true; break; }
          }
        }
        // NPC 迎面也跳 (巡逻猫挡路)
        if (!jump && Array.isArray(cache.npcs)) for (const e of cache.npcs) {
          if (!e || e.scanned) continue;
          const ahead = e.x - (P.x + P.width);
          if (ahead > -20 && ahead < look * .7 && !(e.y > pyBot || e.y + (e.height||60) < pyTop)) { jump = true; break; }
        }
        // 卡住自愈: 记录基准x, 长时间无位移 -> 先彻底松键再重新按 + 跳
        if (Math.abs((P.x|0) - (P._lx|0)) < 1 && !P.isDashing) {
          P._stuck = (P._stuck || 0) + 1;
          if (P._stuck === 50) { sendKey("d", false); }
          if (P._stuck >= 56) { jump = true; P._stuck = 0; heldKeys.delete("d"); sendKey("d", true); }
        } else { P._stuck = 0; P._lx = P.x; }

        if (jump && nowT - (P._lastJump || 0) > 360) { P._lastJump = nowT; tapKey(" ", 175); }
      } else if (heldKeys.has("d")) {
        sendKey("d", false); // 关闭挂机时松开
      }

      // ---- 弹窗/聊天自动关闭, 防止挂机软锁 ----
      if (cfg.autoDismiss && (cfg.autoPlay || cfg.autoCatify)) {
        if ((nowT | 0) % 900 < 17) {
          const btn = [...document.querySelectorAll("button,div[role=button],span")]
            .find(e => /^(等一下|继续|跳过|关闭|确定)$/.test((e.textContent || "").trim()) && e.getBoundingClientRect().width > 0);
          if (btn) btn.click();
        }
      }

      autoFire(nowT);
      updateBadge();
    } catch (e) { /* 场景重建期间吞错 */ }
    requestAnimationFrame(tick);
  }

  /* ================= UI: 悬浮球 + 面板 (Shadow DOM) ================= */
  const host = document.createElement("div");
  host.id = "hac-trainer-host";
  document.documentElement.appendChild(host);
  const sh = host.attachShadow({ mode: "closed" });

  const style = document.createElement("style");
  style.textContent = `
    #ball{position:fixed;left:18px;top:38%;width:56px;height:56px;border-radius:50%;
      background:radial-gradient(circle at 30% 30%,#7ef9ff,#1d6ef2 70%);
      box-shadow:0 0 14px #29b6ff88,0 4px 10px #0006;z-index:2147483647;cursor:grab;
      display:flex;align-items:center;justify-content:center;font:bold 12px monospace;color:#fff;
      user-select:none;touch-action:none;border:2px solid #bdf6ff}
    #panel{position:fixed;left:86px;top:32%;width:300px;max-height:80vh;overflow:auto;
      background:#0c1220f2;color:#cfe9ff;font:12px/1.5 system-ui,sans-serif;border:1px solid #2b6cb0aa;
      border-radius:12px;padding:10px 12px;z-index:2147483647;box-shadow:0 8px 28px #000a;display:none}
    .row{display:flex;align-items:center;justify-content:space-between;margin:5px 0;gap:8px}
    .sw{position:relative;width:36px;height:19px;background:#24344d;border-radius:10px;cursor:pointer;flex:none}
    .sw::after{content:"";position:absolute;top:2px;left:2px;width:15px;height:15px;border-radius:50%;background:#8fa8c8;transition:.15s}
    .sw.on{background:#1258a8}.sw.on::after{left:19px;background:#7ef9ff}
    input[type=number]{width:64px;background:#101a2c;color:#fff;border:1px solid #35507a;border-radius:5px;padding:2px 5px}
    input[type=range]{width:110px;accent-color:#39a7ff}
    h4{margin:9px 0 3px;color:#7ef9ff;font-size:12px;border-bottom:1px dashed #2b6cb088;padding-bottom:2px}
    button.act{background:#154a8f;color:#dff3ff;border:none;border-radius:6px;padding:3px 10px;cursor:pointer}
    small{color:#6d89ad}`;
  sh.appendChild(style);

  const ball = document.createElement("div");
  ball.id = "ball"; ball.textContent = "❤--";
  sh.appendChild(ball);
  const panel = document.createElement("div");
  panel.id = "panel"; sh.appendChild(panel);

  function row(label, make) {
    const r = document.createElement("div"); r.className = "row";
    const s = document.createElement("span"); s.textContent = label;
    r.append(s); make(r); panel.append(r); return r;
  }
  function toggle(get, set) {
    const sw = document.createElement("div"); sw.className = "sw" + (get() ? " on" : "");
    sw.onclick = () => { set(!get()); sw.classList.toggle("on", get()); };
    return sw;
  }
  function num(val, min, max, step, onchg) {
    const i = document.createElement("input"); i.type = "number";
    i.value = val; i.min = min; i.max = max; i.step = step ?? 1;
    i.onchange = () => onchg(Number(i.value)); return i;
  }

  panel.append(Object.assign(document.createElement("h4"), { textContent: "♥ 生命" }));
  row("锁血 (无敌)", (r) => {
    r.append(num(cfg.lockHpValue, 1, 99999, 1, (v) => cfg.lockHpValue = v));
    r.append(toggle(() => cfg.lockHpOn, (v) => cfg.lockHpOn = v));
  });
  row("自定义生命值", (r) => {
    const b = document.createElement("button"); b.className = "act"; b.textContent = "应用";
    b.onclick = () => {
      const P = cache.player; if (!P) return alert("先进入对局 (canvas 未挂载)");
      P.hp = cfg.lockHpValue; P.maxHp = Math.max(P.maxHp, cfg.lockHpValue);
      b.textContent = "✓"; setTimeout(() => b.textContent = "应用", 800);
    };
    r.append(b);
  });
  row("恐慌条清零", (r) => r.append(toggle(() => cfg.zeroPanic, (v) => cfg.zeroPanic = v)));
  row("隐蔽回血模式", (r) => r.append(toggle(() => cfg.stealth, (v) => cfg.stealth = v)));
  row("自动关弹窗", (r) => r.append(toggle(() => cfg.autoDismiss, (v) => cfg.autoDismiss = v)));
  row("向前瞬移", (r) => {
    const b = document.createElement("button"); b.className = "act"; b.textContent = "+100m";
    b.onclick = async () => {
      const P = cache.player; if (!P) return alert("先进入对局");
      b.textContent = "...";
      for (let i = 0; i < 20; i++) {           // 20 步 x 120px, 步间让生成器填充
        P.x += 120; await new Promise(q => requestAnimationFrame(q));
        if (i % 4 === 3) await new Promise(q => setTimeout(q, 60));
      }
      b.textContent = "+100m";
    };
    r.append(b);
  });
  row("速死结算 (Fast DIE)", (r) => {
    const b = document.createElement("button"); b.className = "act"; b.textContent = "💀 立即";
    b.onclick = () => { b.textContent = dieNow() ? "💀✓" : "需对局中"; setTimeout(() => b.textContent = "💀 立即", 900); };
    r.append(b);
  });

  panel.append(Object.assign(document.createElement("h4"), { textContent: "⚔ 攻击" }));
  row("攻击率·无冷却", (r) => r.append(toggle(() => cfg.noAtkCd, (v) => cfg.noAtkCd = v)));
  row("自动连发 (次/秒)", (r) => {
    const lab = document.createElement("span"); lab.textContent = "0";
    const rg = document.createElement("input"); rg.type = "range";
    rg.min = 0; rg.max = 20; rg.step = 1; rg.value = cfg.autoFireRate;
    rg.oninput = () => { cfg.autoFireRate = Number(rg.value); lab.textContent = rg.value; };
    r.append(lab, rg);
  });
  row("一击秒杀 (NPC)", (r) => r.append(toggle(() => cfg.oneKill, (v) => cfg.oneKill = v)));
  row("自动猫化·半径", (r) => {
    r.append(num(cfg.catRadius, 80, 900, 20, (v) => cfg.catRadius = v));
    r.append(toggle(() => cfg.autoCatify, (v) => cfg.autoCatify = v));
  });

  panel.append(Object.assign(document.createElement("h4"), { textContent: "⚡ 增强" }));
  row("自动挂机 (AFK)", (r) => r.append(toggle(() => cfg.autoPlay, (v) => cfg.autoPlay = v)));
  row("极速奔跑", (r) => {
    r.append(num(cfg.speedMult, 1.1, 6, 0.1, (v) => {
      // 上限 6x: 再快会跑在地图生成器前面 (障碍/NPC 来不及刷出)
      if (!isFinite(v)) v = 1.1;
      cfg.speedMult = Math.min(6, Math.max(1.1, v));
    }));
    r.append(toggle(() => cfg.speedMode, (v) => {
      cfg.speedMode = v;
      const P = cache.player;
      if (!v && P && P.__hacVx) {           // 关闭时还原普通属性
        try {
          const cur = P.vx;
          delete P.vx; delete P.__hacVx;
          P.vx = cur;
        } catch (e) {}
      }
    }));
  });
  row("无限冲刺", (r) => r.append(toggle(() => cfg.infDash, (v) => cfg.infDash = v)));
  row("磁铁常驻", (r) => r.append(toggle(() => cfg.magnetAlways, (v) => cfg.magnetAlways = v)));
  row("护盾常驻", (r) => r.append(toggle(() => cfg.shieldAlways, (v) => cfg.shieldAlways = v)));
  row("分数倍率", (r) => r.append(
    num(cfg.scoreMul, 1, 1000, 1, (v) => cfg.scoreMul = Math.max(1, v))));
  row("连击锁定 ≥ N", (r) => r.append(
    num(cfg.comboLockN, 0, 9999, 1, (v) => cfg.comboLockN = v)));

  panel.append(Object.assign(document.createElement("h4"), { textContent: "🌐 上传" }));
  row("runToken", (r) => {
    const s2 = document.createElement("small");
    setInterval(() => {
      s2.textContent = net.lastToken
        ? "有效 · " + new Date(net.lastRunStartAt).toLocaleTimeString()
        : "无 (用「再跑一局」重开获取)";
    }, 1000);
    r.append(s2);
  });
  panel.append(Object.assign(document.createElement("small"),
    { textContent: "数据来自 React Fiber 扫描；对局重开会自动重连。悬浮球可拖动，点击展开。" }));

  /* ---- 悬浮球: 点击开合 + 拖动 ---- */
  let open = false, drag = null, moved = false;
  ball.addEventListener("pointerdown", (e) => {
    drag = { x: e.clientX, y: e.clientY, l: parseInt(ball.style.left || 18), t: parseInt(ball.style.top || "38%") === 38 ? ball.offsetTop : parseInt(ball.style.top) };
    moved = false; ball.setPointerCapture(e.pointerId);
  });
  ball.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 4) moved = true;
    ball.style.left = Math.max(0, drag.l + dx) + "px";
    ball.style.top = Math.max(0, drag.t + dy) + "px";
  });
  ball.addEventListener("pointerup", () => {
    if (!moved) { open = !open; panel.style.display = open ? "block" : "none"; }
    drag = null;
  });

  function updateBadge() {
    if (!open) {
      const P = cache.player;
      ball.textContent = P && typeof P.hp === "number"
        ? "❤" + Math.round((P.hp / (P.maxHp || 1)) * 100)
        : "❤--";
    }
  }

  requestAnimationFrame(tick);
  console.log("[HAC-Trainer] loaded — 悬浮球已就绪");
})();
