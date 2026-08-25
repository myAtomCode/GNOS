# diff --git a/frontend/src/App.jsx b/frontend/src/App.jsx

**Session ID:** ses_fcde38239ffexeJXoAGU741iJN
**Created:** 8/24/2026, 12:52:03 PM
**Updated:** 8/24/2026, 12:52:10 PM

---

## User

**Vocaloid 大冒险**

### 游戏设定

- **世界观**：Vocaloid 虚拟歌姬们被困在「数据深渊」中，玩家扮演制作人，通过音乐节奏闯关，逐层解锁并拯救初音未来、镜音铃连、巡音流歌等角色。
- **核心玩法**：节奏打击 + 角色收集 + 剧情解锁，每关对应一首经典 Vocaloid 曲目。
- **技术栈**：前端 React + Canvas 渲染节奏轨道；后端 Node.js + Express 负责用户存档、排行榜、关卡数据下发。

---

### 前端代码（React + Canvas）

```jsx
import React, { useEffect, useRef, useState } from 'react';

const VOCALOIDS = ['初音未来', '镜音铃', '镜音连', '巡音流歌', 'KAITO', 'MEIKO'];

export default function VocaloidAdventure() {
  const canvasRef = useRef(null);
  const [score, setScore] = useState(0);
  const [combo, setCombo] = useState(0);
  const [unlocked, setUnlocked] = useState(['初音未来']);
  const [notes, setNotes] = useState([]);
  const [level, setLevel] = useState(1);

  // 生成音符
  useEffect(() => {
    const interval = setInterval(() => {
      setNotes(prev => [...prev, {
        id: Date.now(),
        lane: Math.floor(Math.random() * 4),
        y: 0,
        speed: 3 + level * 0.5
      }]);
    }, 800 - level * 50);
    return () => clearInterval(interval);
  }, [level]);

  // Canvas 渲染
  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas.getContext('2d');
    let animId;

    const draw = () => {
      ctx.fillStyle = '#1a1a2e';
      ctx.fillRect(0, 0, 400, 600);

      // 绘制轨道
      for (let i = 0; i < 4; i++) {
        ctx.strokeStyle = '#39ff14';
        ctx.beginPath();
        ctx.moveTo(i * 100 + 50, 0);
        ctx.lineTo(i * 100 + 50, 600);
        ctx.stroke();
      }

      // 判定线
      ctx.fillStyle = '#ff6b9d';
      ctx.fillRect(0, 520, 400, 4);

      // 绘制音符
      notes.forEach(note => {
        note.y += note.speed;
        const colors = ['#39ff14', '#ff6b9d', '#00d4ff', '#ffdd57'];
        ctx.fillStyle = colors[note.lane];
        ctx.beginPath();
        ctx.arc(note.lane * 100 + 50, note.y, 20, 0, Math.PI * 2);
        ctx.fill();
      });

      // 清除已落下的音符
      setNotes(prev => prev.filter(n => n.y < 620));

      animId = requestAnimationFrame(draw);
    };

    draw();
    return () => cancelAnimationFrame(animId);
  }, [notes]);

  // 键盘输入判定
  const handleKey = (e) => {
    const keyMap = { 'd': 0, 'f': 1, 'j': 2, 'k': 3 };
    const lane = keyMap[e.key];
    if (lane === undefined) return;

    const hitNote = notes.find(n => n.lane === lane && n.y > 480 && n.y < 560);
    if (hitNote) {
      setNotes(prev => prev.filter(n => n.id !== hitNote.id));
      setScore(s => s + 100 * (1 + combo * 0.1));
      setCombo(c => c + 1);

      // 每500分解锁角色
      if (Math.floor(score / 500) > unlocked.length - 1) {
        const nextChar = VOCALOIDS[unlocked.length];
        if (nextChar) setUnlocked(u => [...u, nextChar]);
      }
    } else {
      setCombo(0);
    }
  };

  useEffect(() => {
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [notes, combo, score, unlocked]);

  return (
    <div style={{ display: 'flex', gap: '20px', padding: '20px', background: '#0f0f23', minHeight: '100vh', color: '#fff' }}>
      <div>
        <h1> Vocaloid 大冒险</h1>
        <p>分数: {Math.floor(score)} | Combo: {combo} | 关卡: {level}</p>
        <p>已解锁: {unlocked.join(', ')}</p>
        <canvas ref={canvasRef} width={400} height={600} style={{ border: '2px solid #39ff14', borderRadius: '8px' }} />
        <p style={{ color: '#888' }}>按键: D F J K 对应四条轨道</p>
      </div>
    </div>
  );
}
```

---

### 后端代码（Node.js + Express）

