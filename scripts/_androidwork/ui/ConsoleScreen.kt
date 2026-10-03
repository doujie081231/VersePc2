package com.verse.explorer.app.ui

import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import android.util.Base64
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.navigation.NavHostController
import coil.ImageLoader
import coil.compose.AsyncImage
import coil.decode.SvgDecoder
import coil.request.ImageRequest
import com.verse.explorer.app.data.SettingsRepository
import com.verse.explorer.app.data.feedback.FeedbackRepository
import com.verse.explorer.app.ui.components.PageHeader
import com.verse.explorer.app.ui.components.PcButton
import com.verse.explorer.app.ui.components.PcButtonVariant
import com.verse.explorer.app.ui.components.PcCard
import com.verse.explorer.app.ui.components.PcSwitchRow
import com.verse.explorer.app.ui.theme.VerseColors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.text.DecimalFormat

/** 站点地址（对齐 versepc2 feedback.js FEEDBACK_SITE_URL） */
private const val FEEDBACK_SITE_URL = "https://verselauncher.cn"

/** 附件限制（对齐 versepc2 feedback.js） */
private const val FEEDBACK_MAX_FILES = 10
private const val FEEDBACK_MAX_FILE_BYTES = 20 * 1024 * 1024

private data class FeedbackTypeInfo(
    val id: String,
    val title: String,
    val desc: String,
    val icon: ImageVector
)

private data class FeedbackCategoryInfo(
    val id: String,
    val title: String,
    val desc: String,
    val icon: ImageVector
)

private data class FeedbackAttachment(
    val uri: Uri,
    val name: String,
    val size: Long
)

/**
 * 问题反馈页（对齐 versepc2 page-feedback.js / feedback.js）：
 * 问题分类（启动/下载/其他）+ 反馈内容 + 日志附件（自动收集）+ 截图/视频（手动）
 * + 环境信息 + 人机验证（站点 SVG 验证码）+ HMAC 签名提交通道。
 */
