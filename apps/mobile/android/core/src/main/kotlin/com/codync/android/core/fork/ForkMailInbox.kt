package com.codync.android.core.fork

import java.net.IDN
import java.util.Locale
import kotlinx.coroutines.CancellationException

enum class MailInboxError(val code: String) {
    INVALID_REQUEST("invalid_request"),
    ACCESS_DENIED("access_denied"),
    NOT_CONNECTED("not_connected"),
    RECONNECT_REQUIRED("reconnect_required"),
    UNAVAILABLE("unavailable"),
    CURSOR_INVALID("cursor_invalid"),
}

class MailInboxException(val error: MailInboxError) : Exception(error.code)

data class MailInboxMailbox(
    val mailboxId: String,
    val label: String,
) {
    override fun toString(): String = "MailInboxMailbox(redacted)"
}

sealed interface MailInboxStatus {
    data class Ready(val mailboxes: List<MailInboxMailbox>) : MailInboxStatus {
        override fun toString(): String = "MailInboxStatus.Ready(count=${mailboxes.size})"
    }

    data object NotConnected : MailInboxStatus {
        override fun toString(): String = "MailInboxStatus.not_connected"
    }

    data object ReconnectRequired : MailInboxStatus {
        override fun toString(): String = "MailInboxStatus.reconnect_required"
    }

    data object Unavailable : MailInboxStatus {
        override fun toString(): String = "MailInboxStatus.unavailable"
    }
}

data class MailInboxThreadSummary(
    val threadId: String,
    val subject: String?,
    val lastMessageAt: String,
) {
    override fun toString(): String = "MailInboxThreadSummary(redacted)"
}

data class MailInboxListThreadsResponse(
    val threads: List<MailInboxThreadSummary>,
    val nextCursor: String?,
) {
    override fun toString(): String =
        "MailInboxListThreadsResponse(count=${threads.size}, hasNext=${nextCursor != null})"
}

data class MailInboxSenderPresentation(
    val displayName: String?,
    val declaredDomain: String?,
) {
    val isVerifiedIdentity: Boolean = false
    val hasDeclaredDomain: Boolean get() = declaredDomain != null

    override fun toString(): String =
        "MailInboxSenderPresentation(hasDisplay=${displayName != null}, hasDomain=${declaredDomain != null}, verified=$isVerifiedIdentity)"
}

data class MailInboxMessage(
    val messageId: String,
    val fromDisplay: String?,
    val fromDomain: String?,
    val sentAt: String,
    val bodyText: String,
) {
    val senderPresentation: MailInboxSenderPresentation
        get() = MailInboxSenderPresentation(
            displayName = fromDisplay,
            declaredDomain = fromDomain,
        )

    override fun toString(): String = "MailInboxMessage(redacted)"
}

data class MailInboxOpenThreadResponse(
    val threadId: String,
    val subject: String?,
    val messages: List<MailInboxMessage>,
    val truncated: Boolean,
) {
    override fun toString(): String =
        "MailInboxOpenThreadResponse(messageCount=${messages.size}, truncated=$truncated)"
}

object MailInboxSanitizer {
    private fun isBidiControl(codePoint: Int): Boolean =
        codePoint in 0x202A..0x202E ||
            codePoint in 0x2066..0x2069 ||
            codePoint == 0x200E ||
            codePoint == 0x200F ||
            codePoint == 0x061C

    private fun isTagCharacter(codePoint: Int): Boolean =
        codePoint in 0xE0000..0xE007F

    private fun isLineOrParagraphSeparator(codePoint: Int): Boolean =
        codePoint == 0x2028 || codePoint == 0x2029

    private fun isNonTextualControl(codePoint: Int, allowLineBreaksAndTabs: Boolean): Boolean {
        if (allowLineBreaksAndTabs && (codePoint == 0x000A || codePoint == 0x0009)) {
            return false
        }
        return codePoint in 0x0000..0x001F || codePoint in 0x007F..0x009F
    }

