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