package com.codync.android.fork

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.view.WindowManager
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.codync.android.core.fork.ForkMailInboxStore
import com.codync.android.core.fork.MailInboxError
import com.codync.android.core.fork.MailInboxException
import com.codync.android.core.fork.MailInboxListThreadsResponse
import com.codync.android.core.fork.MailInboxMailbox
import com.codync.android.core.fork.MailInboxMessage
import com.codync.android.core.fork.MailInboxOpenThreadResponse
import com.codync.android.core.fork.MailInboxStatus
import com.codync.android.core.fork.MailInboxThreadSummary
import com.codync.android.core.fork.MailInboxTransport
import com.codync.android.design.BackButton
import com.codync.android.design.CodyncTheme
import com.codync.android.design.IconButton
import com.codync.android.design.Palette
import com.codync.android.design.ScreenHeader
import com.codync.android.design.Symbol
import com.codync.android.design.Type
import com.codync.android.design.medium
import com.codync.android.design.monospacedDigit
import com.codync.android.design.semibold
import com.kyant.shapes.RoundedRectangle
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

private const val NEUTRAL_SUBJECT_FALLBACK = "Sem assunto"
private const val NEUTRAL_SENDER_FALLBACK = "Remetente não identificado"
private const val NEUTRAL_MAILBOX_LABEL = "Caixa"

data class ForkMailInboxMailboxItem(
    val mailboxId: String,
    val label: String,
)

data class ForkMailInboxThreadItem(
    val threadId: String,
    val subject: String,
    val lastMessageAt: String,
)

data class ForkMailInboxMessageItem(
    val messageId: String,
    val senderDisplayName: String,
    val declaredDomain: String?,
    val sentAt: String,
    val bodyText: String,
) {
    val isVerifiedIdentity: Boolean = false
}

sealed interface ForkMailInboxScreenState {
    data object Unavailable : ForkMailInboxScreenState
    data object NotConnected : ForkMailInboxScreenState
    data object ReconnectRequired : ForkMailInboxScreenState
    data object AccessDenied : ForkMailInboxScreenState
    data object Loading : ForkMailInboxScreenState
    data object NoMailboxes : ForkMailInboxScreenState
    data object EmptyInbox : ForkMailInboxScreenState

    data class ThreadList(
        val threads: List<ForkMailInboxThreadItem>,
        val hasNextPage: Boolean,
        val isLoadingMore: Boolean,
        val isLoadingDetail: Boolean,
    ) : ForkMailInboxScreenState

    data class ThreadDetail(
        val threadId: String,
        val subject: String,
        val messages: List<ForkMailInboxMessageItem>,
        val truncated: Boolean,
    ) : ForkMailInboxScreenState
}

data class ForkMailInboxUiState(
    val featureEnabled: Boolean,
    val showPrivacyMask: Boolean,
    val screenState: ForkMailInboxScreenState,
    val mailboxes: List<ForkMailInboxMailboxItem> = emptyList(),
    val selectedMailboxId: String? = null,
)

fun shouldShowPrivacyMask(
    shouldMaskInAppSwitcher: Boolean,
    isAppInBackground: Boolean,
): Boolean = shouldMaskInAppSwitcher && isAppInBackground

fun toForkMailInboxUiState(
    store: ForkMailInboxStore,
    isAppInBackground: Boolean = false,
    preferListOverDetail: Boolean = false,
): ForkMailInboxUiState {
    val showMask = shouldShowPrivacyMask(store.shouldMaskInAppSwitcher, isAppInBackground)
    val screenState = mapScreenState(store, preferListOverDetail)
    val mailboxes = when (screenState) {
        ForkMailInboxScreenState.EmptyInbox, is ForkMailInboxScreenState.ThreadList ->
            (store.status as? MailInboxStatus.Ready)?.mailboxes.orEmpty().distinctBy { it.mailboxId }.map {
                ForkMailInboxMailboxItem(it.mailboxId, it.label.ifBlank { NEUTRAL_MAILBOX_LABEL })
            }
        else -> emptyList()
    }
    return ForkMailInboxUiState(
        featureEnabled = store.featureEnabled,
        showPrivacyMask = showMask,
        screenState = screenState,
        mailboxes = mailboxes,
        selectedMailboxId = store.selectedMailboxId.takeIf { id -> mailboxes.any { it.mailboxId == id } },
    )
}

