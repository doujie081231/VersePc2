import { NextRequest, NextResponse } from "next/server";
import { feedbackSignKey } from "@/lib/sign";
import { CORS_ALLOW_ALL } from "@/lib/cors";

// 下发签名密钥：只给站点页面（同源）使用，密钥不进入静态构建产物，避免被从包体里“扒”出。
export async function GET(request: NextRequest) {
  const host = request.headers.get("host") || "";
  const referer = request.headers.get("referer") || "";
  const origin = request.headers.get("origin") || "";
  const sameHost =
    (origin && new URL(origin).hostname.indexOf("verselauncher.cn") >= 0) ||
    (referer && referer.indexOf("verselauncher.cn") >= 0) ||
    (host.indexOf("verselauncher.cn") >= 0);
  if (!sameHost) {
    return NextResponse.json({ error: "forbidden" }, { status: 403, headers: CORS_ALLOW_ALL });
  }
  const key = feedbackSignKey();
  if (!key) {
    return NextResponse.json({ error: "not configured" }, { status: 500, headers: CORS_ALLOW_ALL });
  }
  return NextResponse.json({ key }, { headers: CORS_ALLOW_ALL });
}