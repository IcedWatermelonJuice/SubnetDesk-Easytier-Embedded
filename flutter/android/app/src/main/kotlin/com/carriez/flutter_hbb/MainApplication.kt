package com.carriez.flutter_hbb

import android.app.Application
import android.app.ActivityManager
import android.os.Build
import android.os.Process
import android.util.Log
import ffi.FFI

class MainApplication : Application() {
    companion object {
        private const val TAG = "MainApplication"
    }

    override fun onCreate() {
        super.onCreate()
        // 组网子进程只加载独立的 EasyTier 库，不能初始化远控服务和原 FFI。
        val processName = if (Build.VERSION.SDK_INT >= 28) getProcessName() else {
            (getSystemService(ACTIVITY_SERVICE) as ActivityManager).runningAppProcesses
                ?.firstOrNull { it.pid == Process.myPid() }?.processName
                ?: java.io.File("/proc/self/cmdline").readText(Charsets.UTF_8).substringBefore('\u0000')
        }
        if (processName.endsWith(":easytier")) return
        Log.d(TAG, "App start")
        FFI.onAppStart(applicationContext)
        FFI.initLanCredentialStore(LanCredentialStore(applicationContext))
        EasyTierStateReceiver.register(applicationContext)
    }
}
