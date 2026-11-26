package com.tvdisplay.remote

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import com.tvdisplay.remote.data.Prefs
import com.tvdisplay.remote.logic.parseServerUrl
import com.tvdisplay.remote.service.ConnectionService
import com.tvdisplay.remote.ui.TvDisplayApp

class MainActivity : ComponentActivity() {
    private val notificationPermission = registerForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (Build.VERSION.SDK_INT >= 33) {
            notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
        val prefs = Prefs(this)
        if (prefs.serverUrl.isNotBlank() && prefs.password.isNotBlank()) {
            startConnection()
        }
        setContent {
            TvDisplayApp(
                initialUrl = prefs.serverUrl,
                initialPassword = prefs.password,
                onSave = { url, password ->
                    val parsed = parseServerUrl(url)
                    prefs.serverUrl = parsed.httpBase
                    prefs.password = password
                    prefs.lastRevision = 0L
                    startConnection()
                    parsed.httpBase
                },
            )
        }
    }

    private fun startConnection() {
        val intent = Intent(this, ConnectionService::class.java)
        if (Build.VERSION.SDK_INT >= 26) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }
}