@Composable
fun ConsoleScreen(navController: NavHostController) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    val feedbackTypes = listOf(
        FeedbackTypeInfo("bug", "问题反馈", "崩溃、报错、功能异常", Icons.Filled.BugReport),
        FeedbackTypeInfo("feature", "功能建议", "希望增加或改进的功能", Icons.Filled.Lightbulb),
        FeedbackTypeInfo("other", "其他", "使用体验、咨询等", Icons.Filled.Chat)
    )
    val feedbackCategories = listOf(
        FeedbackCategoryInfo("launch", "启动问题", "启动失败、崩溃、闪退", Icons.Filled.PlayArrow),
        FeedbackCategoryInfo("download", "下载问题", "资源下载慢、卡住、校验失败", Icons.Filled.Download),
        FeedbackCategoryInfo("other", "其他", "功能建议、体验优化等", Icons.Filled.Info)
    )

    var selectedType by remember { mutableStateOf("bug") }
    var selectedCategory by remember { mutableStateOf("other") }
    var title by remember { mutableStateOf("") }
    var detail by remember { mutableStateOf("") }
    var contact by remember { mutableStateOf("") }
    var mediaFiles by remember { mutableStateOf<List<FeedbackAttachment>>(emptyList()) }
    var includeEnv by remember { mutableStateOf(true) }
    var submitting by remember { mutableStateOf(false) }

    // 验证码
    var captchaId by remember { mutableStateOf("") }
    var captchaSvg by remember { mutableStateOf("") }
    var captchaInput by remember { mutableStateOf("") }
    var signKey by remember { mutableStateOf("") }
    val svgImageLoader = remember {
        ImageLoader.Builder(context)
            .components { add(SvgDecoder.Factory()) }
            .build()
    }

    // 自动收集的日志附件
    var autoLogFiles by remember { mutableStateOf<List<File>>(emptyList()) }
    var logsLoading by remember { mutableStateOf(false) }

    // 刷新验证码 + 签名密钥
    fun refreshCaptcha() {
        scope.launch(Dispatchers.IO) {
            runCatching {
                val (id, svg) = FeedbackRepository.fetchCaptcha()
                withContext(Dispatchers.Main) {
                    captchaId = id
                    captchaSvg = svg
                    captchaInput = ""
                }
            }.onFailure { e ->
                withContext(Dispatchers.Main) {
                    Toast.makeText(context, "验证码加载失败：${e.message}", Toast.LENGTH_SHORT).show()
                }
            }
            // 签名密钥只取一次
            if (signKey.isBlank()) {
                runCatching {
                    val key = FeedbackRepository.fetchSignKey()
                    withContext(Dispatchers.Main) { signKey = key }
                }.onFailure { }
            }
        }
    }

    // 分类变化 → 重新扫描日志
    fun scanLogs() {
        if (selectedCategory == "other") {
            autoLogFiles = emptyList()
            return
        }
        logsLoading = true
        autoLogFiles = emptyList()
        scope.launch(Dispatchers.IO) {
            val files = FeedbackRepository.collectLogs(context.applicationContext, selectedCategory)
            withContext(Dispatchers.Main) {
                autoLogFiles = files
                logsLoading = false
            }
        }
    }

    LaunchedEffect(Unit) {
        refreshCaptcha()
        scanLogs()
    }

    // 验证码 SVG → data URI（coil-svg 渲染）
    val captchaDataUri = remember(captchaSvg) {
        if (captchaSvg.isBlank()) null
        else "data:image/svg+xml;base64," + Base64.encodeToString(captchaSvg.toByteArray(), Base64.NO_WRAP)
    }

    // 选择截图/视频
    val pickMedia = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenMultipleDocuments()
    ) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        val current = mediaFiles.toMutableList()
        val rejected = mutableListOf<String>()
        uris.forEach { uri ->
            if (current.size >= FEEDBACK_MAX_FILES) {
                rejected.add("${displayName(context, uri)}（已达数量上限）")
                return@forEach
            }
            val name = displayName(context, uri)
            val size = querySize(context, uri)
            if (size > FEEDBACK_MAX_FILE_BYTES) {
                rejected.add("$name（超过 20 MB）")
                return@forEach
            }
            if (current.any { it.uri == uri }) return@forEach
            current.add(FeedbackAttachment(uri, name, size))
        }
        mediaFiles = current
        if (rejected.isNotEmpty()) {
            Toast.makeText(context, "已跳过：${rejected.joinToString("、")}", Toast.LENGTH_SHORT).show()
        }
    }

    // 环境信息（对齐 versepc2 loadFeedbackEnv）
    val envRows = remember {
        val versionName = runCatching {
            context.packageManager.getPackageInfo(context.packageName, 0).versionName
        }.getOrNull() ?: "未知"
        listOf(
            "启动器版本" to "VerseExplorer $versionName",
            "操作系统" to "Android ${android.os.Build.VERSION.RELEASE} · ${android.os.Build.MODEL}",
            "Java" to "Java ${SettingsRepository.getJavaVersion()}"
        )
    }

    var submitError by remember { mutableStateOf<String?>(null) }

    fun validate(): String? {
        if (title.trim().length < 4) return "标题太短，请至少填写 4 个字"
        if (detail.trim().length < 10) return "描述太短，请至少填写 10 个字"
        if (captchaInput.trim().isBlank()) return "请完成人机验证"
        return null
    }

    fun resetForm() {
        title = ""
        detail = ""
        contact = ""
        mediaFiles = emptyList()
        captchaInput = ""
        submitError = null
        refreshCaptcha()
        scanLogs()
    }

    fun submit() {
        if (submitting) return
        val error = validate()
        if (error != null) {
            submitError = error
            return
        }
        if (captchaId.isBlank() || signKey.isBlank()) {
            submitError = "验证码与签名信息尚未就绪，请稍后重试"
            refreshCaptcha()
            return
        }
        val category = selectedCategory
        val categoryLabel = when (category) {
            "launch" -> "启动问题"
            "download" -> "下载问题"
            else -> "其他"
        }
        val envText = if (includeEnv) envRows.joinToString("；") { (k, v) -> "$k：$v" } else ""

        submitting = true
        submitError = null
        scope.launch(Dispatchers.IO) {
            // 手动附件转字节
            val media = mediaFiles.mapNotNull { att ->
                runCatching {
                    val bytes = context.contentResolver.openInputStream(att.uri)?.use { it.readBytes() } ?: return@mapNotNull null
                    att.name to bytes
                }.getOrNull()
            }
            val result = runCatching {
                FeedbackRepository.submitFeedback(
                    captchaId = captchaId,
                    captchaInput = captchaInput.trim(),
                    category = category,
                    categoryLabel = categoryLabel,
                    title = title.trim(),
                    description = detail.trim(),
                    contact = contact.trim(),
                    signKey = signKey,
                    env = envText,
                    logFiles = autoLogFiles,
                    mediaFiles = media
                )
            }.getOrElse { e -> "提交失败：${e.message}" }

            withContext(Dispatchers.Main) {
                submitting = false
                if (result == null) {
                    Toast.makeText(context, "反馈已提交，感谢你的反馈！", Toast.LENGTH_LONG).show()
                    resetForm()
                } else {
                    submitError = result
                    if (result.contains("验证码")) {
                        refreshCaptcha()
                    }
                }
            }
        }
    }

    fun openFeedbackSite() {
        runCatching {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(FEEDBACK_SITE_URL)))
        }.onFailure {
            Toast.makeText(context, "无法打开浏览器", Toast.LENGTH_SHORT).show()
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 20.dp)
            .padding(top = 20.dp, bottom = 16.dp)
    ) {
        PageHeader(
            title = "问题反馈",
            actions = {
                PcButton(
                    text = "访问官网",
                    onClick = { openFeedbackSite() },
                    variant = PcButtonVariant.SECONDARY,
                    icon = Icons.Filled.OpenInNew
                )
            }
        )

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            // 反馈类型
            item {
                PcCard(title = "反馈类型") {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        feedbackTypes.forEach { typeInfo ->
                            val active = selectedType == typeInfo.id
                            Surface(
                                shape = RoundedCornerShape(10.dp),
                                color = if (active) VerseColors.PcAccent.copy(alpha = 0.1f) else VerseColors.PcBgTertiary,
                                border = BorderStroke(
                                    1.dp,
                                    if (active) VerseColors.PcAccent else VerseColors.PcBorder
                                ),
                                modifier = Modifier
                                    .weight(1f)
                                    .clickable { selectedType = typeInfo.id }
                            ) {
                                Column(
                                    modifier = Modifier.padding(horizontal = 8.dp, vertical = 10.dp),
                                    horizontalAlignment = Alignment.CenterHorizontally
                                ) {
                                    Icon(
                                        typeInfo.icon,
                                        contentDescription = null,
                                        tint = if (active) VerseColors.PcAccent else VerseColors.PcTextSecondary,
                                        modifier = Modifier.size(20.dp)
                                    )
                                    Spacer(Modifier.height(6.dp))
                                    Text(
                                        typeInfo.title,
                                        fontSize = 12.sp,
                                        fontWeight = FontWeight.SemiBold,
                                        color = if (active) VerseColors.PcAccent else VerseColors.PcTextPrimary
                                    )
                                    Spacer(Modifier.height(2.dp))
                                    Text(
                                        typeInfo.desc,
                                        fontSize = 10.sp,
                                        color = VerseColors.PcTextMuted,
                                        maxLines = 2
                                    )
                                }
                            }
                        }
                    }
                }
            }

            // 问题分类（决定自动附加日志）
            item {
                PcCard(title = "问题分类") {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp)
                        ) {
                            feedbackCategories.forEach { cat ->
                                val active = selectedCategory == cat.id
                                Surface(
                                    shape = RoundedCornerShape(10.dp),
                                    color = if (active) VerseColors.PcAccent.copy(alpha = 0.1f) else VerseColors.PcBgTertiary,
                                    border = BorderStroke(
                                        1.dp,
                                        if (active) VerseColors.PcAccent else VerseColors.PcBorder
                                    ),
                                    modifier = Modifier
                                        .weight(1f)
                                        .clickable {
                                            selectedCategory = cat.id
                                            scanLogs()
                                        }
                                ) {
                                    Column(
                                        modifier = Modifier.padding(horizontal = 8.dp, vertical = 10.dp),
                                        horizontalAlignment = Alignment.CenterHorizontally
                                    ) {
                                        Icon(
                                            cat.icon,
                                            contentDescription = null,
                                            tint = if (active) VerseColors.PcAccent else VerseColors.PcTextSecondary,
                                            modifier = Modifier.size(20.dp)
                                        )
                                        Spacer(Modifier.height(6.dp))
                                        Text(
                                            cat.title,
                                            fontSize = 12.sp,
                                            fontWeight = FontWeight.SemiBold,
                                            color = if (active) VerseColors.PcAccent else VerseColors.PcTextPrimary
                                        )
                                        Spacer(Modifier.height(2.dp))
                                        Text(
                                            cat.desc,
                                            fontSize = 10.sp,
                                            color = VerseColors.PcTextMuted,
                                            maxLines = 2,
                                            textAlign = androidx.compose.ui.text.style.TextAlign.Center
                                        )
                                    }
                                }
                            }
                        }
                        Text(
                            "选择分类后将自动附带对应日志，请先复现问题再上传",
                            fontSize = 11.sp,
                            color = VerseColors.PcTextMuted
                        )
                    }
                }
            }

            // 反馈内容
            item {
                PcCard(title = "反馈内容") {
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(10.dp)
                    ) {
                        Text(
                            "标题 *",
                            fontSize = 13.sp,
                            fontWeight = FontWeight.Medium,
                            color = VerseColors.PcTextSecondary
                        )
                        OutlinedTextField(
                            value = title,
                            onValueChange = { if (it.length <= 50) title = it },
                            modifier = Modifier.fillMaxWidth(),
                            placeholder = { Text("一句话概括你遇到的问题", fontSize = 13.sp, color = VerseColors.PcTextMuted) },
                            singleLine = true,
                            shape = RoundedCornerShape(8.dp),
                            colors = outTextFieldColors()
                        )
                        Text(
                            "详细描述 *",
                            fontSize = 13.sp,
                            fontWeight = FontWeight.Medium,
                            color = VerseColors.PcTextSecondary
                        )
                        OutlinedTextField(
                            value = detail,
                            onValueChange = { if (it.length <= 2000) detail = it },
                            modifier = Modifier.fillMaxWidth().height(120.dp),
                            placeholder = {
                                Text(
                                    "请描述复现步骤、期望结果与实际结果，越具体越容易定位",
                                    fontSize = 13.sp,
                                    color = VerseColors.PcTextMuted
                                )
                            },
                            shape = RoundedCornerShape(8.dp),
                            colors = outTextFieldColors()
                        )
                        Text(
                            "${detail.length} / 2000",
                            fontSize = 11.sp,
                            color = VerseColors.PcTextMuted,
                            modifier = Modifier.align(Alignment.End)
                        )
                        Text(
                            "联系方式（选填）",
                            fontSize = 13.sp,
                            fontWeight = FontWeight.Medium,
                            color = VerseColors.PcTextSecondary
                        )
                        OutlinedTextField(
                            value = contact,
                            onValueChange = { if (it.length <= 80) contact = it },
                            modifier = Modifier.fillMaxWidth(),
                            placeholder = { Text("邮箱 / QQ，便于我们回复你", fontSize = 13.sp, color = VerseColors.PcTextMuted) },
                            singleLine = true,
                            shape = RoundedCornerShape(8.dp),
                            colors = outTextFieldColors()
                        )
                        Text(
                            "不填也能提交，但可能无法收到处理结果",
                            fontSize = 11.sp,
                            color = VerseColors.PcTextMuted
                        )
                    }
                }
            }

            // 日志附件（自动收集）
            item {
                PcCard(title = "日志附件") {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (selectedCategory == "other") {
                            Text(
                                "「其他」分类无需收集日志，可直接补充截图或描述",
                                fontSize = 12.sp,
                                color = VerseColors.PcTextMuted
                            )
                        } else if (logsLoading) {
                            Text(
                                "正在扫描日志...",
                                fontSize = 12.sp,
                                color = VerseColors.PcAccent
                            )
                        } else if (autoLogFiles.isEmpty()) {
                            Text(
                                if (selectedCategory == "launch") "未找到启动日志（可能尚未复现过启动问题）"
                                else "未找到下载日志（可能尚未复现过下载问题）",
                                fontSize = 12.sp,
                                color = VerseColors.PcTextMuted
                            )
                        } else {
                            autoLogFiles.take(8).forEach { f ->
                                Row(
                                    modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp))
                                        .background(VerseColors.PcBgTertiary).padding(horizontal = 12.dp, vertical = 8.dp),
                                    verticalAlignment = Alignment.CenterVertically
                                ) {
                                    Icon(
                                        Icons.Filled.Description,
                                        contentDescription = null,
                                        tint = VerseColors.PcAccent,
                                        modifier = Modifier.size(16.dp)
                                    )
                                    Spacer(Modifier.width(8.dp))
                                    Text(
                                        f.name,
                                        fontSize = 12.sp,
                                        color = VerseColors.PcTextPrimary,
                                        maxLines = 1,
                                        modifier = Modifier.weight(1f, fill = false)
                                    )
                                }
                            }
                            Text(
                                "已自动收集 ${autoLogFiles.size} 个日志文件（随反馈一并提交）",
                                fontSize = 11.sp,
                                color = VerseColors.PcTextMuted
                            )
                        }
                        Text(
                            "提交前请一定先复现问题，确保日志已记录到本次异常，再上传",
                            fontSize = 11.sp,
                            color = VerseColors.PcRed
                        )
                    }
                }
            }

            // 截图或视频（手动选择）
            item {
                PcCard(title = "截图或视频") {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Surface(
                            shape = RoundedCornerShape(10.dp),
                            color = VerseColors.PcBgTertiary,
                            border = BorderStroke(1.dp, VerseColors.PcBorder),
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { pickMedia.launch(arrayOf("image/*", "video/*")) }
                        ) {
                            Column(
                                modifier = Modifier.padding(vertical = 22.dp),
                                horizontalAlignment = Alignment.CenterHorizontally
                            ) {
                                Icon(
                                    Icons.Filled.Image,
                                    contentDescription = null,
                                    tint = VerseColors.PcTextMuted,
                                    modifier = Modifier.size(26.dp)
                                )
                                Spacer(Modifier.height(8.dp))
                                Text(
                                    "点击选择截图或视频",
                                    fontSize = 13.sp,
                                    fontWeight = FontWeight.Medium,
                                    color = VerseColors.PcTextSecondary
                                )
                                Spacer(Modifier.height(2.dp))
                                Text(
                                    "支持图片、视频，最多 $FEEDBACK_MAX_FILES 个，单个不超过 20 MB",
                                    fontSize = 11.sp,
                                    color = VerseColors.PcTextMuted
                                )
                            }
                        }
                        mediaFiles.forEachIndexed { index, file ->
                            Row(
                                modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp))
                                    .background(VerseColors.PcBgTertiary).padding(horizontal = 12.dp, vertical = 8.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Icon(
                                    Icons.Filled.Image,
                                    contentDescription = null,
                                    tint = VerseColors.PcTextSecondary,
                                    modifier = Modifier.size(16.dp)
                                )
                                Spacer(Modifier.width(8.dp))
                                Text(
                                    file.name,
                                    fontSize = 12.sp,
                                    color = VerseColors.PcTextPrimary,
                                    maxLines = 1,
                                    modifier = Modifier.weight(1f, fill = false)
                                )
                                Spacer(Modifier.width(8.dp))
                                Text(
                                    formatSize(file.size),
                                    fontSize = 11.sp,
                                    color = VerseColors.PcTextMuted
                                )
                                Spacer(Modifier.width(4.dp))
                                Icon(
                                    Icons.Filled.Close,
                                    contentDescription = "移除",
                                    tint = VerseColors.PcTextMuted,
                                    modifier = Modifier.size(16.dp).clip(RoundedCornerShape(4.dp)).clickable {
                                        mediaFiles = mediaFiles.filterIndexed { i, _ -> i != index }
                                    }
                                )
                            }
                        }
                    }
                }
            }

            // 环境信息
            item {
                PcCard(title = "环境信息") {
                    PcSwitchRow(
                        label = "随反馈一并提交（推荐）",
                        description = "包含启动器版本、系统与 Java 信息，有助于快速定位问题",
                        checked = includeEnv,
                        onToggle = { includeEnv = it }
                    )
                    Spacer(Modifier.height(8.dp))
                    envRows.forEach { (key, value) ->
                        Row(
                            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            verticalAlignment = Alignment.CenterVertically
                        ) {
                            Text(
                                key,
                                fontSize = 12.sp,
                                color = VerseColors.PcTextSecondary,
                                modifier = Modifier.width(88.dp)
                            )
                            Text(
                                value,
                                fontSize = 12.sp,
                                color = VerseColors.PcTextPrimary,
                                maxLines = 1
                            )
                        }
                    }
                }
            }

            // 人机验证（站点 SVG 验证码）
            item {
                PcCard(title = "人机验证") {
                    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(10.dp)
                        ) {
                            if (captchaDataUri != null) {
                                AsyncImage(
                                    model = ImageRequest.Builder(context)
                                        .data(captchaDataUri)
                                        .build(),
                                    imageLoader = svgImageLoader,
                                    contentDescription = "验证码",
                                    modifier = Modifier
                                        .size(width = 130.dp, height = 44.dp)
                                        .clip(RoundedCornerShape(6.dp))
                                        .background(VerseColors.PcBgTertiary)
                                        .clickable { refreshCaptcha() }
                                )
                            } else {
                                Box(
                                    modifier = Modifier
                                        .size(width = 130.dp, height = 44.dp)
                                        .clip(RoundedCornerShape(6.dp))
                                        .background(VerseColors.PcBgTertiary),
                                    contentAlignment = Alignment.Center
                                ) {
                                    Text("加载中...", fontSize = 10.sp, color = VerseColors.PcTextMuted)
                                }
                            }
                            OutlinedTextField(
                                value = captchaInput,
                                onValueChange = { if (it.length <= 4) captchaInput = it },
                                modifier = Modifier.weight(1f),
                                placeholder = { Text("输入 4 位验证码", fontSize = 13.sp, color = VerseColors.PcTextMuted) },
                                singleLine = true,
                                shape = RoundedCornerShape(8.dp),
                                colors = outTextFieldColors()
                            )
                            PcButton(
                                text = "刷新",
                                onClick = { refreshCaptcha() },
                                variant = PcButtonVariant.OUTLINE
                            )
                        }
                        Text(
                            "看不清？点击验证码图片换一张",
                            fontSize = 11.sp,
                            color = VerseColors.PcTextMuted
                        )
                    }
                }
            }

            // 提交栏
            item {
                PcCard {
                    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        if (submitError != null) {
                            Text(
                                submitError.orEmpty(),
                                fontSize = 11.sp,
                                color = VerseColors.PcRed
                            )
                        }
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                "请务必先复现问题再上传",
                                fontSize = 11.sp,
                                color = VerseColors.PcTextMuted,
                                modifier = Modifier.weight(1f)
                            )
                            PcButton(
                                text = "重置",
                                onClick = { resetForm() },
                                variant = PcButtonVariant.OUTLINE,
                                modifier = Modifier.padding(end = 10.dp)
                            )
                            PcButton(
                                text = if (submitting) "提交中..." else "提交反馈",
                                onClick = { submit() },
                                variant = PcButtonVariant.PRIMARY,
                                enabled = validate() == null && !submitting
                            )
                        }
                    }
                }
            }

            item { Spacer(Modifier.height(20.dp)) }
        }
    }
}

