package org.dashchat.tauripluginlifecycle

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
            override fun onStop(owner: LifecycleOwner) {
                // The app is closing rather than going to the background, and by now
                // the activity is gone, so a pause handler couldn't call into Tauri.
                if (activity.isFinishing) return
                nativeOnStop()
            }
            override fun onResume(owner: LifecycleOwner) = nativeOnStart()
        })
    }

    private external fun nativeOnStop()
    private external fun nativeOnStart()
}