    fun sanitizeSingleLine(raw: String?): String? {
        if (raw == null) return null
        val sb = StringBuilder(raw.length)
        var i = 0
        while (i < raw.length) {
            val cp = raw.codePointAt(i)
            val step = Character.charCount(cp)
            val isFormat = Character.getType(cp) == Character.FORMAT.toInt()
            if (!isFormat &&
                !isTagCharacter(cp) &&
                !isLineOrParagraphSeparator(cp) &&
                !isNonTextualControl(cp, allowLineBreaksAndTabs = false)
            ) {
                sb.appendCodePoint(cp)
            }
            i += step
        }
        return sb.toString()
    }

    fun sanitizeDomain(raw: String?): String? {
        val cleaned = sanitizeSingleLine(raw)?.trim()?.lowercase(Locale.ROOT) ?: return null
        if (cleaned.isEmpty()) return null
        val ascii = try {
            IDN.toASCII(cleaned, IDN.USE_STD3_ASCII_RULES).lowercase(Locale.ROOT)
        } catch (_: IllegalArgumentException) {
            return null
        }
        if (ascii.isEmpty()) return null
        val validChars = ascii.all { ch ->
            ch in 'a'..'z' || ch in '0'..'9' || ch == '.' || ch == '-'
        }
        if (!validChars ||
            ascii.startsWith('.') ||
            ascii.endsWith('.') ||
            ascii.startsWith('-') ||
            ascii.endsWith('-') ||
            ascii.contains("..")
        ) {
            return null
        }
        return ascii
    }

    fun sanitizeBodyText(raw: String): String {
        val normalized = raw
            .replace("\r\n", "\n")
            .replace('\r', '\n')
            .replace('\u2028', '\n')
            .replace('\u2029', '\n')
        val sb = StringBuilder(normalized.length)
        var i = 0
        while (i < normalized.length) {
            val cp = normalized.codePointAt(i)
            val step = Character.charCount(cp)
            if (!isBidiControl(cp) &&
                !isTagCharacter(cp) &&
                cp != 0x200B &&
                cp != 0xFEFF &&
                !isNonTextualControl(cp, allowLineBreaksAndTabs = true)
            ) {
                sb.appendCodePoint(cp)
            }
            i += step
        }
        return sb.toString()
    }

    fun sanitizeThreadSummary(item: MailInboxThreadSummary): MailInboxThreadSummary =
        item.copy(subject = sanitizeSingleLine(item.subject))

    fun sanitizeMessage(item: MailInboxMessage): MailInboxMessage =
        item.copy(
            fromDisplay = sanitizeSingleLine(item.fromDisplay),
            fromDomain = sanitizeDomain(item.fromDomain),
            bodyText = sanitizeBodyText(item.bodyText),
        )

    fun sanitizeOpenThread(response: MailInboxOpenThreadResponse): MailInboxOpenThreadResponse =
        response.copy(
            subject = sanitizeSingleLine(response.subject),
            messages = response.messages.map(::sanitizeMessage),
        )
}

interface MailInboxTransport {
    suspend fun mailInboxStatus(spaceId: String): MailInboxStatus
    suspend fun mailInboxListThreads(
        spaceId: String,
        mailboxId: String,
        cursor: String?,
    ): MailInboxListThreadsResponse

    suspend fun mailInboxOpenThread(
        spaceId: String,
        mailboxId: String,
        threadId: String,
    ): MailInboxOpenThreadResponse
}

