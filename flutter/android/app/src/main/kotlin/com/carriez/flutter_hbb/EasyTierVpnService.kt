package com.carriez.flutter_hbb

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.ParcelFileDescriptor
import android.system.OsConstants
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

/** 核心与底层网络绑定仅发生在 :easytier，主进程仍可经 TUN 远控。 */
class EasyTierVpnService : VpnService() {
    companion object {
        const val ACTION_CONTROL = "com.zibochen.subnetdesk.EASYTIER_CONTROL"
        const val ACTION_PREPARE = "com.zibochen.subnetdesk.EASYTIER_PREPARE"
        const val ACTION_STOP = "com.zibochen.subnetdesk.EASYTIER_STOP"
        const val ACTION_STATE = "com.zibochen.subnetdesk.EASYTIER_STATE"
        const val INTERNAL_PERMISSION = "com.zibochen.subnetdesk.permission.EASYTIER_INTERNAL"
        const val RPC = 1
        private const val CHANNEL = "easytier-network"
        private const val NOTIFICATION = 47801
    }

    private val worker = Executors.newSingleThreadScheduledExecutor()
    private lateinit var store: EasyTierProfileStore
    private lateinit var connectivity: ConnectivityManager
    private var poll: ScheduledFuture<*>? = null
    private var networkRequested = false
    private var physicalNetwork: Network? = null
    private var tunnel: ParcelFileDescriptor? = null
    private var tunnelKey = ""
    private var state = "stopped"
    private var runtimeError = ""
    private var runningId = ""
    private var runningToml = ""
    private var wantedProfile: JSONObject? = null
    private var hostname = ""
    private var runningHostname = ""
    @Volatile private var nativeStarted = false
    @Volatile private var nativeGeneration = 0
    private var lastNative = JSONObject()
    private var completedId = ""
    private var commandError = ""
    private var foreground = false
    @Volatile private var stopping = false
    private var fatalStoreError = ""

    private val messenger by lazy { Messenger(object : Handler(Looper.getMainLooper()) {
        override fun handleMessage(message: Message) {
            // 此服务有系统 BIND_VPN_SERVICE 权限，控制协议仍额外限制为应用 UID。
            if (message.what != RPC || message.sendingUid != applicationInfo.uid || stopping) return
            val reply = message.replyTo ?: return
            val sequence = message.arg1
            val encoded = message.data.getString("request") ?: return
            worker.execute {
                val result = try {
                    require(encoded.toByteArray(Charsets.UTF_8).size <= 512 * 1024) { "组网请求过大" }
                    execute(JSONObject(encoded))
                } catch (error: Exception) {
                    JSONObject().put("ok", false).put("error", error.message ?: "组网操作失败")
                }
                try { reply.send(Message.obtain(null, RPC).apply {
                    arg1 = sequence; data = Bundle().apply { putString("response", result.toString()) }
                }) } catch (_: Exception) { /* 调用页面可能已经退出，服务状态仍可重新查询。 */ }
            }
        }
    }) }

    override fun onCreate() {
        super.onCreate()
        connectivity = getSystemService(CONNECTIVITY_SERVICE) as ConnectivityManager
        try { store = EasyTierProfileStore(this) }
        catch (error: Exception) { fatalStoreError = error.message ?: "无法读取组网方案"; state = "failed" }
        poll = worker.scheduleWithFixedDelay({
            try { refreshCore(); publish(false) }
            catch (error: Exception) { fail(error.message ?: "组网状态查询失败") }
        }, 1, 2, TimeUnit.SECONDS)
    }

