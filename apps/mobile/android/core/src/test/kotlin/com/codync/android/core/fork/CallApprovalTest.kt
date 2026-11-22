package com.codync.android.core.fork

import com.codync.android.core.Bot
import com.codync.android.core.CallOperator
import com.codync.android.core.CallTarget
import com.codync.android.core.Entry
import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

class CallApprovalTest {
    private class Desk(override val entries: List<Entry>) : CallTarget {
        override val bots = listOf(Bot("test-bot", "Test Bot"))
        val responses = mutableListOf<Pair<Entry, String>>()

        override fun send(text: String): Unit = error("Unexpected message: $text")
        override fun respond(entry: Entry, optionId: String) { responses += entry to optionId }
    }

    private fun choice(id: String?, name: String, kind: String? = "allow_once") = buildJsonObject {
        id?.let { put("optionId", it) }
        put("name", name)
        kind?.let { put("kind", it) }
    }

    private fun card(policy: JsonElement?, vararg choices: JsonObject) = Entry(
        "approval", 3, "test-bot", 7, "permission", data = buildJsonObject {
            put("status", "pending")
            put("title", "Update test plan")
            policy?.let { put("voiceApproval", it) }
            put("options", JsonArray(choices.toList()))
        },
    )

    private fun status(operator: CallOperator) = operator.call("bot_status", buildJsonObject { })["approval"]!!.jsonObject

    private fun answer(operator: CallOperator, option: String) =
        operator.call("answer_approval", buildJsonObject { put("option", option) })

    private fun heardAnswer(operator: CallOperator, option: String): JsonObject {
        operator.audioStarted()
        operator.speechDetected()
        return answer(operator, option)
    }

    @Test fun missingUnknownAndAppOnlyPoliciesNeverExposeOptionsOrRespond() {
        for (policy in listOf(null, JsonNull, JsonPrimitive(""), JsonPrimitive("app_only"), JsonPrimitive("future"), JsonPrimitive(false), buildJsonObject { })) {
            val desk = Desk(listOf(card(policy, choice("once", "Allow once"))))
            val operator = CallOperator("test-bot", desk)
            val approval = status(operator)
            assertEquals(JsonPrimitive("app_only"), approval["voiceApproval"])
            assertEquals(JsonPrimitive("Aprove no app"), approval["note"])
            assertTrue(approval["options"]!!.jsonArray.isEmpty())
            assertEquals(buildJsonObject { put("error", "Aprove no app") }, heardAnswer(operator, "once"))
            assertTrue(desk.responses.isEmpty())
        }
    }

    @Test fun statusAndUnknownAnswersExcludeIneligibleOptionsIncludingCrossKindDuplicateIds() {
        val desk = Desk(listOf(card(JsonPrimitive("allow_once"),
            choice("once", "Allow once"),
            choice("always", "Always allow", "allow_always"),
            choice("reject", "Reject", "reject_once"),
            choice("never", "Always reject", "reject_always"),
            choice("untyped", "Missing kind", null),
            choice("future", "Unknown kind", "future"),
            choice("duplicate", "First duplicate"),
            choice("duplicate", "Second duplicate", "reject_always"),
            choice("double", "First double"),
            choice("double", "Second double"),
            choice("  ", "Blank ID"),
            choice(null, "Missing ID"),
        )))
        val operator = CallOperator("test-bot", desk)
        val approval = status(operator)
        assertEquals(JsonPrimitive("Update test plan"), approval["request"])
        assertEquals(JsonPrimitive("allow_once"), approval["voiceApproval"])
        assertEquals(JsonPrimitive("Pela voz, apenas permitir uma vez."), approval["note"])
        assertEquals(JsonArray(listOf(JsonPrimitive("Allow once"))), approval["options"])

        val unknown = heardAnswer(operator, "please allow once now")
        assertEquals(JsonPrimitive("Opção não reconhecida"), unknown["error"])
        assertEquals(JsonArray(listOf(choice("once", "Allow once"))), unknown["options"])
        for (option in listOf("always", "Reject", "never", "Missing kind", "future", "First duplicate", "duplicate", "First double", "Blank ID", "Missing ID", " ")) {
            assertEquals(option, buildJsonObject { put("error", "Aprove no app") }, heardAnswer(operator, option))
        }
        assertTrue(desk.responses.isEmpty())
    }

    @Test fun approvalMatchesExactOpaqueIdOrNormalizedNameAndRequiresFreshSpeech() {
        val approval = card(JsonPrimitive("allow_once"), choice("Once_V1", "Allow once"))
        val desk = Desk(listOf(approval))
        val operator = CallOperator("test-bot", desk)

        assertEquals(JsonPrimitive("Nothing was heard from the user yet."), answer(operator, "Once_V1")["error"])
        for (option in listOf("once_v1", " Once_V1 ", "allow")) {
            assertEquals(JsonPrimitive("Opção não reconhecida"), heardAnswer(operator, option)["error"])
        }
        assertTrue(desk.responses.isEmpty())
        assertEquals(JsonPrimitive("Allow once"), heardAnswer(operator, "Once_V1")["answered"])
        assertEquals(JsonPrimitive("Nothing was heard from the user yet."), answer(operator, "Once_V1")["error"])
        assertEquals(JsonPrimitive("Allow once"), heardAnswer(operator, "  ALLOW ONCE  ")["answered"])
        assertEquals(List(2) { approval to "Once_V1" }, desk.responses)
    }

    @Test fun ambiguousNamesAndNameIdCollisionsConsiderAllOptionsBeforeEligibility() {
        val desk = Desk(listOf(card(JsonPrimitive("allow_once"),
            choice("collision", "Shared name"),
            choice("blocked", " COLLISION ", "reject_once"),
            choice(null, "shared name", "reject_once"),
        )))
        val operator = CallOperator("test-bot", desk)
        assertTrue(status(operator)["options"]!!.jsonArray.isEmpty())
        for (option in listOf("collision", " shared NAME ", "unrecognized")) {
            assertEquals(option, buildJsonObject { put("error", "Aprove no app") }, heardAnswer(operator, option))
        }
        assertTrue(desk.responses.isEmpty())
    }

    @Test fun statusUsesExactIdWhenTheOptionNameIsAmbiguous() {
        val approval = card(JsonPrimitive("allow_once"),
            choice("Once_V1", "Allow once"),
            choice("blocked", " ALLOW ONCE ", "reject_once"),
        )
        val desk = Desk(listOf(approval))
        val operator = CallOperator("test-bot", desk)

        assertEquals(JsonArray(listOf(JsonPrimitive("Once_V1"))), status(operator)["options"])
        assertEquals(JsonPrimitive("Allow once"), heardAnswer(operator, "Once_V1")["answered"])
        assertEquals(listOf(approval to "Once_V1"), desk.responses)
    }
}
