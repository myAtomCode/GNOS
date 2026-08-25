#!/bin/bash
# 自动查找所有包含 "npx" 的进程并强制终止（慎用）

# 获取匹配的进程 PID（-f 匹配完整命令行）
pids=$(pgrep -f "npx" 2>/dev/null)

if [ -z "$pids" ]; then
    echo "未找到任何 npx 进程。"
    exit 0
fi

echo "找到以下 npx 进程："
ps -p $pids -o pid,cmd --no-headers

# 直接强制终止（无确认）
echo "正在强制终止这些进程..."
kill -9 $pids 2>/dev/null

# 检查是否还有残留
remaining=$(pgrep -f "npx" 2>/dev/null)
if [ -z "$remaining" ]; then
    echo "所有 npx 进程已终止。"
else
    echo "警告：仍有进程残留（PID: $remaining），请手动检查。"
fi
