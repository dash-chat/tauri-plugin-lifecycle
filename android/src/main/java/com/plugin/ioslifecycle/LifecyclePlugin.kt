package com.plugin.ioslifecycle

import android.app.Activity
import android.webkit.WebView
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Plugin

@TauriPlugin
class LifecyclePlugin(private val activity: Activity) : Plugin(activity) {

    override fun load(webView: WebView) {
        System.loadLibrary("tauri_app_lib")
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStop(owner: LifecycleOwner) = nativeOnStop()
            override fun onResume(owner: LifecycleOwner) = nativeOnStart()
        })
    }

    private external fun nativeOnStop()
    private external fun nativeOnStart()
}
