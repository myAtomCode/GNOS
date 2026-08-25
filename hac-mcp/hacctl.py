#!/usr/bin/env python3
# hacctl.py — send a tool call to the running hac-mcp server and print the reply.
import json, sys, time

FIFO = "/tmp/opencode/hac-cmd.fifo"
OUT = "/tmp/opencode/hac-out.log"

def call(name, args=None, timeout=90, want_image=False):
    rid = int(time.time() * 1000) % 100000
    req = {"jsonrpc": "2.0", "id": rid, "method": "tools/call",
           "params": {"name": name, "arguments": args or {}}}
    with open(FIFO, "w") as f:
        f.write(json.dumps(req) + "\n")
    t0 = time.time()
    with open(OUT, "r") as log:
        log.seek(0, 2)
        while time.time() - t0 < timeout:
            line = log.readline()
            if not line:
                time.sleep(0.15); continue
            try: m = json.loads(line)
            except Exception: continue
            if m.get("id") != rid: continue
            res = m.get("result", m.get("error"))
            for c in (res or {}).get("content", []):
                if c["type"] == "image" and want_image:
                    import base64
                    path = "/tmp/opencode/hac-ctl.png"
                    open(path, "wb").write(base64.b64decode(c["data"]))
                    print(f"[image saved {path}]")
                elif c["type"] == "text":
                    print(c["text"])
            return
    print(f"(timeout waiting for {name})")

if __name__ == "__main__":
    name = sys.argv[1] if len(sys.argv) > 1 else "page_info"
    raw = json.loads(sys.argv[2]) if len(sys.argv) > 2 and not sys.argv[2].startswith("--") else {}
    if isinstance(raw, str):
        raw = {"expression": raw}   # eval_js 快捷方式
    args = raw
    img = "--img" in sys.argv
    call(name, args, want_image=img)