private fun mapScreenState(
    store: ForkMailInboxStore,
    preferListOverDetail: Boolean,
): ForkMailInboxScreenState {
    if (!store.featureEnabled) {
        return ForkMailInboxScreenState.Unavailable
    }
    if (store.isLoadingStatus) return ForkMailInboxScreenState.Loading
    if (store.lastError == MailInboxError.ACCESS_DENIED) {
        return ForkMailInboxScreenState.AccessDenied
    }
    if (store.status is MailInboxStatus.NotConnected || store.lastError == MailInboxError.NOT_CONNECTED) {
        return ForkMailInboxScreenState.NotConnected
    }
    if (store.status is MailInboxStatus.ReconnectRequired || store.lastError == MailInboxError.RECONNECT_REQUIRED) {
        return ForkMailInboxScreenState.ReconnectRequired
    }
    if (store.status is MailInboxStatus.Unavailable || store.lastError != null) {
        return ForkMailInboxScreenState.Unavailable
    }
    val readyStatus = store.status as? MailInboxStatus.Ready ?: return ForkMailInboxScreenState.Loading
    if (readyStatus.mailboxes.isEmpty()) return ForkMailInboxScreenState.NoMailboxes
    if (readyStatus.mailboxes.none { it.mailboxId == store.selectedMailboxId }) {
        return ForkMailInboxScreenState.Loading
    }

    val openThread = store.openThread
    if (openThread != null && !preferListOverDetail && !store.isLoadingThreadDetail) {
        return ForkMailInboxScreenState.ThreadDetail(
            threadId = openThread.threadId,
            subject = normalizeSubject(openThread.subject),
            messages = openThread.messages.distinctBy { it.messageId }.map { message ->
                val presentation = message.senderPresentation
                ForkMailInboxMessageItem(
                    messageId = message.messageId,
                    senderDisplayName = presentation.displayName
                        ?.takeIf { it.isNotBlank() }
                        ?: NEUTRAL_SENDER_FALLBACK,
                    declaredDomain = presentation.declaredDomain?.takeIf { it.isNotBlank() },
                    sentAt = message.sentAt,
                    bodyText = message.bodyText,
                )
            },
            truncated = openThread.truncated,
        )
    }

    if (!store.hasLoadedFirstPage || (store.isLoadingThreads && store.threads.isEmpty()) ||
        (store.isLoadingThreadDetail && store.threads.isEmpty())
    ) {
        return ForkMailInboxScreenState.Loading
    }

    if (store.isEmptyInbox) {
        return ForkMailInboxScreenState.EmptyInbox
    }

    return ForkMailInboxScreenState.ThreadList(
        threads = store.threads.distinctBy { it.threadId }.map { summary ->
            ForkMailInboxThreadItem(summary.threadId, normalizeSubject(summary.subject), summary.lastMessageAt)
        },
        hasNextPage = store.nextCursor != null,
        isLoadingMore = store.isLoadingThreads,
        isLoadingDetail = store.isLoadingThreadDetail,
    )
}

private fun normalizeSubject(subject: String?): String =
    subject?.takeIf { it.isNotBlank() } ?: NEUTRAL_SUBJECT_FALLBACK

internal class ForkMailInboxPresentation(private val store: ForkMailInboxStore, private val spaceId: String) {
    private val state = MutableStateFlow(ForkMailInboxUiState(
        store.featureEnabled,
        showPrivacyMask = false,
        screenState = if (store.featureEnabled) ForkMailInboxScreenState.Loading else ForkMailInboxScreenState.Unavailable,
    ))
    val uiState = state.asStateFlow()
    private var requesting = false
    private var preferListOverDetail = false
    private var isAppInBackground = true
    private var started = false

    suspend fun start() {
        if (started || !store.featureEnabled) return
        started = true
        request { store.loadStatus(spaceId) }
        val mailboxes = (store.status as? MailInboxStatus.Ready)?.mailboxes.orEmpty()
        val mailbox = mailboxes.firstOrNull { it.mailboxId == store.selectedMailboxId } ?: mailboxes.firstOrNull()
        if (mailbox != null && store.lastError == null) request { store.loadFirstPage(spaceId, mailbox.mailboxId) }
    }

    suspend fun selectMailbox(mailboxId: String) {
        if (state.value.mailboxes.none { it.mailboxId == mailboxId }) return
        request { store.loadFirstPage(spaceId, mailboxId) }
    }

    suspend fun openThread(threadId: String) {
        val list = state.value.screenState as? ForkMailInboxScreenState.ThreadList ?: return
        if (list.threads.none { it.threadId == threadId }) return
        val mailboxId = state.value.selectedMailboxId ?: return
        request { store.openThread(spaceId, mailboxId, threadId) }
    }