```javascript
const express = require('express');
const cors = require('cors');
const app = express();

app.use(cors());
app.use(express.json());

// 模拟数据库
const players = {};
const leaderboard = [];

// 关卡数据
const levels = [
  { id: 1, name: 'メルト', bpm: 120, notes: 150 },
  { id: 2, name: '千本桜', bpm: 154, notes: 280 },
  { id: 3, name: 'ロキ', bpm: 156, notes: 320 },
];

// 注册/登录
app.post('/api/login', (req, res) => {
  const { username } = req.body;
  if (!players[username]) {
    players[username] = { score: 0, level: 1, unlocked: ['初音未来'] };
  }
  res.json({ success: true, player: players[username] });
});

// 获取关卡列表
app.get('/api/levels', (req, res) => {
  res.json(levels);
});

// 提交分数
app.post('/api/score', (req, res) => {
  const { username, score, combo } = req.body;
  if (players[username]) {
    players[username].score += score;
    // 解锁角色逻辑
    const VOCALOIDS = ['初音未来', '镜音铃', '镜音连', '巡音流歌', 'KAITO', 'MEIKO'];
    const unlockCount = Math.min(Math.floor(players[username].score / 500) + 1, VOCALOIDS.length);
    players[username].unlocked = VOCALOIDS.slice(0, unlockCount);
  }
  leaderboard.push({ username, score, combo, time: Date.now() });
  leaderboard.sort((a, b) => b.score - a.score);
  res.json({ success: true, player: players[username] });
});

// 排行榜
app.get('/api/leaderboard', (req, res) => {
  res.json(leaderboard.slice(0, 10));
});

app.listen(3001, () => console.log(' Vocaloid 大冒险服务器运行在 http://localhost:3001'));
```

---

### 前后端配合说明

| 功能 | 前端 | 后端 |
| ------ |------ |------ |
| 用户登录 | 发送 `POST /api/login` | 创建玩家数据并返回 |
| 关卡加载 | 请求 `GET /api/levels` | 返回曲目、BPM、音符数 |
| 实时计分 | 本地计算 Combo 和分数 | `POST /api/score` 持久化 |
| 角色解锁 | 前端预览动画 | 后端校验并记录解锁状态 |
| 排行榜 | 请求 `GET /api/leaderboard` | 返回 Top 10 |

### 运行方式

```bash
# 后端
npm init -y && npm install express cors
node server.js

# 前端（Create React App）
npx create-react-app vocaloid-adventure
# 将前端代码放入 src/App.jsx
npm start
```

游戏跑起来后，按 **D F J K** 打击下落的音符，每积累 500 分解锁一位新的 Vocaloid 角色，分数同步上传后端写入排行榜。 

---

## Assistant (Build · Miku · 3.8s)

**Tool: write**

