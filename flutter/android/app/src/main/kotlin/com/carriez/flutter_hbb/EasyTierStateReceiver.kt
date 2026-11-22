package com.carriez.flutter_hbb

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import ffi.FFI
import org.json.JSONObject

/** 应用级接收器不依赖 Activity，后台被控时仍持续更新来源白名单。 */
internal class EasyTierStateReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != EasyTierVpnService.ACTION_STATE) return
        val encoded = intent.getStringExtra("snapshot") ?: return
        if (encoded.toByteArray(Charsets.UTF_8).size > 512 * 1024) return
        try { JSONObject(encoded); FFI.onEasyTierState(encoded) }
        catch (_: Exception) { /* 无效消息不刷新有效期，Rust 策略会过期并拒绝来源。 */ }
    }

    companion object {
        fun register(context: Context) {
            val receiver = EasyTierStateReceiver()
            val filter = IntentFilter(EasyTierVpnService.ACTION_STATE)
            if (Build.VERSION.SDK_INT >= 33) context.registerReceiver(receiver, filter, EasyTierVpnService.INTERNAL_PERMISSION, null, Context.RECEIVER_NOT_EXPORTED)
            else @Suppress("DEPRECATION") context.registerReceiver(receiver, filter, EasyTierVpnService.INTERNAL_PERMISSION, null)
        }
    }
}
