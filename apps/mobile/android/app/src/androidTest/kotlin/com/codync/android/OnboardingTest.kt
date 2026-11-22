// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android

import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.lifecycle.viewmodel.compose.viewModel
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

/** Read-only: the store only receives taps that never pair, skip or sign in. */
class OnboardingTest {
    @get:Rule val compose = createComposeRule()

    @Test fun welcomeLeadsToInstallAndBackReturnsToWelcome() {
        compose.setContent { Hosted { CodyncApp(AppState(loading = false), viewModel()) } }
        compose.onNodeWithText("Your coding agents", substring = true).assertIsDisplayed()
        compose.onNodeWithText("Get started").performClick()
        compose.onNodeWithText("Open Mexerica", substring = true).assertIsDisplayed()
        compose.onNodeWithContentDescription("Step 1 of 2").assertIsDisplayed()
        compose.onNodeWithContentDescription("Back").performClick()
        compose.onNodeWithText("Get started").assertIsDisplayed()
    }

    @Test fun pairingExplainsTheComputerStepAndAllowsSkipping() {
        var skipped = false
        compose.setContent { Hosted { PairingView(AppState(loading = false), viewModel(), onSkip = { skipped = true }) } }
        compose.onNodeWithText("Open Mexerica on your computer and sign in with the same account", substring = true)
            .performScrollTo().assertIsDisplayed()
        compose.onNodeWithText("Source code").assertDoesNotExist()
        compose.onNodeWithText("Watch the demo").assertDoesNotExist()
        compose.onNodeWithText("Skip").assertDoesNotExist()
        compose.onNodeWithText("Continue").performClick()
        compose.onNodeWithText("Scan the pairing code").assertIsDisplayed()
        compose.onNodeWithContentDescription("Step 2 of 2").assertIsDisplayed()
        compose.onNodeWithText("Open Mexerica on your computer and show its pairing code.").assertIsDisplayed()
        compose.onNodeWithText("Or paste a ${BuildConfig.URL_SCHEME}://pair link").performScrollTo().assertIsDisplayed()
        compose.onNodeWithContentDescription("Pair").assertIsNotEnabled()
        compose.onNode(hasSetTextAction()).performTextInput("draft-link")
        compose.onNodeWithContentDescription("Pair").assertIsEnabled()
        compose.onNodeWithContentDescription("Back").performClick()
        compose.onNodeWithText("Open Mexerica", substring = true).assertIsDisplayed()
        compose.onNodeWithText("Continue").performClick()
        compose.onNodeWithText("Skip").performClick()
        assertTrue(skipped)
    }

    @Test fun pairingPointsToMexericaOnTheComputer() {
        compose.setContent { Hosted { PairingView(AppState(loading = false), viewModel()) } }
        compose.onNodeWithText("Continue").performClick()
        compose.onNodeWithText("Open Mexerica on your computer and show its pairing code.").assertIsDisplayed()
        compose.onNodeWithText("codync-host pair").assertDoesNotExist()
    }
}
