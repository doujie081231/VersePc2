#!/bin/bash
set -e
BASE="https://verselauncher.cn/api/feedback"
cd /www/wwwroot/default

echo "=== 1. 获取验证码 ==="
CAP=$(curl -s "$BASE/captcha")
echo "$CAP" | head -c 200
echo ""
CAP_ID=$(echo "$CAP" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).id")
# 从 KV 取验证码明文（测试用，服务器本地可读）
CAP_CODE=$(cat ".data/captcha/$CAP_ID" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).code")
echo "captchaId=$CAP_ID code=$CAP_CODE"

echo "=== 2. 获取签名密钥 ==="
SIGNINFO=$(curl -s -H "Referer: https://verselauncher.cn/feedback" "$BASE/signinfo")
echo "$SIGNINFO" | head -c 200
echo ""
SIGN_KEY=$(echo "$SIGNINFO" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).key")

echo "=== 3. 构造签名并提交（带附件） ==="
cat > /tmp/sig.mjs <<'EOF'
import crypto from 'crypto';
const key = process.argv[2];
const parts = process.argv.slice(3);
const h = crypto.createHmac('sha256', Buffer.from(key, 'hex')).update(parts.join('|')).digest('hex');
process.stdout.write(h);
EOF
NONCE=$(node -p "require('crypto').randomBytes(16).toString('hex').slice(0,20)")
TS=$(node -p "Date.now()")
TITLE="测试提交"
DESC="这是一段端到端测试描述，用于验证签名链路。"
CATEGORY="启动问题"
SIG=$(node /tmp/sig.mjs "$SIGN_KEY" "verse-feedback-v1" "$CAP_ID" "$NONCE" "$TS" "versepc2" "$CATEGORY" "$TITLE" "$DESC")
echo "nonce=$NONCE"
echo "首条测试文件" > /tmp/feedback-test.txt
RESP=$(curl -s -X POST "$BASE" \
  -F "client=versepc2" \
  -F "title=测试提交" \
  -F "description=这是一段端到端测试描述，用于验证签名链路。" \
  -F "category=启动问题" \
  -F "captchaId=$CAP_ID" \
  -F "captchaAnswer=$CAP_CODE" \
  -F "nonce=$NONCE" \
  -F "ts=$TS" \
  -F "sig=$SIG" \
  -F "file1=@/tmp/feedback-test.txt")
echo "RESP=$RESP"
FB_ID=$(echo "$RESP" | node -p "const j=JSON.parse(require('fs').readFileSync(0,'utf8')); j.ok?j.id:'FAIL'")
echo "feedback id=$FB_ID"
if [ "$FB_ID" = "FAIL" ]; then echo ">>>签名提交流程失败"; exit 1; fi

echo "=== 4. 校验落盘 ==="
cat ".data/feedback/$FB_ID" | node -p "const j=JSON.parse(require('fs').readFileSync(0,'utf8')); JSON.stringify({id:j.id,category:j.category,client:j.client,files:j.files},null,0)"
ls -la ".data/feedback-files/$FB_ID/"

echo "=== 5. 重放同一 nonce -> 应 403 replay ==="
CAP2=$(curl -s "$BASE/captcha")
CAP2_ID=$(echo "$CAP2" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).id")
CAP2_CODE=$(cat ".data/captcha/$CAP2_ID" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).code")
TS2=$(node -p "Date.now()")
SIG2=$(node /tmp/sig.mjs "$SIGN_KEY" "verse-feedback-v1" "$CAP2_ID" "$NONCE" "$TS2" "versepc2" "启动问题" "重放测试" "同一 nonce 重放。")
CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE" \
  -F "client=versepc2" -F "title=重放测试" -F "description=同一 nonce 重放。" -F "category=启动问题" \
  -F "captchaId=$CAP2_ID" -F "captchaAnswer=$CAP2_CODE" \
  -F "nonce=$NONCE" -F "ts=$TS2" -F "sig=$SIG2")
echo "replay http=$CODE (期望 403)"

echo "=== 6. 伪造签名 -> 应 403 ==="
CAP3=$(curl -s "$BASE/captcha")
CAP3_ID=$(echo "$CAP3" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).id")
CAP3_CODE=$(cat ".data/captcha/$CAP3_ID" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).code")
FAKE_SIG=$(printf '0%.0s' {1..64})
CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE" \
  -F "client=versepc2" -F "title=伪造测试" -F "description=错误签名。" -F "category=启动问题" \
  -F "captchaId=$CAP3_ID" -F "captchaAnswer=$CAP3_CODE" \
  -F "nonce=FAKEnonce___1" -F "ts=$(node -p "Date.now()")" -F "sig=$FAKE_SIG")
echo "forged http=$CODE (期望 403)"

echo "=== 7. 当日第二次提交 -> 应 429 ==="
CAP4=$(curl -s "$BASE/captcha")
CAP4_ID=$(echo "$CAP4" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).id")
CAP4_CODE=$(cat ".data/captcha/$CAP4_ID" | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).code")
NONCE4=$(node -p "require('crypto').randomBytes(16).toString('hex').slice(0,20)")
TS4=$(node -p "Date.now()")
SIG4=$(node /tmp/sig.mjs "$SIGN_KEY" "verse-feedback-v1" "$CAP4_ID" "$NONCE4" "$TS4" "versepc2" "启动问题" "限额测试" "当日第二条。")
CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE" \
  -F "client=versepc2" -F "title=限额测试" -F "description=当日第二条。" -F "category=启动问题" \
  -F "captchaId=$CAP4_ID" -F "captchaAnswer=$CAP4_CODE" \
  -F "nonce=$NONCE4" -F "ts=$TS4" -F "sig=$SIG4")
echo "daily-limit http=$CODE (期望 429)"

echo "=== 8. 清理测试数据 ==="
node -e "const fs=require('fs'); fs.rmSync('.data/feedback/$FB_ID',{force:true}); fs.rmSync('.data/feedback-files/$FB_ID',{recursive:true,force:true});"
IP=x
# 找到当日限流 KEY 并删除
ls .data/feedback-rate/ 2>/dev/null | while read f; do rm -f ".data/feedback-rate/$f" && echo "cleaned rate key: $f"; done
echo "=== DONE ==="