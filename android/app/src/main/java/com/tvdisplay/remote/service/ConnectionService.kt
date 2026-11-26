package com.tvdisplay.remote.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.core.app.NotificationCompat
import com.tvdisplay.remote.MainActivity
import com.tvdisplay.remote.R
import com.tvdisplay.remote.data.Prefs
import com.tvdisplay.remote.logic.ContentMessage
import com.tvdisplay.remote.logic.DisplayState
import com.tvdisplay.remote.logic.LinkStatus
import com.tvdisplay.remote.logic.authPayload
import com.tvdisplay.remote.logic.backoffMillis
import com.tvdisplay.remote.logic.parseServerMessage
import com.tvdisplay.remote.logic.parseServerUrl
import com.tvdisplay.remote.logic.shouldBringToForeground
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import kotlin.coroutines.resumeWithException

class ConnectionService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var loop: Job? = null
    private val mainHandler = Handler(Looper.getMainLooper())
    private val http = OkHttpClient.Builder()
        .pingInterval(30, java.util.concurrent.TimeUnit.SECONDS)
        .readTimeout(0, java.util.concurrent.TimeUnit.MILLISECONDS)
        .build()

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createChannels()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = statusNotification(getString(R.string.notif_connecting))
        if (Build.VERSION.SDK_INT >= 29) {
            startForeground(STATUS_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            startForeground(STATUS_ID, notification)
        }
        if (loop == null) {
            loop = scope.launch { runLoop() }
        }
        return START_STICKY
    }

    override fun onDestroy() {
        loop?.cancel()
        scope.cancel()
        http.dispatcher.executorService.shutdown()
        super.onDestroy()
    }

    private suspend fun runLoop() {
        val prefs = Prefs(this)
        var attempt = 0
        while (currentCoroutineContext().isActive) {
            val url = prefs.serverUrl
            val password = prefs.password
            if (url.isBlank() || password.isBlank()) {
                DisplayState.updateLink(LinkStatus.NEED_CONFIG)
                delay(1_000)
                continue
            }
            DisplayState.updateLink(if (attempt == 0) LinkStatus.CONNECTING else LinkStatus.RECONNECTING)
            try {
                connectOnce(url, password, prefs) { attempt = 0 }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (ended: ConnectionEnded) {
                val status = if (ended.authRejected) LinkStatus.AUTH_FAILED else LinkStatus.RECONNECTING
                DisplayState.updateLink(status)
                val text = if (ended.authRejected) {
                    getString(R.string.notif_auth_failed)
                } else {
                    getString(R.string.notif_reconnecting)
                }
                updateNotification(text)
                delay(backoffMillis(attempt))
                if (!ended.authRejected || attempt < 8) {
                    attempt += 1
                }
            } catch (error: Exception) {
                Log.w(TAG, "connection ended: ${error.javaClass.simpleName}")
                DisplayState.updateLink(LinkStatus.RECONNECTING)
                updateNotification(getString(R.string.notif_reconnecting))
                delay(backoffMillis(attempt))
                attempt += 1
            }
        }
    }

    private suspend fun connectOnce(
        url: String,
        password: String,
        prefs: Prefs,
        onHealthy: () -> Unit,
    ) {
        val parsed = parseServerUrl(url)
        suspendCancellableCoroutine<Unit> { continuation ->
            val resumed = AtomicBoolean(false)

            fun finish(error: Throwable) {
                if (resumed.compareAndSet(false, true)) {
                    continuation.resumeWithException(error)
                }
            }

            val request = Request.Builder().url(parsed.webSocketUrl).build()
            val socket = http.newWebSocket(
                request,
                object : WebSocketListener() {
                    override fun onOpen(webSocket: WebSocket, response: Response) {
                        webSocket.send(authPayload(password))
                    }

                    override fun onMessage(webSocket: WebSocket, text: String) {
                        val message = try {
                            parseServerMessage(text)
                        } catch (_: Exception) {
                            Log.w(TAG, "ignored malformed message")
                            return
                        }
                        if (message.type != "content") return
                        handleContent(message, prefs)
                        onHealthy()
                        DisplayState.updateLink(LinkStatus.ONLINE)
                        updateNotification(getString(R.string.notif_online))
                    }

                    override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                        webSocket.close(code, null)
                    }

                    override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                        Log.i(TAG, "socket closed code=$code")
                        finish(ConnectionEnded(authRejected = code == 1008))
                    }

                    override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                        Log.w(TAG, "socket failure: ${t.javaClass.simpleName}")
                        finish(t)
                    }
                },
            )
            continuation.invokeOnCancellation { socket.cancel() }
        }
    }

    private fun handleContent(message: ContentMessage, prefs: Prefs) {
        val stealFocus = shouldBringToForeground(
            forcePopup = message.forcePopup,
            incomingRevision = message.revision,
            lastShownRevision = prefs.lastRevision,
        )
        DisplayState.updateContent(message)
        if (message.revision > prefs.lastRevision) {
            prefs.lastRevision = message.revision
        }
        if (stealFocus) {
            mainHandler.post { bringToFront() }
        }
    }

    private fun bringToFront() {
        val launch = Intent(this, MainActivity::class.java).apply {
            addFlags(
                Intent.FLAG_ACTIVITY_NEW_TASK or
                    Intent.FLAG_ACTIVITY_SINGLE_TOP or
                    Intent.FLAG_ACTIVITY_REORDER_TO_FRONT,
            )
        }
        val pending = PendingIntent.getActivity(
            this,
            1,
            launch,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val popup = NotificationCompat.Builder(this, CHANNEL_POPUP)
            .setSmallIcon(R.drawable.ic_stat)
            .setContentTitle(getString(R.string.notif_popup_title))
            .setContentText(getString(R.string.notif_popup_text))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(NotificationCompat.CATEGORY_ALARM)
            .setFullScreenIntent(pending, true)
            .setAutoCancel(true)
            .build()
        notificationManager().notify(POPUP_ID, popup)
        try {
            startActivity(launch)
        } catch (error: Exception) {
            Log.w(TAG, "foreground request blocked: ${error.javaClass.simpleName}")
        }
    }

    private fun createChannels() {
        if (Build.VERSION.SDK_INT < 26) return
        val manager = notificationManager()
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_STATUS, getString(R.string.notif_channel), NotificationManager.IMPORTANCE_LOW),
        )
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_POPUP, getString(R.string.notif_channel_popup), NotificationManager.IMPORTANCE_HIGH),
        )
    }

    private fun updateNotification(text: String) {
        notificationManager().notify(STATUS_ID, statusNotification(text))
    }

    private fun statusNotification(text: String): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return NotificationCompat.Builder(this, CHANNEL_STATUS)
            .setSmallIcon(R.drawable.ic_stat)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(text)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setContentIntent(open)
            .build()
    }

    private fun notificationManager(): NotificationManager {
        return getSystemService(NotificationManager::class.java)
    }

    private class ConnectionEnded(val authRejected: Boolean) : Exception()

    companion object {
        private const val TAG = "TvDisplay"
        private const val STATUS_ID = 7
        private const val POPUP_ID = 8
        private const val CHANNEL_STATUS = "connection"
        private const val CHANNEL_POPUP = "popup"
    }
}
