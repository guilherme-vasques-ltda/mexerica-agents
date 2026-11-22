package com.codync.android.core.fork

import java.util.concurrent.atomic.AtomicInteger
import com.codync.android.core.Pcm
import com.codync.android.core.UtteranceDetector

/**
 * What a call may act on: only speech the user made in it, on its own microphone, while it listens.
 * The engine reports that speech when it hears it, and every piece of the computer's data handed to
 * the model (a bot's reply, a notice, any tool result) clears it, so nothing the data says can ask
 * for a new action.
 */
class CallEvidence {
    /** The call's microphone delivers audio on the current link: speech from here on belongs to this call. */
    var audioStarted = false
    /** The user spoke since the computer's data last reached the model. */
    var speechThisTurn = false
    /** The call is muted: nothing said while muted counts, and nothing said before survives it. */
    var muted = false
    /** The microphone stamps each frame with the current authority epoch. */
    val captureEpoch = AtomicInteger(0)
    /** Deliveries the provider hasn't confirmed yet: speech it reports meanwhile may predate them. */
    val unconfirmed = mutableSetOf<String>()

    // Pausing doesn't change the link: only an ACK or link teardown retires its pending deliveries.
    fun pause() {
        audioStarted = false
        speechThisTurn = false
        captureEpoch.incrementAndGet()
    }

    fun resetLink() {
        pause()
        unconfirmed.clear()
    }
}

/**
 * The capture epoch when the user's current speech began: the first voiced stretch after a full pause,
 * so a cough doesn't count as speech. Only that beginning is kept, so a pause in the middle of a
 * sentence never hands out a newer mark.
 */
class SpeechOnset {
    private var voiced = 0
    private var quiet = 0
    private var start: Int? = null
    private var reported = false

    /** Takes a microphone frame captured in epoch [at]; returns the epoch where speech began, once per utterance. */
    fun heard(pcm: ByteArray, level: Float, at: Int): Int? {
        if (level > UtteranceDetector.THRESHOLD) {
            if (voiced == 0) start = at
            voiced += pcm.size
            quiet = 0
            if (reported || voiced < VOICED) return null
            reported = true
            return start
        }
        if (voiced == 0) return null
        quiet += pcm.size
        if (quiet < PAUSE) return null
        reset()
        return null
    }

    /** The utterance is over: the next voiced stretch is measured as a new one. */
    fun reset() { voiced = 0; quiet = 0; start = null; reported = false }

    private companion object {
        /** A voiced stretch this long is speech; on-device calls already drop shorter ones. */
        const val VOICED = Pcm.RATE * 2 * 4 / 10
        /** Quiet this long ends the utterance; anything shorter is a word boundary. */
        const val PAUSE = Pcm.RATE * 2
    }
}