    suspend fun loadMore() {
        val list = state.value.screenState as? ForkMailInboxScreenState.ThreadList ?: return
        if (list.hasNextPage) request(clearBack = false) { store.loadNextPage() }
    }

    fun backToList() {
        preferListOverDetail = true
        publish()
    }

    fun setBackground(background: Boolean) {
        isAppInBackground = background
        if (started) publish()
    }

    private fun publish() {
        state.value = toForkMailInboxUiState(store, isAppInBackground, preferListOverDetail)
    }

    private suspend fun request(clearBack: Boolean = true, run: suspend () -> Unit) {
        if (requesting || !store.featureEnabled) return
        requesting = true
        if (clearBack) preferListOverDetail = false
        try {
            coroutineScope {
                val result = async(start = CoroutineStart.UNDISPATCHED) { run() }
                publish()
                result.await()
            }
        } finally {
            requesting = false
            publish()
        }
    }
}

@Composable
fun ForkMailInboxScreen(
    store: ForkMailInboxStore,
    spaceId: String,
    modifier: Modifier = Modifier,
    onClose: (() -> Unit)? = null,
    enableSecureWindowFlag: Boolean = true,
) {
    val lifecycleOwner = LocalLifecycleOwner.current
    if (enableSecureWindowFlag && store.featureEnabled) SecureWindowFlagEffect()
    key(store, spaceId) {
        val presentation = remember { ForkMailInboxPresentation(store, spaceId) }
        DisposableEffect(lifecycleOwner, presentation) {
            presentation.setBackground(!lifecycleOwner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED))
            val observer = LifecycleEventObserver { _, event ->
                when (event) {
                    Lifecycle.Event.ON_PAUSE, Lifecycle.Event.ON_STOP -> presentation.setBackground(true)
                    Lifecycle.Event.ON_RESUME -> presentation.setBackground(false)
                    else -> Unit
                }
            }
            lifecycleOwner.lifecycle.addObserver(observer)
            onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
        }
        LaunchedEffect(presentation) { presentation.start() }
        MailInboxContent(presentation, modifier, onClose)
    }
}

@Composable
private fun MailInboxContent(presentation: ForkMailInboxPresentation, modifier: Modifier, onClose: (() -> Unit)?) {
    val scope = rememberCoroutineScope()
    val uiState by presentation.uiState.collectAsState()

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(Palette.background),
    ) {
        if (uiState.showPrivacyMask) {
            PrivacyMaskOverlay()
            return@Box
        }
        Column(modifier = Modifier.fillMaxSize()) {
            val detailState = uiState.screenState as? ForkMailInboxScreenState.ThreadDetail
            ScreenHeader(
                leading = {
                    if (detailState != null) {
                        BackButton { presentation.backToList() }
                    } else if (onClose != null) {
                        IconButton("Fechar", "xmark", action = onClose)
                    }
                },
                title = {
                    Text(
                        text = if (detailState != null) "Conversa" else "Caixa de E-mail",
                        style = Type.body.semibold,
                        color = Palette.text,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
            )

            if (uiState.mailboxes.isNotEmpty()) {
                MailboxSelector(uiState) { mailboxId -> scope.launch { presentation.selectMailbox(mailboxId) } }
            }

            Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                when (val screen = uiState.screenState) {
                    ForkMailInboxScreenState.Unavailable -> StatusNotice(
                        symbolName = "exclamationmark.circle",
                        title = "Caixa de e-mail indisponível",
                        message = "A leitura de e-mail está desativada ou indisponível no momento.",
                    )
                    ForkMailInboxScreenState.NotConnected -> StatusNotice(
                        symbolName = "envelope.fill",
                        title = "Caixa não conectada",
                        message = "Nenhuma caixa de e-mail está vinculada a este workspace.",
                    )
                    ForkMailInboxScreenState.ReconnectRequired -> StatusNotice(
                        symbolName = "exclamationmark.triangle",
                        title = "Reconexão necessária",
                        message = "A autorização da caixa expirou e precisa ser reconectada.",
                    )
                    ForkMailInboxScreenState.AccessDenied -> StatusNotice(
                        symbolName = "exclamationmark.circle",
                        title = "Acesso negado",
                        message = "Você não tem permissão para visualizar este conteúdo.",
                    )
                    ForkMailInboxScreenState.Loading -> Box(
                        modifier = Modifier.fillMaxSize(),
                        contentAlignment = Alignment.Center,
                    ) {
                        CircularProgressIndicator(color = Palette.accent)
                    }
                    ForkMailInboxScreenState.EmptyInbox -> StatusNotice(
                        symbolName = "envelope.fill",
                        title = "Caixa vazia",
                        message = "Nenhuma conversa encontrada nesta caixa.",
                    )
                    ForkMailInboxScreenState.NoMailboxes -> StatusNotice(
                        symbolName = "envelope.fill",
                        title = "Nenhuma caixa disponível",
                        message = "Não há caixas para selecionar neste espaço.",
                    )
                    is ForkMailInboxScreenState.ThreadList -> ThreadListContent(
                        state = screen,
                        onOpenThread = { threadId -> scope.launch { presentation.openThread(threadId) } },
                        onLoadMore = { scope.launch { presentation.loadMore() } },
                    )
                    is ForkMailInboxScreenState.ThreadDetail -> ThreadDetailContent(
                        state = screen,
                    )
                }
            }
        }
    }
}

