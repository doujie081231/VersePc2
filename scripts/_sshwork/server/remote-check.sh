#!/bin/bash
set -x
cd /www/wwwroot/default || exit 1
echo '===== FILES ====='
md5sum app/api/feedback/route.ts lib/sign.ts lib/cors.ts lib/feedback-store.ts app/api/feedback/captcha/route.ts app/api/feedback/signinfo/route.ts app/feedback/page.tsx
echo '===== PKG SCRIPTS ====='
node -p "JSON.stringify(require('./package.json').scripts)"
echo '===== NGINX ROOT/DENY ====='
grep -nE 'root|deny|\.data' /www/server/panel/vhost/nginx/verselauncher.cn.conf | head -20
echo '===== DATA DIR ====='
ls -la .data 2>&1 | head -6
ls .data/feedback-files 2>&1 | head -6
echo '===== SIGN INFO FILE ====='
cat .env.local | sed -E 's/(=.*)/=<redacted>/' | grep -E 'FEEDBACK|SIGN'