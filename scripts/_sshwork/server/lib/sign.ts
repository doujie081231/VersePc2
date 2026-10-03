// 反馈请求防伪签名：HMAC-SHA256 over 规范化字段 + 时间窗 + 一次性 nonce（防重放）
// 说明：密钥也存在于客户端（启动器/官网页面）以完成签名，属于“防伪造/防重放/防批量刷”的
// 增强壁垒，配合人机验证码与 IP 每日限流共同生效，并非对抗持有密钥的定向攻击。
import crypto from "crypto";
import { getKV } from "./file-kv";

const KEY_ENV = (process.env.FEEDBACK_SIGN_KEY || "").trim();
const signKey = Buffer.from(KEY_ENV, "hex");

export function feedbackSignKey(): string {
  return KEY_ENV;
}

export function signPayload(parts: string[]): string {
  return crypto.createHmac("sha256", signKey).update(parts.join("|")).digest("hex");
}

export interface SignFields {
  captchaId: string;
  nonce: string;
  ts: string;
  client: string;
  category: string;
  title: string;
  description: string;
  sig: string;
}

const TS_WINDOW_MS = 10 * 60 * 1000; // 签名时间窗 ±10 分钟

export function verifyFeedbackSignature(p: SignFields): boolean {
  if (!KEY_ENV) return false;
  const ts = Number(p.ts);
  if (!Number.isFinite(ts) || Number.isNaN(ts)) return false;
  const now = Date.now();
  if (Math.abs(now - ts) > TS_WINDOW_MS) return false;
  if (!p.nonce || !/^[A-Za-z0-9_-]{8,64}$/.test(p.nonce)) return false;
  if (!p.captchaId || !p.client) return false;
  const parts = ["verse-feedback-v1", p.captchaId, p.nonce, p.ts, p.client, p.category || "", p.title, p.description];
  const expected = signPayload(parts);
  let expectedBuf: Buffer, givenBuf: Buffer;
  try {
    expectedBuf = Buffer.from(expected, "hex");
    givenBuf = Buffer.from(String(p.sig || ""), "hex");
  } catch {
    return false;
  }
  if (expectedBuf.length !== givenBuf.length) return false;
  return crypto.timingSafeEqual(expectedBuf, givenBuf);
}

const nonceKV = getKV("feedback-nonce");
const NONCE_TTL_MS = 30 * 60 * 1000;

// 一次性 nonce：已存在则拒绝（防重放）；成功后写入带过期时间的记录
export async function consumeNonce(nonce: string): Promise<boolean> {
  const hit = await nonceKV.get(nonce);
  if (hit !== null) return false;
  await nonceKV.put(nonce, JSON.stringify({ ts: Date.now() + NONCE_TTL_MS }));
  return true;
}