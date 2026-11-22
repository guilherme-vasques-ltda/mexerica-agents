// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android

import androidx.compose.foundation.layout.Column
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import com.codync.android.design.*

@Composable internal fun AnalyticsSheet() {
    Column {
        ModalHeader("Usage analytics")
        CardForm {
            CardSection {
                row { Text("Usage analytics and crash uploads are disabled in Mexerica.", style = Type.body, color = Palette.text) }
            }
        }
    }
}
