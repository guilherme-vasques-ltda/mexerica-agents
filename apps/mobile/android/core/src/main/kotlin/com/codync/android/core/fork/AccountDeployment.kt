package com.codync.android.core.fork

import java.net.URI
import java.util.Base64

class AccountDeployment private constructor(val cloudUrl: String, val publishableKey: String, val tokenOrigin: String) {
    companion object {
        fun parse(cloudUrl: String, publishableKey: String, tokenOrigin: String): AccountDeployment? {
            if (https(cloudUrl) == null || !isOrigin(tokenOrigin)) return null
            val key = Regex("pk_(?:test|live)_([A-Za-z0-9+/]+={0,2})").matchEntire(publishableKey) ?: return null
            val frontend = runCatching { Base64.getDecoder().decode(key.groupValues[1]).toString(Charsets.UTF_8) }.getOrNull()
                ?: return null
            if (!frontend.endsWith('$') || !isOrigin("https://${frontend.dropLast(1)}")) return null
            return AccountDeployment(cloudUrl, publishableKey, tokenOrigin)
        }

        private fun isOrigin(value: String): Boolean = https(value)?.let { it.port == -1 && it.rawPath.isEmpty() } == true

        private fun https(value: String): URI? = runCatching { URI(value) }.getOrNull()?.takeIf {
            it.scheme == "https" && !it.host.isNullOrBlank() && it.rawUserInfo == null && it.rawQuery == null &&
                it.rawFragment == null && (it.port == -1 || it.port in 1..65535)
        }
    }
}
