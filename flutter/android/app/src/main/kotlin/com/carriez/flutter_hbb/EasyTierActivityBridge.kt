package com.carriez.flutter_hbb

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.net.VpnService
import android.os.Build
import android.provider.OpenableColumns
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodChannel
import org.json.JSONObject
import java.util.concurrent.Executors

/** 系统授权和文档选择必须由可见 Activity 发起，服务不自行弹授权窗口。 */
internal class EasyTierActivityBridge(private val activity: Activity, messenger: BinaryMessenger) {
    companion object {
        private const val VPN = 48201
        private const val IMPORT = 48202
        private const val EXPORT = 48203
    }
    private val channel = MethodChannel(messenger, "com.zibochen.subnetdesk/easytier")
    private val client = EasyTierClient(activity)
    private val io = Executors.newSingleThreadExecutor()
    private var resumed = false
    private var pending: MethodChannel.Result? = null
    private var pendingRequest: JSONObject? = null
    private var exportToml: String? = null
    private var closed = false
    private val activeResults = HashSet<OnceResult>()

    /** Activity 退出与异步文档回调可能交错，每次 Flutter 调用只能完成一次。 */
    private class OnceResult(
        private val target: MethodChannel.Result,
        private val finished: (OnceResult) -> Unit
    ) : MethodChannel.Result {
        private var done = false
        private fun finish(reply: () -> Unit) {
            if (done) return
            done = true
            finished(this)
            reply()
        }
        override fun success(result: Any?) = finish { target.success(result) }
        override fun error(code: String, message: String?, details: Any?) = finish { target.error(code, message, details) }
        override fun notImplemented() = finish { target.notImplemented() }
    }

