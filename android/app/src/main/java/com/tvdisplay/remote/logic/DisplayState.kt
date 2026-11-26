package com.tvdisplay.remote.logic

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

enum class LinkStatus {
    NEED_CONFIG,
    CONNECTING,
    ONLINE,
    RECONNECTING,
    AUTH_FAILED,
}

object DisplayState {
    private val contentState = MutableStateFlow<ContentMessage?>(null)
    private val linkState = MutableStateFlow(LinkStatus.NEED_CONFIG)

    val content: StateFlow<ContentMessage?> = contentState
    val link: StateFlow<LinkStatus> = linkState

    fun updateContent(message: ContentMessage) {
        contentState.value = message
    }

    fun updateLink(status: LinkStatus) {
        linkState.value = status
    }
}