**Input:**
```json
{
  "filePath": "Node.js",
  "content": "import React, { useEffect, useRef, useState } from 'react';\n\nconst VOCALOIDS = ['初音未来', '镜音铃', '镜音连', '巡音流歌', 'KAITO', 'MEIKO'];\n\nexport default function VocaloidAdventure() {\n  const canvasRef = useRef(null);\n  const [score, setScore] = useState(0);\n  const [combo, setCombo] = useState(0);\n  const [unlocked, setUnlocked] = useState(['初音未来']);\n  const [notes, setNotes] = useState([]);\n  const [level, setLevel] = useState(1);\n\n  // 生成音符\n  useEffect(() => {\n    const interval = setInterval(() => {\n      setNotes(prev => [...prev, {\n        id: Date.now(),\n        lane: Math.floor(Math.random() * 4),\n        y: 0,\n        speed: 3 + level * 0.5\n      }]);\n    }, 800 - level * 50);\n    return () => clearInterval(interval);\n  }, [level]);\n\n  // Canvas 渲染\n  useEffect(() => {\n    const canvas = canvasRef.current;\n    const ctx = canvas.getContext('2d');\n    let animId;\n\n    const draw = () => {\n      ctx.fillStyle = '#1a1a2e';\n      ctx.fillRect(0, 0, 400, 600);\n\n      // 绘制轨道\n      for (let i = 0; i < 4; i++) {\n        ctx.strokeStyle = '#39ff14';\n        ctx.beginPath();\n        ctx.moveTo(i * 100 + 50, 0);\n        ctx.lineTo(i * 100 + 50, 600);\n        ctx.stroke();\n      }\n\n      // 判定线\n      ctx.fillStyle = '#ff6b9d';\n      ctx.fillRect(0, 520, 400, 4);\n\n      // 绘制音符\n      notes.forEach(note => {\n        note.y += note.speed;\n        const colors = ['#39ff14', '#ff6b9d', '#00d4ff', '#ffdd57'];\n        ctx.fillStyle = colors[note.lane];\n        ctx.beginPath();\n        ctx.arc(note.lane * 100 + 50, note.y, 20, 0, Math.PI * 2);\n        ctx.fill();\n      });\n\n      // 清除已落下的音符\n      setNotes(prev => prev.filter(n => n.y < 620));\n\n      animId = requestAnimationFrame(draw);\n    };\n\n    draw();\n    return () => cancelAnimationFrame(animId);\n  }, [notes]);\n\n  // 键盘输入判定\n  const handleKey = (e) => {\n    const keyMap = { 'd': 0, 'f': 1, 'j': 2, 'k': 3 };\n    const lane = keyMap[e.key];\n    if (lane === undefined) return;\n\n    const hitNote = notes.find(n => n.lane === lane && n.y > 480 && n.y < 560);\n    if (hitNote) {\n      setNotes(prev => prev.filter(n => n.id !== hitNote.id));\n      setScore(s => s + 100 * (1 + combo * 0.1));\n      setCombo(c => c + 1);\n\n      // 每500分解锁角色\n      if (Math.floor(score / 500) > unlocked.length - 1) {\n        const nextChar = VOCALOIDS[unlocked.length];\n        if (nextChar) setUnlocked(u => [...u, nextChar]);\n      }\n    } else {\n      setCombo(0);\n    }\n  };\n\n  useEffect(() => {\n    window.addEventListener('keydown', handleKey);\n    return () => window.removeEventListener('keydown', handleKey);\n  }, [notes, combo, score, unlocked]);\n\n  return (\n    <div style={{ display: 'flex', gap: '20px', padding: '20px', background: '#0f0f23', minHeight: '100vh', color: '#fff' }}>\n      <div>\n        <h1> Vocaloid 大冒险</h1>\n        <p>分数: {Math.floor(score)} | Combo: {combo} | 关卡: {level}</p>\n        <p>已解锁: {unlocked.join(', ')}</p>\n        <canvas ref={canvasRef} width={400} height={600} style={{ border: '2px solid #39ff14', borderRadius: '8px' }} />\n        <p style={{ color: '#888' }}>按键: D F J K 对应四条轨道</p>\n      </div>\n    </div>\n  );\n}"
}
```

**Output:**
```
Wrote file successfully.
```

**Tool: bash**

**Input:**
```json
{
  "command": "grep -rn \"**Vocaloid 大冒险**\n\n### 游戏设定\n\n- **世界观**：Vocaloid 虚拟歌姬们被困在「数据深渊」中，玩家扮演制作人，通过音乐节奏闯关，\" ."
}
```

