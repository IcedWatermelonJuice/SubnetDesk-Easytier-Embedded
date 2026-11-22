package com.carriez.flutter_hbb

import androidx.annotation.Keep

/** 与 RustDesk 分开的组网库，只在 :easytier 进程加载。 */
@Keep
object EasyTierNative {
    val available: Boolean by lazy {
        try {
            System.loadLibrary("subnetdesk_easytier_android")
            true
        } catch (_: UnsatisfiedLinkError) {
            false
        } catch (_: SecurityException) {
            false
        }
    }

    external fun start(configToml: String): String
    external fun status(): String
    external fun setTunFd(fd: Int): String
    external fun stop(): String
    external fun profile(toml: String, fieldsJson: String): String
}
