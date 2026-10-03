import { NextRequest, NextResponse } from "next/server";
import { verifyCaptcha } from "@/lib/captcha";
import { verifyFeedbackSignature, consumeNonce } from "@/lib/sign";
import { CORS_ALLOW_ALL } from "@/lib/cors";
import {
  addFeedback,
  hasSubmittedToday,
  markSubmittedToday,
  saveFeedbackFiles,
  updateFeedbackFiles,
} from "@/lib/feedback-store";

const MAX_FILE_SIZE = 20 * 1024 * 1024; // 20MB
const MAX_FILES = 10;
const MAX_CATEGORY_LEN = 32;

// 客户端可信 IP：优先取 nginx 覆写的 X-Real-IP（不可伪造），
// 其次取 X-Forwarded-For 最后一段（nginx 追加的真实客户端 IP，前面的可伪造，故不能取第一段）。
function getClientIp(request: NextRequest): string {
  const real = request.headers.get("x-real-ip");
  if (real) return real.trim();
  const fwd = request.headers.get("x-forwarded-for");
  if (fwd) {
    const parts = fwd.split(",").map((s) => s.trim()).filter(Boolean);
    const last = parts[parts.length - 1];
    if (last) return last;
  }
  return "unknown";
}

export async function OPTIONS() {
  return new NextResponse(null, { status: 204, headers: CORS_ALLOW_ALL });
}

export async function POST(request: NextRequest) {
  let form: FormData;
  try {
    form = await request.formData();
  } catch {
    return NextResponse.json({ error: "invalid form" }, { status: 400, headers: CORS_ALLOW_ALL });
  }

  // 0. 防伪签名：时间窗 + 一次性 nonce + HMAC 校验（启动器/官网联网端必带；手机端兼容旧协议）
  const client = ((form.get("client") as string | null) ?? "versepc2").trim();
  const ip = getClientIp(request);

  if (client === "versepc2") {
    const captchaId = (form.get("captchaId") as string | null) ?? "";
    const title = ((form.get("title") as string | null) ?? "").trim();
    const description = ((form.get("description") as string | null) ?? "").trim();
    const category = ((form.get("category") as string | null) ?? "").trim();
    const sigFields = {
      captchaId,
      nonce: (form.get("nonce") as string | null) ?? "",
      ts: (form.get("ts") as string | null) ?? "",
      client,
      category,
      title,
      description,
      sig: (form.get("sig") as string | null) ?? "",
    };
    if (!verifyFeedbackSignature(sigFields)) {
      return NextResponse.json({ error: "invalid signature" }, { status: 403, headers: CORS_ALLOW_ALL });
    }
    if (!(await consumeNonce(sigFields.nonce))) {
      return NextResponse.json({ error: "replay denied" }, { status: 403, headers: CORS_ALLOW_ALL });
    }
  }

  // 1. 校验验证码
  const captchaId = form.get("captchaId") as string | null;
  const captchaAnswer = form.get("captchaAnswer") as string | null;
  const captchaOk = await verifyCaptcha(captchaId, captchaAnswer);
  if (!captchaOk) {
    return NextResponse.json({ error: "captcha incorrect" }, { status: 400, headers: CORS_ALLOW_ALL });
  }

  // 2. IP 一天一条
  if (await hasSubmittedToday(ip)) {
    return NextResponse.json({ error: "daily limit" }, { status: 429, headers: CORS_ALLOW_ALL });
  }

  // 3. 字段
  const title = ((form.get("title") as string | null) ?? "").trim();
  const description = ((form.get("description") as string | null) ?? "").trim();
  const email = ((form.get("email") as string | null) ?? "").trim();
  const phone = ((form.get("phone") as string | null) ?? "").trim();
  const category = ((form.get("category") as string | null) ?? "").trim().slice(0, MAX_CATEGORY_LEN);

  if (!title || !description) {
    return NextResponse.json({ error: "title and description required" }, { status: 400, headers: CORS_ALLOW_ALL });
  }
  if (client !== "手机端" && client !== "versepc2") {
    return NextResponse.json({ error: "invalid client" }, { status: 400, headers: CORS_ALLOW_ALL });
  }

  // 4. 附件（File[]）
  const files: File[] = [];
  for (const [key, value] of form.entries()) {
    if (key.startsWith("file") && value instanceof File) {
      if (value.size > MAX_FILE_SIZE) {
        return NextResponse.json({ error: "file too large" }, { status: 400, headers: CORS_ALLOW_ALL });
      }
      files.push(value);
    }
  }
  if (files.length > MAX_FILES) {
    return NextResponse.json({ error: "too many files" }, { status: 400, headers: CORS_ALLOW_ALL });
  }

  // 5. 先写记录，再保存附件并写回文件名
  const item = await addFeedback({
    title,
    client,
    description,
    email,
    phone,
    category,
    ip,
    files: [],
  });

  if (files.length > 0) {
    const savedNames = await saveFeedbackFiles(item.id, files);
    await updateFeedbackFiles(item.id, savedNames);
    item.files = savedNames;
  }

  // 6. 记录限流（成功提交后）
  await markSubmittedToday(ip);

  return NextResponse.json({ ok: true, id: item.id }, { headers: CORS_ALLOW_ALL });
}