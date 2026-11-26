package com.tvdisplay.remote.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Base64

fun decodePoster(encoded: String?): Bitmap? {
    if (encoded.isNullOrEmpty()) return null
    val bytes = try {
        Base64.decode(encoded, Base64.DEFAULT)
    } catch (_: IllegalArgumentException) {
        return null
    }
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
    var sample = 1
    while (bounds.outWidth / sample > 1920 || bounds.outHeight / sample > 1920) {
        sample *= 2
        if (sample >= 32) break
    }
    val options = BitmapFactory.Options().apply { inSampleSize = sample }
    return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
}
