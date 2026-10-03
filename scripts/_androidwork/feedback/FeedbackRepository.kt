package com.verse.explorer.app.data.feedback

import android.content.Context
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * 反馈提交仓库（对齐 PC 端 feedback.js sendFeedbackToSite）：
 * 加载验证码 SVG、下发签名密钥、扫描日志附件、multipart 提交 + HMAC 防伪签名。
 */
object FeedbackRepository {

    private const val SITE = "https://verselauncher.cn"

    private val client = OkHttpClient.Builder()
        .connectTimeout(20, TimeUnit.SECONDS)
        .readTimeout(60, TimeUnit.SECONDS)
        .build()

    /** 站点验证码：GET /api/feedback/captcha → { id, svg } */
    suspend fun fetchCaptcha(): Pair<String, String> {
        val req = Request.Builder().url("$SITE/api/feedback/captcha").build()
        client.newCall(req).execute().use { resp ->
            val body = resp.body?.string() ?: throw IllegalStateException("HTTP ${resp.code}")
            return runCatching {
                val json = JSONObject(body)
                json.getString("id") to json.getString("svg")
            }.getOrElse { throw IllegalStateException(body) }
        }
    }

    /** 站点签名密钥：GET /api/feedback/signinfo → { key }（host=verselauncher.cn 放行） */
    suspend fun fetchSignKey(): String {
        val req = Request.Builder().url("$SITE/api/feedback/signinfo").build()
        client.newCall(req).execute().use { resp ->
            val body = resp.body?.string() ?: throw IllegalStateException("HTTP ${resp.code}")
            return runCatching { JSONObject(body).getString("key") }
                .getOrElse { throw IllegalStateException(body) }
        }
    }

    /**
     * 扫描启动器日志附件（按分类）：
     * launch  -> logs 下启动相关（launch-debug / launch-fail-* / startup-timing / latest 等）
     * download -> logs 下下载相关（modpack-import / updater-download / loader-install / *.install 等）
     * 目录无法直接列取时回退到已知路径。返回按修改时间倒序，最多 8 个。
     */
    fun collectLogs(context: Context, category: String): List<File> {
        val base = context.getExternalFilesDir(null)
        val logsDir = File(File(base, "VerseExplorerX"), "logs")
        val candidates = mutableListOf<File>()

        // 内部 filesDir 也扫一遍（部分日志写内存储）
        val internal = File(context.filesDir, "VerseExplorerX/logs")

        for (dir in listOf(logsDir, internal)) {
            if (!dir.isDirectory) continue
            dir.listFiles()?.forEach { f ->
                if (!f.isFile) return@forEach
                val name = f.name
                val keep = when (category) {
                    "launch" ->
                        name.startsWith("launch") || name.contains("startup-timing") ||
                            name.startsWith("latest") || name.startsWith("terminal_export") ||
                            name.startsWith("game") || name.startsWith("native_debug") ||
                            name.startsWith("verse_debug") || name.endsWith(".log")
                    "download" ->
                        name.contains("modpack-import") || name.contains("updater-download") ||
                            name.contains("loader-install") || name.endsWith(".install")
                    else -> false
                }
                if (keep && f.exists() && f.length() > 0 && f.length() <= 20 * 1024 * 1024) {
                    candidates.add(f)
                }
            }
        }

        // 每个目录留最近修改的，避免日志爆炸
        val grouped = candidates.groupBy { it.parentFile?.absolutePath }
        val picked = grouped.values.map { group ->
            group.sortedByDescending { it.lastModified() }.first()
        }
        return picked.sortedByDescending { it.lastModified() }.take(8)
    }

    /**
     * 提交反馈：multipart POST + 验证码 + HMAC 签名 + nonce 防重放。
     * 返回 null 表示成功；否则返回错误信息（含每日 429 提示）。
     */
    suspend fun submitFeedback(
        captchaId: String,
        captchaInput: String,
        category: String,
        categoryLabel: String,
        title: String,
        description: String,
        contact: String,
        signKey: String,
        env: String,
        logFiles: List<File>,
        mediaFiles: List<Pair<String, ByteArray>>
    ): String? {
        val nonce = FeedbackCrypto.randomNonce()
        val ts = System.currentTimeMillis().toString()
        val client = "versepc2"
        val desc = description + if (env.isNotBlank()) "\n\n【环境信息】$env" else ""

        val payload = listOf(
            "verse-feedback-v1", captchaId, nonce, ts, client, categoryLabel, title, desc
        )
        val sig = FeedbackCrypto.hmacSha256Hex(signKey, payload.joinToString("|"))

        val bodyBuilder = MultipartBody.Builder()
            .setType(MultipartBody.FORM)
            .addFormDataPart("client", client)
            .addFormDataPart("title", title)
            .addFormDataPart("description", desc)
            .addFormDataPart("category", categoryLabel)
            .addFormDataPart("email", contact)
            .addFormDataPart("captchaId", captchaId)
            .addFormDataPart("captchaAnswer", captchaInput)
            .addFormDataPart("nonce", nonce)
            .addFormDataPart("ts", ts)
            .addFormDataPart("sig", sig)

        var index = 0
        for (f in logFiles) {
            if (index >= 10) break
            bodyBuilder.addFormDataPart(
                "file$index", f.name,
                f.readBytes().toRequestBody("application/octet-stream".toMediaType())
            )
            index++
        }
        for ((name, bytes) in mediaFiles) {
            if (index >= 10) break
            bodyBuilder.addFormDataPart("file$index", name, bytes.toRequestBody("application/octet-stream".toMediaType()))
            index++
        }

        val req = Request.Builder()
            .url("$SITE/api/feedback")
            .post(bodyBuilder.build())
            .build()

        client.newCall(req).execute().use { resp ->
            val body = resp.body?.string() ?: ""
            when (resp.code) {
                in 200..299 -> return null
                429 -> return "今日已提交过反馈，请明日再试"
                403 -> {
                    return when {
                        body.contains("invalid signature") -> "提交校验失败，请刷新验证码后重试"
                        body.contains("replay denied") -> "请勿重复提交，请刷新验证码后重试"
                        else -> "提交被拒绝（$body），请稍后重试"
                    }
                }
                400 -> {
                    return when {
                        body.contains("captcha incorrect") -> "验证码不正确，请点击验证码刷新后重试"
                        else -> "提交参数有误（$body）"
                    }
                }
                else -> return "提交失败（HTTP ${resp.code}），请稍后重试"
            }
        }
    }
}