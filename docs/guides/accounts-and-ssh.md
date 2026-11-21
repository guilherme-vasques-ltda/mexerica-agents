# Accounts, device approval and SSH

O desktop usa a API de conta (`apps/desktop/src/main/account.ts`), com token protegido pelo `safeStorage` e OAuth no navegador do sistema. Os clientes móveis são mantidos separadamente.

## Configuration

A configuração pública do desktop fica em `apps/desktop/config/<env>.plist` e é processada por `apps/desktop/tools/account-config.mjs`, que gera `resources/account-config.json`. A configuração do provedor e o consentimento OAuth devem ser conferidos; segredos não devem ser incluídos no aplicativo.

### Desktop Sign in with Apple

The welcome screen (`AccountWelcomeView.tsx`, mounted by `AccountEntryGate.tsx`) is the desktop app's gate: it offers e-mail and password (**Entrar**), followed by **Apple**, **X** and **Google** under “ou entre com”. The chat shell does not mount until the main process confirms the session and secure storage. The password is typed only in the account fields and cleared when submitted, when another account action starts and when the screen unmounts; it is not stored or logged. An uncertain result is checked through the same attempt without automatically resending the password or OAuth. Additional challenges such as MFA do not have a screen in this version and cannot be skipped. Without sign-in configured the build only says it is unavailable; there is no offline or local-pairing bypass. **Settings → General → Account** has Sign out, which returns to the gate. A previous `onboardingDone` preference does not grant entry.

On the desktop, `AccountService.signIn('apple')` starts Clerk's `oauth_apple` sign-in
and opens its redirect in the system browser, which returns to
`com.pokai.Codync://callback` (`apps/desktop/src/main/auth.ts`). This uses Clerk's
Apple Services ID and needs no native Sign in with Apple entitlement. iOS continues
using Clerk's native Apple authorization.

The production Services ID `com.pokai.Codync.signin` is associated with primary
App ID `com.pokai.Codync.ios` in Apple Developer. Keep that association: Apple's
web and native authorization must represent the same identity rather than being
joined by email. The desktop app's `com.pokai.Codync://callback` must also remain allowed
in Clerk. See [Apple's web configuration guide](https://developer.apple.com/help/account/capabilities/configure-sign-in-with-apple-for-the-web/)
and the Clerk guide for Apple OAuth (Social Connections).

A configuração histórica de credenciais do upstream não comprova a configuração da Mexerica. Não copie credenciais nem identificadores de outra instância.

To check cross-device discovery, sign in with the same Apple Account on both
devices of one environment (Dev ↔ Dev, production ↔ production) and verify that
Clerk has one user ID. Then verify that the computer joins the account and the phone
requests access with the existing approval flow. Hide My Email does not change the
device approval requirement.

The desktop app's Apple sign-in and production cross-device identity matching
still need to be checked on a release build.

## Flow

O desktop usa a API de conta para login e restauração da sessão. A sessão não autoriza um computador por si só.

The custom account menu displays the authenticated email and avatar when
available. `AccountSession.sessionToken()` (desktop: `AccountService.sessionToken()`, used by `cloud.ts`)
hands the session JWT to the Codync cloud client. A Clerk session never authorizes a computer by itself: each
computer approves each device after comparing a 6-digit code
(remote relay protocol, §4.2). Conversations are not uploaded.

## Verification

- Run the desktop app with `npm run dev` in `apps/desktop/`.
- Open the account menu from either sidebar layout; test arrows, Return, Escape
  and clicking outside the panel.
- Sign in with Apple and Google test users, verify the avatar/email, restart the app and
  verify session restoration, then log out and verify the anonymous state.
- Cancel the browser flow and verify no error; retry with networking unavailable
  and verify that the application stays usable.

Dashboard creation and the end-to-end Apple/Google flows require the user's browser
consent; compilation alone does not verify those steps.

References:
- https://github.com/clerk/clerk-ios

## Desktop: computers, approvals and SSH

The desktop app attaches this computer's host and its SSH computers over loopback; they
follow into whichever account is active. The account's other computers are not reached
directly (the encrypted channel wasn't ported to the desktop app), so their
"Ask for access" rows show status only. An older copy of a computer the app uses shows
*Older copy · last seen …* with *Remove from account* (`DELETE /v1/computers/{id}`, confirmed
first). See [desktop app](../architecture/desktop-app.md).

**Computers & devices** (Settings) shows, for this computer and each SSH computer:
*Reach from anywhere* (`setCloud`, on/off; the host's build decides which cloud),
*Add to account* (claim: `POST /v1/claims` → loopback `claimSign` → complete) and
*Remove from account* (`unclaim`), a pairing QR, and the authorized devices with revoke.
Signed in, this computer joins the account on its own once per host identity; an identity
that was already in the account and was removed (here or from a phone) is never re-added
silently, only through *Add to account*. The section below, *Other computers in your account*,
lists only the account's other computers. A device asking for access raises the menu bar dot
and opens the approval sheet; the device shows the 6-digit code and the person types it into the
sheet. Approve stays disabled until the typed code equals the Hub's code (the Mac never displays it),
so no grant exists before the comparison.

SSH computers use the system OpenSSH (`apps/desktop/src/main/ssh.ts`): `ssh -G` to
resolve the target, `ssh-keygen -F` against `~/.ssh/known_hosts` and
`~/.codync/ssh_known_hosts`, a fingerprint confirmation on first contact (no proxy),
`codync-host info --json` for the identity and loopback token, then
`ssh -N -L 127.0.0.1:<free port>:127.0.0.1:<remote port>` with keepalive and backoff.
A changed host key or a different computer ID blocks the connection. Sign-in is key or
ssh-agent only (`BatchMode`: nothing prompts for a password or passphrase); a refused key
stops with a message instead of retrying. The remote command searches Homebrew,
`~/.local/bin` and the Mac app bundle for `codync-host` too, since `sh -l` doesn't read
`~/.zprofile`. At launch the app kills tunnels a crashed or force-quit copy left behind
(`pkill` on the `.codync/ssh_known_hosts` argument only Codync's tunnels carry).

## Verification boundaries

Build with normal simulator signing for Clerk Keychain access. Test Google consent, cancellation, restoration, two-account switching, per-account cache isolation and sign-out on real test accounts. Build success cannot verify Clerk Dashboard state.

For SSH, verify first-contact fingerprints and changed-key rejection. With ProxyJump/ProxyCommand, establish trust in a terminal first. The app checks that the local forwarding listener belongs to its SSH process. The desktop app is not a phone-to-SSH gateway.

