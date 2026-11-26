package com.tvdisplay.remote.logic

fun shouldBringToForeground(
    forcePopup: Boolean,
    incomingRevision: Long,
    lastShownRevision: Long,
): Boolean = forcePopup && incomingRevision > lastShownRevision
