package com.carriez.flutter_hbb

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import org.json.JSONObject

/** 只传消息，不把跨进程 IBinder 当作 LocalBinder 使用。 */
internal class EasyTierClient(context: Context) {
    private val context = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private var service: Messenger? = null
    private var bound = false
    private var closed = false
    private var sequence = 0
    private val pending = LinkedHashMap<Int, Pair<JSONObject, (JSONObject?, String?) -> Unit>>()
    private val sent = HashSet<Int>()
    private val replies = Messenger(object : Handler(Looper.getMainLooper()) {
        override fun handleMessage(message: Message) {
            if (message.what != EasyTierVpnService.RPC) return
            val request = pending.remove(message.arg1) ?: return
            sent.remove(message.arg1)
            val response = try { JSONObject(message.data.getString("response") ?: error("缺少响应")) }
            catch (_: Exception) { request.second(null, "EasyTier 服务返回无效数据"); return }
            request.second(response, null)
        }
    })

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            if (closed || binder == null) return
            service = Messenger(binder)
            pending.keys.toList().forEach { send(it) }
        }
        override fun onServiceDisconnected(name: ComponentName?) {
            service = null
            val callbacks = pending.values.toList()
            pending.clear(); sent.clear()
            callbacks.forEach { it.second(null, "EasyTier 后台服务连接已断开") }
        }
        override fun onBindingDied(name: ComponentName?) { onServiceDisconnected(name); unbind() }
        override fun onNullBinding(name: ComponentName?) { onServiceDisconnected(name); unbind() }
    }

    fun request(request: JSONObject, callback: (JSONObject?, String?) -> Unit) {
        if (Looper.myLooper() != Looper.getMainLooper()) { main.post { this.request(request, callback) }; return }
        if (closed) { callback(null, "组网页面已关闭"); return }
        val id = ++sequence
        pending[id] = Pair(request, callback)
        try {
            if (!bound) {
                bound = context.bindService(Intent(context, EasyTierVpnService::class.java).setAction(EasyTierVpnService.ACTION_CONTROL), connection, Context.BIND_AUTO_CREATE)
                check(bound) { "无法连接 EasyTier 后台服务" }
            }
            send(id)
            main.postDelayed({
                pending.remove(id)?.let { sent.remove(id); it.second(null, "等待 EasyTier 服务响应超时，请刷新查看实际状态") }
            }, 15000)
        } catch (error: Exception) { pending.remove(id); sent.remove(id); callback(null, error.message ?: "无法连接组网服务") }
    }

    private fun send(id: Int) {
        val target = service ?: return
        val request = pending[id] ?: return
        if (!sent.add(id)) return
        try {
            target.send(Message.obtain(null, EasyTierVpnService.RPC).apply {
                arg1 = id; replyTo = replies
                data = Bundle().apply { putString("request", request.first.toString()) }
            })
        } catch (_: Exception) { sent.remove(id); pending.remove(id)?.second?.invoke(null, "无法发送 EasyTier 服务请求") }
    }

    private fun unbind() {
        if (bound) { try { context.unbindService(connection) } catch (_: IllegalArgumentException) {} }
        bound = false; service = null
    }

    fun close() {
        closed = true
        val callbacks = pending.values.toList()
        pending.clear(); sent.clear(); unbind()
        callbacks.forEach { it.second(null, "组网页面已关闭") }
    }
}
