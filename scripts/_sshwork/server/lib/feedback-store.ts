import fs from "fs";
import path from "path";
import crypto from "crypto";
import { getKV } from "./file-kv";

interface Feedback {
  id: string;
  title: string;
  client: string;
  description: string;
  email: string;
  phone: string;
  category?: string;
  createdAt: number;
  ip: string;
  files: string[];
}

const feedbackKV = getKV("feedback");
const rateKV = getKV("feedback-rate");

export function feedbackFilesDir(id: string): string {
  return path.join(process.cwd(), ".data", "feedback-files", id);
}

export async function listFeedback(): Promise<Feedback[]> {
  const { keys } = await feedbackKV.list("");
  const items = (
    await Promise.all(
      keys.map(async (k) => {
        const raw = await feedbackKV.get(k.name);
        if (!raw) return null;
        try {
          return JSON.parse(raw) as Feedback;
        } catch {
          return null;
        }
      })
    )
  )
    .filter((x): x is Feedback => x !== null)
    .sort((a, b) => b.createdAt - a.createdAt);
  return items;
}

export async function getFeedback(id: string): Promise<Feedback | null> {
  const raw = await feedbackKV.get(id);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as Feedback;
  } catch {
    return null;
  }
}

// id 用随机 18 位 hex：不可枚举，防止他人遍历 /files/<id>/ 拉取附件
export async function addFeedback(input: Omit<Feedback, "id" | "createdAt">): Promise<Feedback> {
  const id = crypto.randomBytes(9).toString("hex");
  const item: Feedback = { id, createdAt: Date.now(), ...input };
  await feedbackKV.put(id, JSON.stringify(item));
  return item;
}

// 附件保存后把文件名写回记录
export async function updateFeedbackFiles(id: string, files: string[]): Promise<void> {
  const raw = await feedbackKV.get(id);
  if (!raw) return;
  try {
    const item = JSON.parse(raw) as Feedback;
    item.files = files;
    await feedbackKV.put(id, JSON.stringify(item));
  } catch {}
}

export async function deleteFeedback(id: string): Promise<void> {
  await feedbackKV.delete(id);
  // 删除附件目录
  const dir = feedbackFilesDir(id);
  if (fs.existsSync(dir)) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

// 保存附件，返回文件名数组
export async function saveFeedbackFiles(id: string, files: File[]): Promise<string[]> {
  const dir = feedbackFilesDir(id);
  fs.mkdirSync(dir, { recursive: true });
  const names: string[] = [];
  for (const file of files) {
    const name = safeFileName(file.name);
    const filePath = path.join(dir, name);
    const buf = Buffer.from(await file.arrayBuffer());
    fs.writeFileSync(filePath, buf);
    names.push(name);
  }
  return names;
}

function safeFileName(name: string): string {
  return name.replace(/[\\/:*?"<>|]/g, "_").replace(/\s+/g, "_");
}

// IP 限流：一天一条（IP 需清洗，Windows 文件名不能含冒号）
function sanitizeIp(ip: string): string {
  return ip.replace(/:/g, "_").replace(/[^a-zA-Z0-9_.-]/g, "_");
}

export function todayKey(): string {
  const d = new Date();
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mm}-${dd}`;
}

export async function hasSubmittedToday(ip: string): Promise<boolean> {
  const raw = await rateKV.get(`${todayKey()}/${sanitizeIp(ip)}`);
  return raw !== null;
}

export async function markSubmittedToday(ip: string): Promise<void> {
  await rateKV.put(`${todayKey()}/${sanitizeIp(ip)}`, "1");
}