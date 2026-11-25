package com.carriez.flutter_hbb

import android.content.Context
import android.util.AtomicFile
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.UUID

/** 仅由组网服务串行访问；配置不会进入 Intent、通知或日志。 */
internal class EasyTierProfileStore(context: Context) {
    companion object {
        const val MAX_TOML = 128 * 1024
        const val MAX_STORE = 16 * 1024 * 1024
    }
    private val directory = File(context.noBackupFilesDir, "easytier")
    private val file = AtomicFile(File(directory, "profiles.json"))
    private var document = load()
    val profiles: JSONArray get() = document.getJSONArray("profiles")
    var selected: String
        get() = document.optString("selected_id")
        set(value) { find(value); mutate { document.put("selected_id", value) } }
    var remoteOnly: Boolean
        get() = document.optBoolean("remote_only")
        set(value) { mutate { document.put("remote_only", value) } }

    private fun mutate(change: () -> Unit) {
        val previous = document.toString()
        try { change(); persist() }
        catch (error: Exception) { document = JSONObject(previous); throw error }
    }

    private fun load(): JSONObject {
        if (!file.baseFile.exists() && !File(file.baseFile.path + ".bak").exists())
            return JSONObject().put("profiles", JSONArray()).put("selected_id", "").put("remote_only", false)
        val data = file.readFully()
        check(data.size <= MAX_STORE) { "EasyTier 方案文件过大" }
        val result = JSONObject(String(data, Charsets.UTF_8))
        val values = result.getJSONArray("profiles")
        check(values.length() <= 64) { "EasyTier 方案数量超过限制" }
        val ids = HashSet<String>()
        for (i in 0 until values.length()) {
            val entry = values.getJSONObject(i)
            val id = UUID.fromString(entry.getString("id")).toString()
            check(ids.add(id)) { "EasyTier 方案 ID 重复" }
            name(entry.getString("name"))
            check(entry.getString("toml").toByteArray(Charsets.UTF_8).size <= MAX_TOML) { "EasyTier TOML 过大" }
        }
        if (!ids.contains(result.optString("selected_id")))
            result.put("selected_id", if (values.length() > 0) values.getJSONObject(0).getString("id") else "")
        return result
    }

    private fun persist() {
        check(directory.isDirectory || directory.mkdirs()) { "无法创建 EasyTier 配置目录" }
        val data = document.toString().toByteArray(Charsets.UTF_8)
        check(data.size <= MAX_STORE) { "EasyTier 方案总长度超过限制" }
        val stream = file.startWrite()
        try { stream.write(data); file.finishWrite(stream) }
        catch (error: Exception) { file.failWrite(stream); document = load(); throw error }
    }

    fun find(id: String): JSONObject {
        for (i in 0 until profiles.length()) {
            val profile = profiles.getJSONObject(i)
            if (profile.getString("id") == id) return JSONObject(profile.toString())
        }
        error("EasyTier 方案不存在")
    }

    fun chosen(id: String?): JSONObject = find(id?.takeIf { it.isNotEmpty() } ?: selected)

    fun save(id: String?, title: String, toml: String): JSONObject {
        check(toml.toByteArray(Charsets.UTF_8).size <= MAX_TOML) { "EasyTier TOML 过大" }
        val profileId = id?.also { UUID.fromString(it); find(it) } ?: UUID.randomUUID().toString()
        val value = JSONObject().put("id", profileId).put("name", name(title)).put("toml", toml)
        var index = -1
        for (i in 0 until profiles.length()) if (profiles.getJSONObject(i).getString("id") == profileId) index = i
        mutate {
            if (index >= 0) profiles.put(index, value) else {
                check(profiles.length() < 64) { "EasyTier 方案最多 64 个" }
                profiles.put(value)
            }
            if (selected.isEmpty()) document.put("selected_id", profileId)
        }
        return JSONObject(value.toString())
    }

    fun delete(id: String) {
        find(id)
        val remaining = JSONArray()
        for (i in 0 until profiles.length()) {
            val value = profiles.getJSONObject(i)
            if (value.getString("id") != id) remaining.put(value)
        }
        mutate {
            document.put("profiles", remaining)
            if (selected == id) document.put("selected_id", if (remaining.length() > 0) remaining.getJSONObject(0).getString("id") else "")
        }
    }

    fun summaries(): JSONArray = JSONArray().also { values ->
        for (i in 0 until profiles.length()) {
            val item = profiles.getJSONObject(i)
            values.put(JSONObject().put("id", item.getString("id")).put("name", item.getString("name")))
        }
    }

    private fun name(value: String): String {
        val text = value.trim()
        require(text.isNotEmpty() && text.codePointCount(0, text.length) <= 80 && text.none { it.isISOControl() }) { "请输入不超过 80 个字符的有效方案名" }
        return text
    }
}