**Output:**
```
./Node.js:1:import React, { useEffect, useRef, useState } from 'react';
./Node.js:2:
./Node.js:3:const VOCALOIDS = ['初音未来', '镜音铃', '镜音连', '巡音流歌', 'KAITO', 'MEIKO'];
./Node.js:4:
./Node.js:5:export default function VocaloidAdventure() {
./Node.js:6:  const canvasRef = useRef(null);
./Node.js:7:  const [score, setScore] = useState(0);
./Node.js:8:  const [combo, setCombo] = useState(0);
./Node.js:9:  const [unlocked, setUnlocked] = useState(['初音未来']);
./Node.js:10:  const [notes, setNotes] = useState([]);
./Node.js:11:  const [level, setLevel] = useState(1);
./Node.js:12:
./Node.js:13:  // 生成音符
./Node.js:14:  useEffect(() => {
./Node.js:15:    const interval = setInterval(() => {
./Node.js:16:      setNotes(prev => [...prev, {
./Node.js:17:        id: Date.now(),
./Node.js:18:        lane: Math.floor(Math.random() * 4),
./Node.js:19:        y: 0,
./Node.js:20:        speed: 3 + level * 0.5
./Node.js:21:      }]);
./Node.js:22:    }, 800 - level * 50);
./Node.js:23:    return () => clearInterval(interval);
./Node.js:24:  }, [level]);
./Node.js:25:
./Node.js:26:  // Canvas 渲染
./Node.js:27:  useEffect(() => {
./Node.js:28:    const canvas = canvasRef.current;
./Node.js:29:    const ctx = canvas.getContext('2d');
./Node.js:30:    let animId;
./Node.js:31:
./Node.js:32:    const draw = () => {
./Node.js:33:      ctx.fillStyle = '#1a1a2e';
./Node.js:34:      ctx.fillRect(0, 0, 400, 600);
./Node.js:35:
./Node.js:36:      // 绘制轨道
./Node.js:37:      for (let i = 0; i < 4; i++) {
./Node.js:38:        ctx.strokeStyle = '#39ff14';
./Node.js:39:        ctx.beginPath();
./Node.js:40:        ctx.moveTo(i * 100 + 50, 0);
./Node.js:41:        ctx.lineTo(i * 100 + 50, 600);
./Node.js:42:        ctx.stroke();
./Node.js:43:      }
./Node.js:44:
./Node.js:45:      // 判定线
./Node.js:46:      ctx.fillStyle = '#ff6b9d';
./Node.js:47:      ctx.fillRect(0, 520, 400, 4);
./Node.js:48:
./Node.js:49:      // 绘制音符
./Node.js:50:      notes.forEach(note => {
./Node.js:51:        note.y += note.speed;
./Node.js:52:        const colors = ['#39ff14', '#ff6b9d', '#00d4ff', '#ffdd57'];
./Node.js:53:        ctx.fillStyle = colors[note.lane];
./Node.js:54:        ctx.beginPath();
./Node.js:55:        ctx.arc(note.lane * 100 + 50, note.y, 20, 0, Math.PI * 2);
./Node.js:56:        ctx.fill();
./Node.js:57:      });
./Node.js:58:
./Node.js:59:      // 清除已落下的音符
./Node.js:60:      setNotes(prev => prev.filter(n => n.y < 620));
./Node.js:61:
./Node.js:62:      animId = requestAnimationFrame(draw);
./Node.js:63:    };
./Node.js:64:
./Node.js:65:    draw();
./Node.js:66:    return () => cancelAnimationFrame(animId);
./Node.js:67:  }, [notes]);
./Node.js:68:
./Node.js:69:  // 键盘输入判定
./Node.js:70:  const handleKey = (e) => {
./Node.js:71:    const keyMap = { 'd': 0, 'f': 1, 'j': 2, 'k': 3 };
./Node.js:72:    const lane = keyMap[e.key];
./Node.js:73:    if (lane === undefined) return;
./Node.js:74:
./Node.js:75:    const hitNote = notes.find(n => n.lane === lane && n.y > 480 && n.y < 560);
./Node.js:76:    if (hitNote) {
./Node.js:77:      setNotes(prev => prev.filter(n => n.id !== hitNote.id));
./Node.js:78:      setScore(s => s + 100 * (1 + combo * 0.1));
./Node.js:79:      setCombo(c => c + 1);
./Node.js:80:
./Node.js:81:      // 每500分解锁角色
./Node.js:82:      if (Math.floor(score / 500) > unlocked.length - 1) {
./Node.js:83:        const nextChar = VOCALOIDS[unlocked.length];
./Node.js:84:        if (nextChar) setUnlocked(u => [...u, nextChar]);
./Node.js:85:      }
./Node.js:86:    } else {
./Node.js:87:      setCombo(0);
./Node.js:88:    }
./Node.js:89:  };
./Node.js:90:
./Node.js:91:  useEffect(() => {
./Node.js:92:    window.addEventListener('keydown', handleKey);
./Node.js:93:    return () => window.removeEventListener('keydown', handleKey);
./Node.js:94:  }, [notes, combo, score, unlocked]);
./Node.js:95:
./Node.js:96:  return (
./Node.js:97:    <div style={{ display: 'flex', gap: '20px', padding: '20px', background: '#0f0f23', minHeight: '100vh', color: '#fff' }}>
./Node.js:98:      <div>
./Node.js:99:        <h1> Vocaloid 大冒险</h1>
./Node.js:100:        <p>分数: {Math.floor(score)} | Combo: {combo} | 关卡: {level}</p>
./Node.js:101:        <p>已解锁: {unlocked.join(', ')}</p>
./Node.js:102:        <canvas ref={canvasRef} width={400} height={600} style={{ border: '2px solid #39ff14', borderRadius: '8px' }} />
./Node.js:103:        <p style={{ color: '#888' }}>按键: D F J K 对应四条轨道</p>
./Node.js:104:      </div>
./Node.js:105:    </div>
./Node.js:106:  );
./Node.js:107:}

```

---

## Assistant (Build · Miku · 3.9s)

diff --git a/Node.js b/Node.js
--- a/Node.js
+++ b/Node.js
@@ -1,3 +1,3 @@
-import React, { useEffect, useRef, useState } from 'react';
+import React, { useEffect, useRef, useState } from 'react';
 
 const VOCALOIDS = ['初音未来', '镜音铃', '镜音连', '巡音流歌', 'KAITO', 'MEIKO'];
@@ -5,7 +5,7 @@
 export default function VocaloidAdventure() {
   const canvasRef = useRef(null);
   const [score, setScore] = useState(0);
   const [combo, setCombo] = useState(0);
-  const [unlocked, setUnlocked] = useState(['初音未来']);
+  const [unlocked, setUnlocked] = useState(['初音未来']);
   const [notes, setNotes] = useState([]);
   const [level, setLevel] = useState

---

