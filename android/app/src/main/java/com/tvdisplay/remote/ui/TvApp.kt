package com.tvdisplay.remote.ui

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.tv.material3.Button
import androidx.tv.material3.ExperimentalTvMaterial3Api
import androidx.tv.material3.Surface
import com.tvdisplay.remote.R
import com.tvdisplay.remote.logic.DisplayState
import com.tvdisplay.remote.logic.LinkStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

private val Ink = Color(0xFFF4F7F2)
private val Muted = Color(0xFFB7C0B8)
private val Green = Color(0xFF3DDC97)
private val Field = Color(0xFF1C2420)

@OptIn(ExperimentalTvMaterial3Api::class)
@Composable
fun TvDisplayApp(
    initialUrl: String,
    initialPassword: String,
    onSave: (String, String) -> String,
) {
    var editing by remember { mutableStateOf(initialUrl.isBlank() || initialPassword.isBlank()) }
    androidx.tv.material3.MaterialTheme {
        MaterialTheme(colorScheme = darkColorScheme(primary = Green, onPrimary = Color(0xFF062117), background = Color(0xFF101311), surface = Field)) {
            Surface(modifier = Modifier.fillMaxSize()) {
                if (editing) {
                    SettingsScreen(
                        initialUrl = initialUrl,
                        initialPassword = initialPassword,
                        onSave = { url, password ->
                            onSave(url, password)
                            editing = false
                        },
                    )
                } else {
                    DisplayScreen(onEdit = { editing = true })
                }
            }
        }
    }
}

@Composable
private fun SettingsScreen(
    initialUrl: String,
    initialPassword: String,
    onSave: (String, String) -> Unit,
) {
    var url by remember { mutableStateOf(initialUrl) }
    var password by remember { mutableStateOf(initialPassword) }
    var error by remember { mutableStateOf<String?>(null) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) {
        focus.requestFocus()
    }
    Column(
        modifier = Modifier.fillMaxSize().padding(horizontal = 72.dp, vertical = 48.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Text(stringResource(R.string.app_name), color = Green, fontSize = 22.sp)
        Spacer(Modifier.height(8.dp))
        Text(stringResource(R.string.settings_title), color = Ink, fontSize = 40.sp)
        Spacer(Modifier.height(28.dp))
        OutlinedTextField(
            value = url,
            onValueChange = { url = it },
            modifier = Modifier.fillMaxWidth().widthIn(max = 720.dp).focusRequester(focus),
            label = { Text(stringResource(R.string.server_url)) },
            placeholder = { Text(stringResource(R.string.server_url_hint)) },
            singleLine = true,
        )
        Spacer(Modifier.height(16.dp))
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            modifier = Modifier.fillMaxWidth().widthIn(max = 720.dp),
            label = { Text(stringResource(R.string.password)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
        )
        if (error != null) {
            Spacer(Modifier.height(12.dp))
            Text(error.orEmpty(), color = Color(0xFFFFB4AB), fontSize = 20.sp)
        }
        Spacer(Modifier.height(24.dp))
        Button(onClick = {
            try {
                if (password.isBlank()) {
                    error = "请填写管理密码"
                    return@Button
                }
                onSave(url, password)
            } catch (invalid: IllegalArgumentException) {
                error = invalid.message
            }
        }) {
            Text(stringResource(R.string.save), fontSize = 22.sp)
        }
    }
}

@Composable
private fun DisplayScreen(onEdit: () -> Unit) {
    val message by DisplayState.content.collectAsState()
    val link by DisplayState.link.collectAsState()
    val bitmap by produceState<Bitmap?>(initialValue = null, message?.revision, message?.imageBase64) {
        val encoded = message?.imageBase64
        value = withContext(Dispatchers.Default) { decodePoster(encoded) }
    }
    Box(modifier = Modifier.fillMaxSize().background(Color.Black)) {
        val current = bitmap
        if (current != null) {
            Image(
                bitmap = current.asImageBitmap(),
                contentDescription = stringResource(R.string.poster_description),
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Fit,
            )
        } else {
            Text(
                stringResource(R.string.empty_content),
                color = Muted,
                fontSize = 28.sp,
                modifier = Modifier.align(Alignment.Center).padding(48.dp),
            )
        }
        Text(
            linkLabel(link),
            color = Green,
            fontSize = 18.sp,
            modifier = Modifier.align(Alignment.TopStart).padding(28.dp),
        )
        val caption = message?.text.orEmpty()
        if (caption.isNotBlank()) {
            Text(
                caption,
                color = Ink,
                fontSize = 32.sp,
                modifier = Modifier
                    .align(Alignment.BottomStart)
                    .fillMaxWidth()
                    .background(Color(0xCC101311))
                    .padding(horizontal = 36.dp, vertical = 22.dp),
            )
        }
        Button(
            onClick = onEdit,
            modifier = Modifier.align(Alignment.BottomEnd).padding(28.dp),
        ) {
            Text(stringResource(R.string.open_settings), fontSize = 18.sp)
        }
    }
}

@Composable
private fun linkLabel(status: LinkStatus): String = when (status) {
    LinkStatus.ONLINE -> stringResource(R.string.status_online)
    LinkStatus.CONNECTING -> stringResource(R.string.status_connecting)
    LinkStatus.RECONNECTING -> stringResource(R.string.status_reconnecting)
    LinkStatus.AUTH_FAILED -> stringResource(R.string.status_auth)
    LinkStatus.NEED_CONFIG -> stringResource(R.string.status_need_config)
}