    override fun onBind(intent: Intent?): IBinder? =
        if (intent?.action == SERVICE_INTERFACE) super.onBind(intent) else
            if (intent?.action == ACTION_CONTROL) messenger.binder else null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_PREPARE -> {
                try { ensureForeground() }
                catch (error: Exception) { worker.execute { fail(error.message ?: "无法启动前台组网服务") } }
            }
            ACTION_STOP -> worker.execute {
                try { stopNetwork(true) }
                catch (error: Exception) { state = "failed"; runtimeError = error.message ?: "核心停止失败" }
                finally { publish(true); stopSelf(); releaseFailedCore() }
            }
        }
        // 未获得新的用户操作时，不在系统重启服务后偷偷重新申请 VPN 授权。
        return START_NOT_STICKY
    }

    private fun execute(request: JSONObject): JSONObject {
        val action = request.optString("action")
        if (action == "status") return snapshot(false)
        if (fatalStoreError.isNotEmpty()) error(fatalStoreError)
        if (action == "read_profile") {
            val profile = store.find(request.getString("id"))
            val parsed = if (EasyTierNative.available) nativeProfile(profile.getString("toml"), JSONObject()) else JSONObject()
            profile.put("fields", parsed.optJSONObject("fields") ?: JSONObject())
            return JSONObject().put("ok", true).put("profile", profile)
        }
        if (action == "export_content") return JSONObject().put("ok", true)
            .put("profile", store.chosen(request.optString("id")))

        val requestId = request.optString("request_id")
        commandError = ""
        try {
            when (action) {
                "save" -> {
                    val id = request.optString("id").takeIf { it.isNotEmpty() }
                    val old = id?.let { store.find(it) }
                    val original = if (request.has("toml")) request.getString("toml") else old?.getString("toml") ?: ""
                    val toml = if (old != null && !request.has("toml") && !request.has("fields")) original else
                        nativeProfile(original, request.optJSONObject("fields") ?: JSONObject()).getString("toml")
                    store.save(id, request.getString("name"), toml)
                }
                "select" -> store.selected = request.getString("id")
                "duplicate" -> {
                    val original = store.find(request.getString("id"))
                    store.save(null, request.optString("name").ifEmpty { original.getString("name") + " 副本" }, original.getString("toml"))
                }
                "delete" -> {
                    require(request.getString("id") != runningId) { "请先停止运行中的方案" }
                    store.delete(request.getString("id"))
                }
                "import_content" -> {
                    val parsed = nativeProfile(request.getString("toml"), JSONObject())
                    store.save(null, request.optString("name").ifEmpty { "导入的方案" }, parsed.getString("toml"))
                }
                "start" -> startNetwork(request)
                "set_remote_only", "remote_only" -> {
                    val enabled = request.getBoolean("enabled")
                    if (enabled) {
                        require(prepare(this) == null) { "请先允许系统 VPN 授权" }
                        store.remoteOnly = true
                        // 设置限制时先清空来源快照，失败也不放开 LAN。
                        publish(false)
                        startNetwork(request)
                    } else { store.remoteOnly = false; publish(false) }
                }
                "stop" -> stopNetwork(true)
                "refresh" -> refreshCore()
                "external_complete" -> Unit
                else -> error("不支持的 EasyTier 操作")
            }
        } catch (error: Throwable) {
            commandError = if (error is UnsatisfiedLinkError) "应用包缺少 Android EasyTier 原生组件" else error.message ?: "组网操作失败"
            if (action == "start" || (action in listOf("set_remote_only", "remote_only") && request.optBoolean("enabled"))) {
                wantedProfile = null
                fail(commandError)
            }
        }
        completedId = requestId
        publish(action == "stop")
        if (action == "stop") releaseFailedCore()
        return snapshot(action == "stop")
    }

    private fun nativeProfile(toml: String, fields: JSONObject): JSONObject {
        check(EasyTierNative.available) { "应用包缺少 Android EasyTier 原生组件" }
        return callNative { EasyTierNative.profile(toml, fields.toString()) }
    }

    private fun callNative(block: () -> String): JSONObject = try { checkedNative(block()) }
        catch (_: LinkageError) { error("应用包中的 Android EasyTier 原生组件不完整") }

    private fun checkedNative(encoded: String): JSONObject = JSONObject(encoded).also {
        check(it.optBoolean("ok")) { it.optString("error", "EasyTier 原生操作失败") }
    }

    private fun startNetwork(request: JSONObject) {
        check(EasyTierNative.available) { "应用包缺少 Android EasyTier 原生组件" }
        check(prepare(this) == null) { "请先允许系统 VPN 授权" }
        val profile = store.chosen(request.optString("id"))
        hostname = request.optString("hostname").trim().ifEmpty { Build.MODEL ?: "Android" }
        require(hostname.none { it.isISOControl() } && hostname.codePointCount(0, hostname.length) <= 128) { "设备名称无效" }
        state = "starting"; lastNative = JSONObject(); publish(false)
        stopCore()
        wantedProfile = profile
        state = "starting"; runtimeError = ""
        ensureForeground()
        if (!networkRequested) {
            val networkRequest = NetworkRequest.Builder().addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN).build()
            connectivity.requestNetwork(networkRequest, networkCallback)
            networkRequested = true
        }
        if (physicalNetwork != null) startCore()
    }

    private fun enqueueNetworkChange(change: () -> Unit) {
        if (stopping) return
        try { worker.execute { if (!stopping) change() } }
        catch (error: RejectedExecutionException) {
            if (!stopping) android.util.Log.w("EasyTierVpnService", "无法处理底层网络变化", error)
        }
    }

    private val networkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) { enqueueNetworkChange {
            try {
                if (!networkRequested) return@enqueueNetworkChange
                if (network == physicalNetwork) return@enqueueNetworkChange
                state = "starting"; lastNative = JSONObject(); publish(false)
                stopCore()
                val bound = if (Build.VERSION.SDK_INT >= 23) connectivity.bindProcessToNetwork(network)
                    else @Suppress("DEPRECATION") ConnectivityManager.setProcessDefaultNetwork(network)
                check(bound) { "无法绑定组网底层网络" }
                physicalNetwork = network
                // stopCore 已关闭旧 TUN；新 TUN 的底层网络交给 Builder 设置。
                if (wantedProfile != null) { state = "starting"; startCore() }
            } catch (error: Exception) { fail(error.message ?: "底层网络切换失败") }
        } }
        override fun onLost(network: Network) { enqueueNetworkChange {
            if (physicalNetwork == network) {
                physicalNetwork = null
                state = "starting"; lastNative = JSONObject(); publish(false)
                try {
                    stopCore()
                    state = if (wantedProfile != null) "starting" else "stopped"
                    runtimeError = if (wantedProfile != null) "等待可用的底层网络" else ""
                } catch (error: Exception) { fail(error.message ?: "底层网络丢失，核心停止失败") }
                finally { publish(false) }
            }
        } }
        override fun onUnavailable() { enqueueNetworkChange { if (networkRequested) fail("没有可用的底层网络") } }
    }

    private fun startCore() {
        val profile = wantedProfile ?: return
        val patched = nativeProfile(profile.getString("toml"), JSONObject().put("hostname", hostname))
        nativeStarted = true
        nativeGeneration += 1
        callNative { EasyTierNative.start(patched.getString("toml")) }
        runningId = profile.getString("id"); runningToml = profile.getString("toml")
        runningHostname = hostname
        lastNative = JSONObject(); runtimeError = ""; state = "starting"
        publish(false)
    }

    private fun refreshCore() {
        if (runningId.isEmpty()) return
        check(EasyTierNative.available) { "应用包缺少 Android EasyTier 原生组件" }
        val status = callNative { EasyTierNative.status() }
        if (status.optString("state") == "failed") { fail(status.optString("error", "EasyTier 已停止")); return }
        lastNative = status
        val address = status.optString("ipv4")
        if (address.isNotEmpty()) {
            configureTunnel(status, address)
            state = if (tunnel != null && status.optBoolean("tun_ready")) "running" else "starting"
        } else state = "starting"
    }

    private fun ipv4(cidr: String, allowDefault: Boolean): Pair<String, Int> {
        val parts = cidr.split('/')
        require(parts.size == 2) { "组网地址缺少网络前缀" }
        val octets = parts[0].split('.')
        require(octets.size == 4 && octets.all { part -> part.isNotEmpty() && part.all { it in '0'..'9' } && part.toIntOrNull()?.let { it in 0..255 } == true }) { "组网 IPv4 地址无效" }
        val bits = parts[1].toIntOrNull() ?: error("组网网络前缀无效")
        require(bits in 0..32 && (bits > 0 || allowDefault)) { "不接受未配置的默认路由" }
        return Pair(parts[0], bits)
    }

    private fun configureTunnel(status: JSONObject, address: String) {
        val local = ipv4(address, false)
        val routes = status.optJSONArray("tun_routes") ?: JSONArray()
        val allowDefault = status.optBoolean("allow_default_route")
        val normalized = (0 until routes.length()).map { routes.getString(it) }.distinct().sorted()
        val mtu = status.optInt("mtu", 1380)
        require(mtu in 576..9000) { "EasyTier MTU 超出 Android 支持范围" }
        val network = physicalNetwork ?: error("组网底层网络已断开")
        val key = "$address:$mtu:$allowDefault:$network:" + normalized.joinToString(",")
        if (tunnel != null && key == tunnelKey) return
        val builder = Builder().setSession("SubnetDesk EasyTier").setMtu(mtu)
            .addAddress(local.first, local.second).allowFamily(OsConstants.AF_INET6)
        if (Build.VERSION.SDK_INT >= 22) builder.setUnderlyingNetworks(arrayOf(network))
        for (route in normalized) {
            if (route == "0.0.0.0/0" && !allowDefault) continue
            val parsed = ipv4(route, allowDefault)
            builder.addRoute(parsed.first, parsed.second)
        }
        // 不添加默认 DNS；未配置的互联网和 IPv6 流量保持系统原有路径。
        val next = builder.establish() ?: error("无法建立 VPN 虚拟网卡，系统授权可能已撤销")
        try {
            if (Build.VERSION.SDK_INT >= 22) check(setUnderlyingNetworks(arrayOf(network))) { "无法设置 VPN 底层网络" }
            callNative { EasyTierNative.setTunFd(next.fd) }
        }
        catch (error: Throwable) { next.close(); throw error }
        val previous = tunnel
        tunnel = next; tunnelKey = key
        previous?.close()
    }

    private fun stopCore() {
        try {
            if (nativeStarted) {
                check(EasyTierNative.available) { "Android EasyTier 原生组件不可用" }
                callNative { EasyTierNative.stop() }
                nativeStarted = false
            }
        } finally {
            runningId = ""; runningToml = ""; runningHostname = ""; lastNative = JSONObject()
            val descriptor = tunnel
            tunnel = null; tunnelKey = ""
            descriptor?.close()
        }
    }

    private fun stopNetwork(intentional: Boolean) {
        wantedProfile = null
        if (intentional && ::store.isInitialized) store.remoteOnly = false
        var problem: Exception? = null
        try { stopCore() } catch (error: Exception) { problem = error } finally {
            if (networkRequested) {
                networkRequested = false
                try { connectivity.unregisterNetworkCallback(networkCallback) }
                catch (error: Exception) { if (problem == null) problem = error }
            }
            physicalNetwork = null
            if (!nativeStarted) {
                try {
                    val unbound = if (Build.VERSION.SDK_INT >= 23) connectivity.bindProcessToNetwork(null)
                        else @Suppress("DEPRECATION") ConnectivityManager.setProcessDefaultNetwork(null)
                    check(unbound) { "无法释放组网底层网络绑定" }
                } catch (error: Exception) { if (problem == null) problem = error }
            }
            try { clearForeground() }
            catch (error: Exception) { if (problem == null) problem = error }
            state = if (problem == null) "stopped" else "failed"
            runtimeError = problem?.message ?: ""
        }
        problem?.let { throw it }
    }

    private fun fail(message: String) {
        // 失败后取消待启动方案和网络请求，避免迟到的网络回调再次启动核心。
        try { stopNetwork(false) }
        catch (error: Exception) { android.util.Log.w("EasyTierVpnService", "组网故障后的清理未完成", error) }
        state = "failed"; runtimeError = message
        publish(false)
        releaseFailedCore()
    }

    private fun releaseFailedCore() {
        if (!nativeStarted) return
        val generation = nativeGeneration
        // 只有独立组网进程可使用最后的释放保障；绝不终止远控主进程。
        Handler(Looper.getMainLooper()).postDelayed({
            if (!nativeStarted || nativeGeneration != generation) return@postDelayed
            val process = try { java.io.File("/proc/self/cmdline").readText(Charsets.UTF_8).substringBefore('\u0000') }
                catch (_: Exception) { "" }
            if (process.endsWith(":easytier")) android.os.Process.killProcess(android.os.Process.myPid())
        }, 250)
    }

    private fun snapshot(intentional: Boolean): JSONObject {
        val result = JSONObject(lastNative.toString())
        result.put("available", EasyTierNative.available && fatalStoreError.isEmpty())
            // 存储损坏时无法确认用户先前的安全选择，不能广播 false 放开来源。
            .put("state", state).put("remote_only", !::store.isInitialized || store.remoteOnly)
            .put("profiles", if (::store.isInitialized) store.summaries() else JSONArray())
            .put("selected_id", if (::store.isInitialized) store.selected else "")
            .put("running_id", runningId).put("completed_request_id", completedId)
            .put("running_hostname", runningHostname)
            .put("command_error", commandError).put("intentional_stop", intentional)
            .put("error", fatalStoreError.ifEmpty { runtimeError.ifEmpty {
                if (!EasyTierNative.available) "应用包缺少 Android EasyTier 原生组件" else lastNative.optString("error")
            } })
        val changed = if (runningId.isEmpty()) false else try { store.find(runningId).getString("toml") != runningToml } catch (_: Exception) { true }
        result.put("running_config_changed", changed)
        if (state != "running") result.put("nodes", JSONArray()).put("routes", JSONArray()).put("queried_at", JSONObject.NULL)
        if (!result.has("nodes")) result.put("nodes", JSONArray())
        if (!result.has("routes")) result.put("routes", JSONArray())
        return result
    }

    private fun publish(intentional: Boolean) {
        sendBroadcast(Intent(ACTION_STATE).setPackage(packageName).putExtra("snapshot", snapshot(intentional).toString()), INTERNAL_PERMISSION)
    }

    @Synchronized private fun clearForeground() {
        if (!foreground) return
        @Suppress("DEPRECATION") stopForeground(true)
        foreground = false
    }

    @Synchronized private fun ensureForeground() {
        if (foreground) return
        check(prepare(this) == null) { "VPN 系统授权尚未获得" }
        val manager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= 26) manager.createNotificationChannel(NotificationChannel(CHANNEL, "EasyTier 组网", NotificationManager.IMPORTANCE_LOW))
        val immutable = if (Build.VERSION.SDK_INT >= 23) PendingIntent.FLAG_IMMUTABLE else 0
        val stop = PendingIntent.getService(this, 0, Intent(this, EasyTierVpnService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_UPDATE_CURRENT or immutable)
        val open = PendingIntent.getActivity(this, 1, Intent(this, MainActivity::class.java), PendingIntent.FLAG_UPDATE_CURRENT or immutable)
        val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(this)
        val notification = builder.setSmallIcon(R.drawable.ic_stat_easytier).setContentTitle("EasyTier 组网")
            .setContentText("SubnetDesk 组网服务正在运行").setContentIntent(open).setOngoing(true)
            .addAction(Notification.Action.Builder(android.R.drawable.ic_menu_close_clear_cancel, "停止组网", stop).build()).build()
        if (Build.VERSION.SDK_INT >= 34) startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SYSTEM_EXEMPTED)
        else startForeground(NOTIFICATION, notification)
        foreground = true
    }

    override fun onRevoke() {
        worker.execute { try { stopNetwork(false) } finally { state = "failed"; runtimeError = "VPN 系统授权已撤销"; publish(false); stopSelf(); releaseFailedCore() } }
        super.onRevoke()
    }

    override fun onDestroy() {
        stopping = true
        poll?.cancel(false)
        worker.execute {
            val failure = if (state == "failed") runtimeError else null
            try { stopNetwork(false) }
            catch (error: Exception) { state = "failed"; runtimeError = error.message ?: "核心停止失败" }
            finally {
                if (failure != null) { state = "failed"; runtimeError = failure }
                publish(false); worker.shutdown(); releaseFailedCore()
            }
        }
        super.onDestroy()
    }
}
