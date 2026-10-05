"use client";
import { useEffect, useRef, useState } from "react";
import { RefreshCw, Send } from "lucide-react";
import { Navbar } from "@/components/ui/navbar";

// 反馈请求防伪签名（与后端 lib/sign.ts 的规范化规则保持一致）
function hexToBytes(hex: string): Uint8Array {
  const clean = hex.replace(/[^0-9a-fA-F]/g, "");
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(clean.substr(i * 2, 2), 16);
  }
  return bytes;
}
function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join("");
}
async function hmacSha256Hex(keyHex: string, message: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    hexToBytes(keyHex),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const sig = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(message));
  return bytesToHex(new Uint8Array(sig));
}
function randomNonce(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes).map((b) => b.toString(36)).join("").slice(0, 32);
}

export default function FeedbackPage() {
  const [title, setTitle] = useState("");
  const [client, setClient] = useState("versepc2");
  const [description, setDescription] = useState("");
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [logs, setLogs] = useState<FileList | null>(null);
  const [images, setImages] = useState<FileList | null>(null);
  const [videos, setVideos] = useState<FileList | null>(null);
  const [captchaId, setCaptchaId] = useState("");
  const [captchaSvg, setCaptchaSvg] = useState("");
  const [captchaAnswer, setCaptchaAnswer] = useState("");
  const [signKey, setSignKey] = useState("");
  const [msg, setMsg] = useState<{ type: "ok" | "err"; text: string } | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function loadCaptcha() {
    try {
      const res = await fetch("/api/feedback/captcha");
      const data = await res.json();
      setCaptchaId(data.id);
      setCaptchaSvg(data.svg);
    } catch {}
    // 签名密钥仅同源下发，不写入静态包
    if (!signKey) {
      try {
        const resKey = await fetch("/api/feedback/signinfo");
        const dataKey = await resKey.json();
        if (dataKey.key) setSignKey(dataKey.key);
      } catch {}
    }
  }

  useEffect(() => {
    loadCaptcha();
  }, []);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (submitting) return;
    setMsg(null);

    // 服务器用 trim 后的 title/description 校验签名，签名与提交必须使用同一份 trim 后内容
    const submitTitle = title.trim();
    const submitDescription = description.trim();

    const formData = new FormData();
    formData.append("title", submitTitle);
    formData.append("client", client);
    formData.append("description", submitDescription);
    formData.append("email", email);
    formData.append("phone", phone);
    formData.append("captchaId", captchaId);
    formData.append("captchaAnswer", captchaAnswer);
    formData.append("category", "");
    for (const f of logs ?? []) formData.append("file-log", f);
    for (const f of images ?? []) formData.append("file-image", f);
    for (const f of videos ?? []) formData.append("file-video", f);

    // versepc2 客户端必须带防伪签名
    if (client === "versepc2") {
      if (!signKey) {
        setMsg({ type: "err", text: "签名信息获取失败，请刷新页面重试" });
        return;
      }
      const nonce = randomNonce();
      const ts = String(Date.now());
      const parts = ["verse-feedback-v1", captchaId, nonce, ts, client, "", submitTitle, submitDescription];
      const sig = await hmacSha256Hex(signKey, parts.join("|"));
      formData.append("nonce", nonce);
      formData.append("ts", ts);
      formData.append("sig", sig);
    }

    setSubmitting(true);
    try {
      const res = await fetch("/api/feedback", { method: "POST", body: formData });
      if (res.ok) {
        setMsg({ type: "ok", text: "提交成功，感谢你的反馈！" });
        setTitle(""); setDescription(""); setEmail(""); setPhone("");
        setLogs(null); setImages(null); setVideos(null); setCaptchaAnswer("");
        loadCaptcha();
      } else if (res.status === 429) {
        setMsg({ type: "err", text: "今日已提交过反馈，请明日再试" });
      } else {
        const data = await res.json().catch(() => ({}));
        setMsg({
          type: "err",
          text:
            (data as any).error === "captcha incorrect"
              ? "验证码错误，请重试"
              : (data as any).error === "invalid signature"
                ? "提交签名校验失败，请刷新后重试"
                : "提交失败，请稍后再试",
        });
        loadCaptcha();
        setCaptchaAnswer("");
      }
    } catch {
      setMsg({ type: "err", text: "网络错误，请稍后再试" });
    } finally {
      setSubmitting(false);
    }
  }

  const inputCls =
    "h-10 rounded-lg border border-gray-300 px-3 text-sm outline-none focus:border-gray-900 w-full";
  const textareaCls =
    "rounded-lg border border-gray-300 px-3 py-2 text-sm outline-none focus:border-gray-900 w-full resize-none";
  const fileCls =
    "block w-full text-sm text-gray-500 file:mr-3 file:rounded-lg file:border-0 file:bg-gray-900 file:px-3 file:py-2 file:text-sm file:font-semibold file:text-white hover:file:bg-gray-800";

  return (
    <main className="min-h-screen" style={{ background: "#ffffff" }}>
      <Navbar />
      <div className="pt-28 pb-24 px-4">
        <div className="max-w-2xl mx-auto">
          <h1 className="text-4xl font-bold text-center leading-tight mb-4" style={{ color: "#111827" }}>
            问题反馈
          </h1>
          <p className="text-center text-base mb-12" style={{ color: "#6b7280" }}>
            遇到问题或有建议？填写表单告诉我们
          </p>

          {msg && (
            <div
              className={`mb-6 rounded-xl px-4 py-3 text-sm ${
                msg.type === "ok" ? "bg-green-50 text-green-700" : "bg-red-50 text-red-700"
              }`}
            >
              {msg.text}
            </div>
          )}

          <form onSubmit={handleSubmit} className="flex flex-col gap-5 rounded-2xl border border-black/10 bg-white p-8 shadow-sm">
            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                标题 *
              </label>
              <input type="text" required value={title} onChange={(e) => setTitle(e.target.value)} placeholder="简要描述问题" className={inputCls} style={{ color: "#111827" }} />
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                版本端 *
              </label>
              <div className="flex gap-3">
                {["手机端", "versepc2"].map((c) => (
                  <button
                    key={c}
                    type="button"
                    onClick={() => setClient(c)}
                    className={`h-10 flex-1 rounded-lg border text-sm font-medium transition-colors ${
                      client === c ? "border-gray-900 bg-gray-900 text-white" : "border-gray-300 bg-white text-gray-700 hover:bg-gray-50"
                    }`}
                  >
                    {c}
                  </button>
                ))}
              </div>
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                详细描述 *
              </label>
              <textarea required value={description} onChange={(e) => setDescription(e.target.value)} rows={5} placeholder="请详细描述问题出现的步骤、现象等" className={textareaCls} style={{ color: "#111827" }} />
            </div>

            <div className="grid sm:grid-cols-2 gap-4">
              <div>
                <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                  联系邮箱
                </label>
                <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="用于回复" className={inputCls} style={{ color: "#111827" }} />
              </div>
              <div>
                <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                  手机号
                </label>
                <input type="tel" value={phone} onChange={(e) => setPhone(e.target.value)} placeholder="选填" className={inputCls} style={{ color: "#111827" }} />
              </div>
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                详细日志
              </label>
              <input type="file" multiple accept=".txt,.log,.zip,.json,.gz" onChange={(e) => setLogs(e.target.files)} className={fileCls} />
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                图片
              </label>
              <input type="file" multiple accept="image/*" onChange={(e) => setImages(e.target.files)} className={fileCls} />
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                视频
              </label>
              <input type="file" multiple accept="video/*" onChange={(e) => setVideos(e.target.files)} className={fileCls} />
            </div>

            <div>
              <label className="block text-sm font-medium mb-2" style={{ color: "#374151" }}>
                人机验证 *
              </label>
              <div className="flex items-center gap-3">
                {captchaSvg && (
                  <span
                    className="shrink-0 rounded-lg border border-gray-200 overflow-hidden"
                    dangerouslySetInnerHTML={{ __html: captchaSvg }}
                  />
                )}
                <input
                  type="text"
                  required
                  value={captchaAnswer}
                  onChange={(e) => setCaptchaAnswer(e.target.value)}
                  placeholder="输入验证码"
                  className={inputCls}
                  style={{ color: "#111827" }}
                  autoComplete="off"
                />
                <button
                  type="button"
                  onClick={loadCaptcha}
                  className="shrink-0 inline-flex items-center gap-1 h-10 rounded-lg border border-gray-300 px-3 text-sm transition-colors hover:bg-gray-50"
                  style={{ color: "#374151" }}
                >
                  <RefreshCw className="size-4" />
                  刷新
                </button>
              </div>
            </div>

            <button
              type="submit"
              disabled={submitting}
              className="inline-flex items-center justify-center gap-1.5 h-11 rounded-lg text-sm font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50"
              style={{ background: "#111827" }}
            >
              <Send className="size-4" />
              {submitting ? "提交中…" : "提交"}
            </button>
          </form>
        </div>
      </div>
    </main>
  );
}