class ForkMailInboxStore(
    private val transport: MailInboxTransport,
    val featureEnabled: Boolean = false,
) {
    var generation: Long = 0L
        private set
    var activeSpaceId: String? = null
        private set
    var selectedMailboxId: String? = null
        private set
    var status: MailInboxStatus? = null
        private set
    var isLoadingStatus: Boolean = false
        private set
    var isLoadingThreads: Boolean = false
        private set
    var isLoadingThreadDetail: Boolean = false
        private set
    var threads: List<MailInboxThreadSummary> = emptyList()
        private set
    var nextCursor: String? = null
        private set
    var hasLoadedFirstPage: Boolean = false
        private set
    var openThread: MailInboxOpenThreadResponse? = null
        private set
    var lastError: MailInboxError? = null
        private set

    val isEmptyInbox: Boolean
        get() = lastError == null &&
            !isLoadingStatus &&
            !isLoadingThreads &&
            status is MailInboxStatus.Ready &&
            hasLoadedFirstPage &&
            threads.isEmpty() &&
            nextCursor == null

    val shouldMaskInAppSwitcher: Boolean
        get() = threads.isNotEmpty() ||
            openThread != null ||
            (status as? MailInboxStatus.Ready)?.mailboxes?.isNotEmpty() == true

    override fun toString(): String =
        "ForkMailInboxStore(generation=$generation, featureEnabled=$featureEnabled, threadCount=${threads.size}, hasOpenThread=${openThread != null}, masked=$shouldMaskInAppSwitcher)"

    fun resetForContextChange() {
        generation += 1L
        activeSpaceId = null
        selectedMailboxId = null
        status = null
        isLoadingStatus = false
        isLoadingThreads = false
        isLoadingThreadDetail = false
        threads = emptyList()
        nextCursor = null
        hasLoadedFirstPage = false
        openThread = null
        lastError = null
    }

    suspend fun loadStatus(spaceId: String) {
        if (!featureEnabled) {
            status = MailInboxStatus.Unavailable
            lastError = MailInboxError.UNAVAILABLE
            return
        }
        val cleanSpaceId = nonEmpty(spaceId)
        if (cleanSpaceId == null) {
            lastError = MailInboxError.INVALID_REQUEST
            return
        }

        if (activeSpaceId != cleanSpaceId) {
            resetForContextChange()
            activeSpaceId = cleanSpaceId
        }

        val requestGeneration = generation
        isLoadingStatus = true
        lastError = null

        try {
            val result = transport.mailInboxStatus(cleanSpaceId)
            if (generation != requestGeneration || activeSpaceId != cleanSpaceId) return
            isLoadingStatus = false
            when (result) {
                is MailInboxStatus.Ready -> {
                    val sanitized = result.mailboxes.map {
                        MailInboxMailbox(
                            mailboxId = it.mailboxId,
                            label = MailInboxSanitizer.sanitizeSingleLine(it.label).orEmpty(),
                        )
                    }
                    status = MailInboxStatus.Ready(sanitized)
                    lastError = null
                }
                MailInboxStatus.NotConnected -> {
                    status = MailInboxStatus.NotConnected
                    lastError = MailInboxError.NOT_CONNECTED
                }
                MailInboxStatus.ReconnectRequired -> {
                    status = MailInboxStatus.ReconnectRequired
                    lastError = MailInboxError.RECONNECT_REQUIRED
                }
                MailInboxStatus.Unavailable -> {
                    status = MailInboxStatus.Unavailable
                    lastError = MailInboxError.UNAVAILABLE
                }
            }
        } catch (t: Throwable) {
            if (t is CancellationException) throw t
            if (generation != requestGeneration || activeSpaceId != cleanSpaceId) return
            isLoadingStatus = false
            val mapped = mapError(t)
            if (mapped == MailInboxError.UNAVAILABLE ||
                mapped == MailInboxError.NOT_CONNECTED ||
                mapped == MailInboxError.RECONNECT_REQUIRED
            ) {
                status = statusFromError(mapped)
            }
            lastError = mapped
        }
    }

    suspend fun loadFirstPage(spaceId: String, mailboxId: String) {
        if (!featureEnabled) {
            status = MailInboxStatus.Unavailable
            lastError = MailInboxError.UNAVAILABLE
            return
        }
        val cleanSpaceId = nonEmpty(spaceId)
        val cleanMailboxId = nonEmpty(mailboxId)
        if (cleanSpaceId == null || cleanMailboxId == null) {
            lastError = MailInboxError.INVALID_REQUEST
            return
        }

        if (activeSpaceId != cleanSpaceId) {
            resetForContextChange()
            activeSpaceId = cleanSpaceId
        } else {
            generation += 1L
        }

        selectedMailboxId = cleanMailboxId
        threads = emptyList()
        nextCursor = null
        hasLoadedFirstPage = false
        openThread = null
        lastError = null

        fetchPage(
            spaceId = cleanSpaceId,
            mailboxId = cleanMailboxId,
            cursor = null,
            isRestartFromCursorInvalid = false,
        )
    }

    suspend fun loadNextPage() {
        if (!featureEnabled) {
            status = MailInboxStatus.Unavailable
            lastError = MailInboxError.UNAVAILABLE
            return
        }
        if (isLoadingThreads) return
        val spaceId = activeSpaceId ?: return
        val mailboxId = selectedMailboxId ?: return
        val cursor = nextCursor ?: return
        fetchPage(
            spaceId = spaceId,
            mailboxId = mailboxId,
            cursor = cursor,
            isRestartFromCursorInvalid = false,
        )
    }

    suspend fun openThread(spaceId: String, mailboxId: String, threadId: String) {
        if (!featureEnabled) {
            status = MailInboxStatus.Unavailable
            lastError = MailInboxError.UNAVAILABLE
            return
        }
        val cleanSpaceId = nonEmpty(spaceId)
        val cleanMailboxId = nonEmpty(mailboxId)
        val cleanThreadId = nonEmpty(threadId)
        if (cleanSpaceId == null || cleanMailboxId == null || cleanThreadId == null) {
            lastError = MailInboxError.INVALID_REQUEST
            return
        }

        if (activeSpaceId != cleanSpaceId) {
            resetForContextChange()
            activeSpaceId = cleanSpaceId
            selectedMailboxId = cleanMailboxId
        } else if (selectedMailboxId == null) {
            selectedMailboxId = cleanMailboxId
        }

        val requestGeneration = generation
        isLoadingThreadDetail = true
        lastError = null

        try {
            val response = transport.mailInboxOpenThread(cleanSpaceId, cleanMailboxId, cleanThreadId)
            if (generation != requestGeneration ||
                activeSpaceId != cleanSpaceId ||
                selectedMailboxId != cleanMailboxId
            ) {
                return
            }
            isLoadingThreadDetail = false
            openThread = MailInboxSanitizer.sanitizeOpenThread(response)
            lastError = null
        } catch (t: Throwable) {
            if (t is CancellationException) throw t
            if (generation != requestGeneration ||
                activeSpaceId != cleanSpaceId ||
                selectedMailboxId != cleanMailboxId
            ) {
                return
            }
            isLoadingThreadDetail = false
            lastError = mapError(t)
        }
    }

    private suspend fun fetchPage(
        spaceId: String,
        mailboxId: String,
        cursor: String?,
        isRestartFromCursorInvalid: Boolean,
    ) {
        val requestGeneration = generation
        isLoadingThreads = true
        lastError = null

        try {
            val page = transport.mailInboxListThreads(spaceId, mailboxId, cursor)
            if (generation != requestGeneration ||
                activeSpaceId != spaceId ||
                selectedMailboxId != mailboxId
            ) {
                return
            }
            isLoadingThreads = false
            val sanitizedThreads = page.threads.map(MailInboxSanitizer::sanitizeThreadSummary)
            if (cursor == null) {
                threads = sanitizedThreads
                hasLoadedFirstPage = true
            } else {
                threads = threads + sanitizedThreads
            }
            nextCursor = page.nextCursor
            lastError = null
        } catch (t: Throwable) {
            if (t is CancellationException) throw t
            if (generation != requestGeneration ||
                activeSpaceId != spaceId ||
                selectedMailboxId != mailboxId
            ) {
                return
            }
            isLoadingThreads = false
            val mapped = mapError(t)
            if (mapped == MailInboxError.CURSOR_INVALID && cursor != null && !isRestartFromCursorInvalid) {
                threads = emptyList()
                nextCursor = null
                hasLoadedFirstPage = false
                fetchPage(
                    spaceId = spaceId,
                    mailboxId = mailboxId,
                    cursor = null,
                    isRestartFromCursorInvalid = true,
                )
                return
            }
            lastError = mapped
        }
    }

    private fun nonEmpty(value: String): String? = value.trim().ifEmpty { null }

    private fun mapError(t: Throwable): MailInboxError =
        (t as? MailInboxException)?.error ?: MailInboxError.UNAVAILABLE

    private fun statusFromError(error: MailInboxError): MailInboxStatus = when (error) {
        MailInboxError.NOT_CONNECTED -> MailInboxStatus.NotConnected
        MailInboxError.RECONNECT_REQUIRED -> MailInboxStatus.ReconnectRequired
        else -> MailInboxStatus.Unavailable
    }
}