@Composable
private fun SecureWindowFlagEffect() {
    val context = LocalContext.current
    DisposableEffect(context) {
        val activity = context.findActivity()
        val window = activity?.window
        val hadSecureFlag = window != null &&
            (window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE) != 0
        if (window != null && !hadSecureFlag) {
            window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
        onDispose {
            if (window != null && !hadSecureFlag) {
                window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
            }
        }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}

@Composable
private fun PrivacyMaskOverlay() {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Palette.background)
            .semantics(mergeDescendants = true) {},
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.padding(24.dp),
        ) {
            Symbol("envelope.fill", size = 36.dp, tint = Palette.tertiary)
            Text(
                text = "Conteúdo protegido",
                style = Type.headline,
                color = Palette.secondary,
                textAlign = TextAlign.Center,
            )
        }
    }
}

@Composable
private fun StatusNotice(
    symbolName: String,
    title: String,
    message: String,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 64.dp, start = 24.dp, end = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Symbol(symbolName, size = 40.dp, tint = Palette.tertiary)
        Text(
            text = title,
            style = Type.title3.semibold,
            color = Palette.text,
            textAlign = TextAlign.Center,
        )
        Text(
            text = message,
            style = Type.subheadline,
            color = Palette.secondary,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun MailboxSelector(
    state: ForkMailInboxUiState,
    onSelectMailbox: (String) -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        for (mailbox in state.mailboxes) {
            FilterChip(
                selected = mailbox.mailboxId == state.selectedMailboxId,
                onClick = { onSelectMailbox(mailbox.mailboxId) },
                label = { Text(mailbox.label, style = Type.footnote.medium, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                colors = FilterChipDefaults.filterChipColors(
                    selectedContainerColor = Palette.accentFill,
                    selectedLabelColor = Palette.onAccent,
                    containerColor = Palette.surface,
                    labelColor = Palette.text,
                ),
            )
        }
    }
}

@Composable
private fun ThreadListContent(
    state: ForkMailInboxScreenState.ThreadList,
    onOpenThread: (String) -> Unit,
    onLoadMore: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxSize()) {
        if (state.isLoadingDetail) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 8.dp),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator(color = Palette.accent)
            }
        }

        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            items(state.threads, key = { "thread:${it.threadId}" }) { thread ->
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clip(RoundedRectangle(16.dp))
                        .background(Palette.surface)
                        .border(1.dp, Palette.border, RoundedRectangle(16.dp))
                        .clickable { onOpenThread(thread.threadId) }
                        .padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = thread.subject,
                        style = Type.body.semibold,
                        color = Palette.text,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = thread.lastMessageAt,
                        style = Type.caption.monospacedDigit,
                        color = Palette.secondary,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }

            if (state.hasNextPage) {
                item(key = "load-more") {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(top = 4.dp, bottom = 12.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        if (state.isLoadingMore) {
                            CircularProgressIndicator(color = Palette.accent)
                        } else {
                            OutlinedButton(onClick = onLoadMore) {
                                Text(
                                    text = "Carregar mais",
                                    style = Type.subheadline.medium,
                                    color = Palette.text,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ThreadDetailContent(
    state: ForkMailInboxScreenState.ThreadDetail,
) {
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "subject-header") {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    text = state.subject,
                    style = Type.title3.semibold,
                    color = Palette.text,
                )
                if (state.truncated) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedRectangle(12.dp))
                            .background(Palette.surface)
                            .border(1.dp, Palette.warning, RoundedRectangle(12.dp))
                            .padding(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Symbol("exclamationmark.triangle", size = 18.dp, tint = Palette.attention)
                        Text(
                            text = "Conteúdo truncado pelo servidor. Parte do histórico pode não estar visível.",
                            style = Type.footnote,
                            color = Palette.text,
                        )
                    }
                }
            }
        }

        items(state.messages, key = { "message:${it.messageId}" }) { message ->
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(RoundedRectangle(16.dp))
                    .background(Palette.surface)
                    .border(1.dp, Palette.border, RoundedRectangle(16.dp))
                    .padding(14.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Text(
                    text = message.senderDisplayName,
                    style = Type.subheadline.semibold,
                    color = Palette.text,
                )
                if (message.declaredDomain != null) {
                    Box(
                        modifier = Modifier
                            .clip(RoundedRectangle(8.dp))
                            .background(Palette.background)
                            .border(1.dp, Palette.border, RoundedRectangle(8.dp))
                            .padding(horizontal = 8.dp, vertical = 4.dp),
                    ) {
                        Text(
                            text = "Domínio declarado: ${message.declaredDomain}",
                            style = Type.caption,
                            color = Palette.secondary,
                        )
                    }
                }
                Text(
                    text = message.sentAt,
                    style = Type.caption.monospacedDigit,
                    color = Palette.tertiary,
                )
                Spacer(modifier = Modifier.height(2.dp))
                Text(
                    text = message.bodyText,
                    style = Type.subheadline,
                    color = Palette.text,
                )
            }
        }
    }
}

class ForkMailInboxFixtureTransport(
    private val statusOverrideBySpace: Map<String, MailInboxStatus> = emptyMap(),
) : MailInboxTransport {
    companion object {
        const val SPACE_ALPHA = "space-alpha-fixture"
        const val SPACE_BETA = "space-beta-fixture"
        const val MAILBOX_EMPTY = "mailbox-empty-fixture"
        const val MAILBOX_MAIN = "mailbox-main-fixture"
        const val MAILBOX_BETA = "mailbox-beta-fixture"
        const val THREAD_WELCOME = "thread-welcome-fixture"
        const val THREAD_TRUNCATED = "thread-truncated-fixture"
        const val THREAD_OLDER = "thread-older-fixture"
        const val CURSOR_PAGE_2 = "cursor-page-2-fixture"
    }

    override suspend fun mailInboxStatus(spaceId: String): MailInboxStatus {
        statusOverrideBySpace[spaceId]?.let { return it }
        return when (spaceId) {
            SPACE_ALPHA -> MailInboxStatus.Ready(
                listOf(
                    MailInboxMailbox(MAILBOX_MAIN, "Caixa Principal"),
                    MailInboxMailbox(MAILBOX_EMPTY, "Caixa Vazia"),
                ),
            )
            SPACE_BETA -> MailInboxStatus.Ready(
                listOf(
                    MailInboxMailbox(MAILBOX_BETA, "Workspace Beta"),
                ),
            )
            else -> throw MailInboxException(MailInboxError.ACCESS_DENIED)
        }
    }

    override suspend fun mailInboxListThreads(
        spaceId: String,
        mailboxId: String,
        cursor: String?,
    ): MailInboxListThreadsResponse {
        val status = mailInboxStatus(spaceId)
        if (status !is MailInboxStatus.Ready) {
            throw MailInboxException(
                when (status) {
                    MailInboxStatus.NotConnected -> MailInboxError.NOT_CONNECTED
                    MailInboxStatus.ReconnectRequired -> MailInboxError.RECONNECT_REQUIRED
                    MailInboxStatus.Unavailable -> MailInboxError.UNAVAILABLE
                    is MailInboxStatus.Ready -> MailInboxError.UNAVAILABLE
                },
            )
        }
        if (status.mailboxes.none { it.mailboxId == mailboxId }) {
            throw MailInboxException(MailInboxError.ACCESS_DENIED)
        }

        return when (mailboxId) {
            MAILBOX_EMPTY, MAILBOX_BETA -> {
                if (cursor != null) throw MailInboxException(MailInboxError.CURSOR_INVALID)
                MailInboxListThreadsResponse(threads = emptyList(), nextCursor = null)
            }
            MAILBOX_MAIN -> when (cursor) {
                null -> MailInboxListThreadsResponse(
                    threads = listOf(
                        MailInboxThreadSummary(
                            threadId = THREAD_WELCOME,
                            subject = "Resumo semanal de operação",
                            lastMessageAt = "2026-11-23T12:30:00Z",
                        ),
                        MailInboxThreadSummary(
                            threadId = THREAD_TRUNCATED,
                            subject = null,
                            lastMessageAt = "2026-11-22T09:15:00Z",
                        ),
                    ),
                    nextCursor = CURSOR_PAGE_2,
                )
                CURSOR_PAGE_2 -> MailInboxListThreadsResponse(
                    threads = listOf(
                        MailInboxThreadSummary(
                            threadId = THREAD_OLDER,
                            subject = "Arquivo de referência antigo",
                            lastMessageAt = "2026-11-20T18:00:00Z",
                        ),
                    ),
                    nextCursor = null,
                )
                else -> throw MailInboxException(MailInboxError.CURSOR_INVALID)
            }
            else -> throw MailInboxException(MailInboxError.ACCESS_DENIED)
        }
    }

    override suspend fun mailInboxOpenThread(
        spaceId: String,
        mailboxId: String,
        threadId: String,
    ): MailInboxOpenThreadResponse {
        val status = mailInboxStatus(spaceId)
        if (status !is MailInboxStatus.Ready || status.mailboxes.none { it.mailboxId == mailboxId }) {
            throw MailInboxException(MailInboxError.ACCESS_DENIED)
        }
        if (mailboxId != MAILBOX_MAIN) {
            throw MailInboxException(MailInboxError.ACCESS_DENIED)
        }

        return when (threadId) {
            THREAD_WELCOME -> MailInboxOpenThreadResponse(
                threadId = THREAD_WELCOME,
                subject = "Resumo semanal de operação",
                messages = listOf(
                    MailInboxMessage(
                        messageId = "msg-1-fixture",
                        fromDisplay = "Equipe Norte",
                        fromDomain = "norte.example.invalid",
                        sentAt = "2026-11-23T12:00:00Z",
                        bodyText = "Olá, segue o status consolidado da semana. Referência em https://portal.example.invalid/relatorio.",
                    ),
                    MailInboxMessage(
                        messageId = "msg-2-fixture",
                        fromDisplay = "Suporte Sul",
                        fromDomain = "sul.example.invalid",
                        sentAt = "2026-11-23T12:30:00Z",
                        bodyText = "Confirmado o recebimento sem pendências.",
                    ),
                ),
                truncated = false,
            )
            THREAD_TRUNCATED -> MailInboxOpenThreadResponse(
                threadId = THREAD_TRUNCATED,
                subject = null,
                messages = listOf(
                    MailInboxMessage(
                        messageId = "msg-3-fixture",
                        fromDisplay = null,
                        fromDomain = null,
                        sentAt = "2026-11-22T09:00:00Z",
                        bodyText = "Primeira mensagem sem identificação completa.",
                    ),
                    MailInboxMessage(
                        messageId = "msg-4-fixture",
                        fromDisplay = "Auditoria Leste",
                        fromDomain = "leste.example.invalid",
                        sentAt = "2026-11-22T09:15:00Z",
                        bodyText = "Mensagem mais recente do fio truncado.",
                    ),
                ),
                truncated = true,
            )
            THREAD_OLDER -> MailInboxOpenThreadResponse(
                threadId = THREAD_OLDER,
                subject = "Arquivo de referência antigo",
                messages = listOf(
                    MailInboxMessage(
                        messageId = "msg-5-fixture",
                        fromDisplay = "Arquivo Oeste",
                        fromDomain = "oeste.example.invalid",
                        sentAt = "2026-11-20T18:00:00Z",
                        bodyText = "Registro arquivado apenas para leitura.",
                    ),
                ),
                truncated = false,
            )
            else -> throw MailInboxException(MailInboxError.ACCESS_DENIED)
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun ForkMailInboxScreenDisabledPreview() {
    val store = remember {
        ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(),
            featureEnabled = false,
        )
    }
    CodyncTheme {
        ForkMailInboxScreen(
            store = store,
            spaceId = ForkMailInboxFixtureTransport.SPACE_ALPHA,
            enableSecureWindowFlag = false,
        )
    }
}

@Preview(showBackground = true)
@Composable
private fun ForkMailInboxScreenFixturePreview() {
    val store = remember {
        ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(),
            featureEnabled = true,
        )
    }
    CodyncTheme {
        ForkMailInboxScreen(
            store = store,
            spaceId = ForkMailInboxFixtureTransport.SPACE_ALPHA,
            enableSecureWindowFlag = false,
        )
    }
}