    init {
        channel.setMethodCallHandler { call, originalResult ->
            val result = OnceResult(originalResult) { activeResults.remove(it) }
            activeResults.add(result)
            try {
                when (call.method) {
                    "status" -> rpc(JSONObject().put("action", "status"), result)
                    "readProfile" -> client.request(JSONObject().put("action", "read_profile").put("id", call.argument<String>("id"))) { response, error ->
                        complete(response, error, result) { it.getJSONObject("profile").toString() }
                    }
                    "command" -> {
                        val request = JSONObject(call.argument<String>("request") ?: error("缺少组网请求"))
                        val needsVpn = request.optString("action") == "start" ||
                            (request.optString("action") in listOf("set_remote_only", "remote_only") && request.optBoolean("enabled"))
                        if (needsVpn) {
                            ensureInteractive()
                            val prepare = VpnService.prepare(activity)
                            if (prepare != null) {
                                begin(result, request)
                                activity.startActivityForResult(prepare, VPN)
                            } else dispatchVpn(request, result)
                        } else rpc(request, result)
                    }
                    "importProfile" -> {
                        ensureInteractive()
                        begin(result, JSONObject().put("action", "import_content").put("request_id", call.argument<String>("request_id"))
                            .put("name", call.argument<String>("name") ?: ""))
                        activity.startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
                            .putExtra(Intent.EXTRA_MIME_TYPES, arrayOf("text/plain", "application/toml", "application/octet-stream")), IMPORT)
                    }
                    "exportProfile" -> {
                        ensureInteractive()
                        val request = JSONObject().put("action", "external_complete").put("request_id", call.argument<String>("request_id"))
                        begin(result, request)
                        client.request(JSONObject().put("action", "export_content").put("id", call.argument<String>("id"))) { response, error ->
                            if (closed || pending !== result) return@request
                            if (error != null || response?.optBoolean("ok") != true) { failPending(error ?: response?.optString("error") ?: "无法导出方案"); return@request }
                            try {
                                val profile = response.getJSONObject("profile")
                                exportToml = profile.getString("toml")
                                val name = profile.getString("name").replace(Regex("[\\\\/:*?\"<>|\\p{Cntrl}]"), "_")
                                activity.startActivityForResult(Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
                                    .setType("text/plain").putExtra(Intent.EXTRA_TITLE, "$name.toml"), EXPORT)
                            } catch (problem: Exception) { failPending(problem.message ?: "无法打开文档保存位置") }
                        }
                    }
                    else -> result.notImplemented()
                }
            } catch (error: Exception) {
                if (pending === result) { pending = null; pendingRequest = null; exportToml = null }
                result.error("easytier", error.message ?: "无法执行组网操作", null)
            }
        }
    }

    fun onResume() { resumed = true }
    fun onPause() { resumed = false }

    private fun ensureInteractive() {
        check(resumed && !activity.isFinishing && !closed) { "请在前台打开组网页面后操作" }
        check(pending == null) { "请先完成当前系统授权或文件选择" }
    }

    private fun begin(result: MethodChannel.Result, request: JSONObject) {
        check(pending == null) { "已有等待完成的组网操作" }
        pending = result; pendingRequest = request
    }

    private fun dispatchVpn(request: JSONObject, result: MethodChannel.Result) {
        val intent = Intent(activity, EasyTierVpnService::class.java).setAction(EasyTierVpnService.ACTION_PREPARE)
        if (Build.VERSION.SDK_INT >= 26) activity.startForegroundService(intent) else activity.startService(intent)
        rpc(request, result)
    }

    private fun rpc(request: JSONObject, result: MethodChannel.Result) = client.request(request) { response, error ->
        complete(response, error, result) { it.toString() }
    }

    private fun complete(response: JSONObject?, error: String?, result: MethodChannel.Result, encode: (JSONObject) -> String) {
        if (closed) return
        when {
            error != null -> result.error("easytier_service", error, null)
            response == null || (response.has("ok") && !response.optBoolean("ok")) -> result.error("easytier", response?.optString("error") ?: "组网操作失败", null)
            else -> try { result.success(encode(response)) } catch (_: Exception) { result.error("easytier", "组网服务数据无效", null) }
        }
    }

    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode !in listOf(VPN, IMPORT, EXPORT)) return false
        val result = pending ?: return true
        val request = pendingRequest ?: return true
        if (resultCode != Activity.RESULT_OK) {
            pending = null; pendingRequest = null; exportToml = null
            result.error("cancelled", "已取消系统授权或文件选择", null)
            return true
        }
        if (requestCode == VPN) {
            pending = null; pendingRequest = null
            try { dispatchVpn(request, result) } catch (error: Exception) { result.error("easytier", error.message ?: "无法启动前台组网服务", null) }
            return true
        }
        val uri = data?.data
        if (uri == null) { failPending("系统没有返回可用的文档"); return true }
        val text = exportToml
        pending = null; pendingRequest = null; exportToml = null
        io.execute {
            try {
                if (requestCode == IMPORT) {
                    val bytes = activity.contentResolver.openInputStream(uri)?.use { it.readBytesLimited(EasyTierProfileStore.MAX_TOML) }
                        ?: error("无法读取选中的 TOML 文档")
                    val text = Charsets.UTF_8.newDecoder().onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
                        .onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT).decode(java.nio.ByteBuffer.wrap(bytes)).toString()
                    request.put("toml", text)
                    if (request.optString("name").isEmpty()) request.put("name", documentName(uri).removeSuffix(".toml").take(80).ifBlank { "导入的方案" })
                } else {
                    check(text != null) { "导出内容已失效，请重试" }
                    activity.contentResolver.openOutputStream(uri, "wt")?.use { it.write(text.toByteArray(Charsets.UTF_8)) }
                        ?: error("无法写入选中的文档")
                }
                rpc(request, result)
            } catch (error: Exception) { activity.runOnUiThread { if (!closed) result.error("easytier_document", error.message ?: "TOML 文档读写失败", null) } }
        }
        return true
    }

    private fun documentName(uri: Uri): String = activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
        if (it.moveToFirst()) it.getString(0) else null
    } ?: "导入的方案"

    private fun java.io.InputStream.readBytesLimited(limit: Int): ByteArray {
        val output = java.io.ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (true) {
            val length = read(buffer)
            if (length < 0) break
            check(output.size() + length <= limit) { "TOML 文档超过 128 KiB 限制" }
            output.write(buffer, 0, length)
        }
        return output.toByteArray()
    }

    private fun failPending(error: String) {
        val result = pending
        pending = null; pendingRequest = null; exportToml = null
        result?.error("easytier", error, null)
    }

    fun close() {
        failPending("页面已关闭，请重新执行操作")
        activeResults.toList().forEach { it.error("easytier_closed", "页面已关闭，请重新执行操作", null) }
        closed = true
        channel.setMethodCallHandler(null)
        client.close(); io.shutdown()
    }
}
