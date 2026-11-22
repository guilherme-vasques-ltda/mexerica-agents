package com.codync.android.core.fork

import com.codync.android.core.text
import kotlinx.serialization.json.*

private const val ALLOW_ONCE = "allow_once"
private const val APPROVE_IN_APP = "Aprove no app"

internal class CallApproval(private val data: JsonObject) {
    private data class Option(val id: String, val name: String?, val kind: String?)

    private val voiceAllowed = data.string("voiceApproval") == ALLOW_ONCE

    fun status(): JsonObject = buildJsonObject {
        put("request", data.text("title") ?: data.text("command") ?: "")
        putJsonArray("options") {
            if (voiceAllowed) {
                val options = options()
                for (option in eligibleOptions(options)) {
                    val name = option.name
                    when {
                        !name.isNullOrBlank() && matchingOptions(options, name).singleOrNull() == option -> add(name)
                        matchingOptions(options, option.id).singleOrNull() == option -> add(option.id)
                    }
                }
            }
        }
        put("voiceApproval", if (voiceAllowed) ALLOW_ONCE else "app_only")
        put("note", if (voiceAllowed) "Pela voz, apenas permitir uma vez." else APPROVE_IN_APP)
    }

    fun answer(arguments: JsonObject, heard: Boolean, respond: (String) -> Unit): JsonObject {
        if (!voiceAllowed) return error(APPROVE_IN_APP)
        if (!heard) return error("Nothing was heard from the user yet.")
        val option = arguments.string("option").orEmpty()
        if (option.isBlank()) return error(APPROVE_IN_APP)
        val options = options()
        val matches = matchingOptions(options, option)
        val eligible = eligibleOptions(options)
        if (matches.isEmpty()) {
            val suggestions = eligible.filter { matchingOptions(options, it.id).size == 1 }
            if (suggestions.isEmpty()) return error(APPROVE_IN_APP)
            return buildJsonObject {
                put("error", "Opção não reconhecida")
                putJsonArray("options") {
                    suggestions.forEach { suggestion ->
                        addJsonObject {
                            put("name", suggestion.name ?: suggestion.id)
                            put("optionId", suggestion.id)
                            put("kind", ALLOW_ONCE)
                        }
                    }
                }
            }
        }
        val pick = matches.singleOrNull() ?: return error(APPROVE_IN_APP)
        if (pick !in eligible) return error(APPROVE_IN_APP)
        respond(pick.id)
        return buildJsonObject { put("answered", pick.name ?: pick.id) }
    }

    private fun options(): List<Option> = (data["options"] as? JsonArray).orEmpty().mapNotNull { option ->
        val fields = option as? JsonObject ?: return@mapNotNull null
        Option(fields.string("optionId").orEmpty(), fields.string("name"), fields.string("kind"))
    }

    private fun eligibleOptions(options: List<Option>): List<Option> {
        val idCounts = options.groupingBy { it.id }.eachCount()
        return options.filter { it.kind == ALLOW_ONCE && it.id.isNotBlank() && idCounts[it.id] == 1 }
    }

    private fun matchingOptions(options: List<Option>, option: String): List<Option> {
        val wanted = option.trim().lowercase()
        return options.filter { it.id == option || it.name?.trim()?.lowercase() == wanted }
    }

    private fun JsonObject.string(key: String): String? = (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content

    private fun error(message: String) = buildJsonObject { put("error", message) }
}
