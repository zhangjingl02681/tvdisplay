package com.tvdisplay.remote.data

import android.content.Context

class Prefs(context: Context) {
    private val prefs = context.getSharedPreferences("tvdisplay", Context.MODE_PRIVATE)

    var serverUrl: String
        get() = prefs.getString(KEY_URL, "").orEmpty()
        set(value) {
            prefs.edit().putString(KEY_URL, value).apply()
        }

    var password: String
        get() = prefs.getString(KEY_PASSWORD, "").orEmpty()
        set(value) {
            prefs.edit().putString(KEY_PASSWORD, value).apply()
        }

    var lastRevision: Long
        get() = prefs.getLong(KEY_REVISION, 0L)
        set(value) {
            prefs.edit().putLong(KEY_REVISION, value).apply()
        }

    private companion object {
        const val KEY_URL = "server_url"
        const val KEY_PASSWORD = "password"
        const val KEY_REVISION = "last_revision"
    }
}
