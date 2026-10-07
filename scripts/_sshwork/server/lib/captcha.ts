import crypto from "crypto";
import { getKV } from "./file-kv";

const CAPTCHA_TTL = 10 * 60 * 1000; // 10 分钟
const captchaKV = getKV("captcha");

const CHARSET = "23456789ABCDEFGHJKMNPQRSTUVWXYZ"; // 去掉易混淆字符

export interface Captcha {
  id: string;
  code: string;
  expiresAt: number;
}

export function generateCode(len = 4): string {
  const bytes = crypto.randomBytes(len);
  let code = "";
  for (let i = 0; i < len; i++) {
    code += CHARSET[bytes[i] % CHARSET.length];
  }
  return code;
}

export async function createCaptcha(): Promise<Captcha> {
  const id = crypto.randomBytes(16).toString("hex");
  const captcha: Captcha = { id, code: generateCode(), expiresAt: Date.now() + CAPTCHA_TTL };
  await captchaKV.put(id, JSON.stringify(captcha));
  return captcha;
}

export async function verifyCaptcha(id: string | undefined | null, answer: string | undefined | null): Promise<boolean> {
  if (!id || !answer) return false;
  const raw = await captchaKV.get(id);
  if (!raw) return false;
  try {
    const captcha = JSON.parse(raw) as Captcha;
    await captchaKV.delete(id); // 一次性，校验即删
    if (captcha.expiresAt < Date.now()) return false;
    return captcha.code.toUpperCase() === answer.trim().toUpperCase();
  } catch {
    return false;
  }
}

// 生成 SVG 验证码图（带简单旋转/颜色/干扰线）
export function captchaSvg(code: string): string {
  const colors = ["#dc2626", "#2563eb", "#059669", "#9333ea", "#d97706"];
  const chars = code.split("");
  const w = 132;
  const h = 44;
  let svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">`;
  // 背景
  svg += `<rect width="${w}" height="${h}" rx="8" fill="#f3f4f6"/>`;
  // 干扰线
  for (let i = 0; i < 4; i++) {
    const x1 = Math.floor(Math.random() * w);
    const y1 = Math.floor(Math.random() * h);
    const x2 = Math.floor(Math.random() * w);
    const y2 = Math.floor(Math.random() * h);
    svg += `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${colors[i % colors.length]}" stroke-width="1.2" opacity="0.5"/>`;
  }
  // 字符
  chars.forEach((c, i) => {
    const x = 22 + i * 26;
    const y = 32;
    const rot = Math.floor(Math.random() * 24) - 12;
    const color = colors[i % colors.length];
    svg += `<text x="${x}" y="${y}" font-size="26" font-family="Arial, sans-serif" font-weight="bold" fill="${color}" transform="rotate(${rot} ${x} ${y})" text-anchor="middle">${c}</text>`;
  });
  svg += `</svg>`;
  return svg;
}