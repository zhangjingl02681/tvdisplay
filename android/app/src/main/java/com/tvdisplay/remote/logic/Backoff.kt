package com.tvdisplay.remote.logic

fun backoffMillis(attempt: Int): Long {
    require(attempt >= 0) { "attempt 不能为负" }
    val shift = attempt.coerceAtMost(5)
    return minOf(1000L shl shift, 30_000L)
}
