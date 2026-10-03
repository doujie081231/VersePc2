package com.verse.explorer.app.data.feedback

import java.security.MessageDigest
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * 反馈请求防伪签名工具（对齐 PC 端 feedback.js / 站点 lib/sign.ts）：
 * HMAC-SHA256 over 规范化字段 + 随机一次性 nonce 防重放。
 */
object FeedbackCrypto {

    /** HMAC-SHA256，密钥为 hex 字符串 */
    fun hmacSha256Hex(keyHex: String, message: String): String {
        val keyBytes = hexToBytes(keyHex)
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(keyBytes, "HmacSHA256"))
        return bytesToHex(mac.doFinal(message.toByteArray(Charsets.UTF_8)))
    }

    /** 随机一次性 nonce（8-64 位字母数字） */
    fun randomNonce(len: Int = 32): String {
        val allowed = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-"
        return (1..len).map { allowed.random() }.joinToString("")
    }

    fun hexToBytes(hex: String): ByteArray {
        val clean = hex.replace(Regex("[^0-9a-fA-F]"), "")
        val out = ByteArray(clean.length / 2)
        for (i in out.indices) {
            out[i] = clean.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
        return out
    }

    fun bytesToHex(bytes: ByteArray): String {
        val sb = StringBuilder(bytes.size * 2)
        for (b in bytes) sb.append("%02x".format(b))
        return sb.toString()
    }
}