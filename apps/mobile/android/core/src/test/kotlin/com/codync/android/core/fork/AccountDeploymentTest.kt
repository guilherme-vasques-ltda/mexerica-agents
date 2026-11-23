package com.codync.android.core.fork

import java.util.Base64
import org.junit.Assert.*
import org.junit.Test

class AccountDeploymentTest {
    private val cloud = "https://cloud.example.invalid/api"
    private val origin = "https://app.example.invalid"
    private val key = "pk_" + "test_" + Base64.getEncoder().encodeToString("clerk.example.invalid$".toByteArray())

    @Test fun blankOrPartialConfigurationsAreDisabled() {
        assertNull(AccountDeployment.parse("", "", ""))
        assertNull(AccountDeployment.parse("", key, origin))
        assertNull(AccountDeployment.parse(cloud, "", origin))
        assertNull(AccountDeployment.parse(cloud, key, ""))
        assertNotNull(AccountDeployment.parse(cloud, key, origin))
    }

    @Test fun invalidTransportCredentialsAndOriginsAreRejected() {
        assertNull(AccountDeployment.parse("http://cloud.example.invalid", key, origin))
        assertNull(AccountDeployment.parse("https://name@cloud.example.invalid", key, origin))
        assertNull(AccountDeployment.parse(cloud, "not-a-publishable-key", origin))
        assertNull(AccountDeployment.parse(cloud, key, "$origin/a"))
        assertNull(AccountDeployment.parse(cloud, key, "$origin#fragment"))
        assertNull(AccountDeployment.parse(cloud, key, "$origin:443"))
    }
}