@Composable
private fun outTextFieldColors() = OutlinedTextFieldDefaults.colors(
    focusedBorderColor = VerseColors.PcAccent,
    unfocusedBorderColor = VerseColors.PcBorder,
    focusedContainerColor = VerseColors.PcBgTertiary,
    unfocusedContainerColor = VerseColors.PcBgTertiary,
    focusedTextColor = VerseColors.PcTextPrimary,
    unfocusedTextColor = VerseColors.PcTextPrimary,
    cursorColor = VerseColors.PcAccent,
    focusedPlaceholderColor = VerseColors.PcTextMuted,
    unfocusedPlaceholderColor = VerseColors.PcTextMuted
)

private fun formatSize(bytes: Long): String {
    if (bytes <= 0) return ""
    return if (bytes < 1024) "$bytes B"
    else if (bytes < 1024 * 1024) "${DecimalFormat("0.0").format(bytes / 1024.0)} KB"
    else "${DecimalFormat("0.0").format(bytes / 1024.0 / 1024.0)} MB"
}

private fun displayName(context: android.content.Context, uri: Uri): String {
    return runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) ?: "未知文件" else "未知文件"
        } ?: "未知文件"
    }.getOrElse { "未知文件" }
}

private fun querySize(context: android.content.Context, uri: Uri): Long {
    return runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.SIZE), null, null, null)?.use { c ->
            if (c.moveToFirst() && !c.isNull(0)) c.getLong(0) else 0L
        } ?: 0L
    }.getOrElse { 0L }
}