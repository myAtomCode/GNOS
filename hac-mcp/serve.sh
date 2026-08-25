#!/usr/bin/env bash
# serve.sh — keep the hac-mcp stdio server alive in the background.
# Commands are written to cmd.pipe; responses land in out.log.
cd "$(dirname "$0")"
FIFO=/tmp/opencode/hac-cmd.fifo
rm -f "$FIFO"; mkfifo "$FIFO"
# hold stdin open forever from the fifo's writer side
exec 3<>"$FIFO"
node mcp-server.mjs <&3 >> /tmp/opencode/hac-out.log 2>&1
