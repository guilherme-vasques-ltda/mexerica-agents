# Client and host compatibility

Clients (iPhone, desktop app on macOS, Linux and Windows, terminal client) and the host update on
different schedules: the iPhone app waits for App Review, the desktop app and standalone hosts
update when their owner (or the opt-in automatic updater) installs a release. Any client can
meet an older or a newer host.

## Rule

Each side names the **oldest version of the other side** it still works with. Versions are
the ordinary release versions (`MARKETING_VERSION`, `host/Cargo.toml`, `apps/desktop/package.json`,
always equal). There is no separate protocol number.

| Floor | Lives in | Meaning |
|---|---|---|
| `minApp` | `host/src/compat.rs` `MIN_APP`, sent in `hello` | The oldest client this host serves correctly. |
| `minHost` | `apps/desktop/src/shared/compat.ts` `MIN_HOST`, `host/src/compat.rs` `MIN_HOST` | The oldest host this client works with. |

On every (re)connect the client reads `hello` and decides, in this order:

1. its own version is below `minApp` → **Update this app**;
2. the host's `version` is below its `minHost` → **Update Codync on <computer>**;
3. otherwise it syncs normally.

Versions compare as `major.minor.patch` (a leading `v` and any `-pre`/`+build` suffix are
ignored), with one rule across lines: major `0` is the Mexerica line and comes after every
inherited Codync `2.x`, so `0.1.0` is newer than `2.20.3` and the `2.20.1` floors keep their
meaning. Leaving major `0` needs this rule revisited. Clients built before the rule (installed
2.x apps) still read a `0.x` host as older and show *Update Codync on <computer>* until they
get it.

No desktop, versões ausentes ou ilegíveis do app, do Hub ou de `minApp` bloqueiam o
acesso ao Hub (fail-closed). A exceção é a versão de app `dev`; ela ainda exige versão
e mínimo legíveis do Hub.

## What each client does

| | iPhone and desktop (`BotStore.mismatch`); Android (`AppState.mismatch`, `AppStoreConnection.kt`) | Terminal client |
|---|---|---|
| Notice | `UpdateNeededCard` above the computer's bots and in place of the composer; status reads *Needs update* | *needs update* in the status line, with the reason |
| Update this app | iPhone: App Store page. Desktop: electron-updater check (`apps/desktop/src/main/updates.ts`). Android: text only | Text only |
| Update the host | Desktop: its own host is reinstalled (**Restart host**). iPhone and Android: instructions for that computer | `^k` → Check for updates |
| Sync while mismatched | Desktop: interrompida; hello em 30–240 s e ao reconectar (detalhes abaixo). iPhone: a cada 30 s | Continues; sending is blocked |

Drafts are kept while sending is blocked. A host update restarts the host, so the reconnect
reads the new `hello` and the notice goes away on its own.

The iPhone and desktop apps also treat **undecodable data from a newer host** as *Update this app*: an event
that stays undecodable after the one rewind, or a `hello` whose other fields can't be decoded
(its `version` and `minApp` are still read). From an older or equal host it's only logged.

### Isolamento no desktop

Cada store mantém um `VersionGate`. Reiniciar, reconectar ou encerrar o transporte
invalida a prova anterior; só um hello RPC compatível da geração atual libera o
cliente. O transporte guardado verifica chamadas, respostas pendentes e eventos,
inclusive de referências capturadas e clientes derivados por `inSpace`. O hello de
rechecagem é o único RPC interno permitido sem prova.

O bloqueio afeta somente aquele computador. O cache anterior continua legível e
não recebe gravações ou projeções do Hub sem prova, e nada é gravado no cache enquanto
o mismatch é conhecido. Uma releitura necessária fica pendente até
um hello compatível, inclusive quando só `minApp` mudar.

Só um mismatch **provado na geração atual** recusa com `Desatualizado` (`versao`, `minimo` e uma frase
traduzível). Antes da prova (partida, reconexão, reinício do stream, queda curta) a ação
espera o link como sempre, com o replay de 20 s; referências capturadas de uma geração
anterior falham como link caído (status 0), nunca com o motivo de versão. O mismatch
anterior continua visível na UI e protegendo o cache numa geração nova, mas é só informação
retida: não prova nada sobre ela, e a ação espera o novo hello (se ele ainda for
incompatível, recusa com `Desatualizado`). Versão ilegível do Hub aparece como
`encontrada: null`. Hello sem
`minApp` é tratado como Hub provavelmente antigo (*atualize o computador*). Ações de `perform` retornam `{ ok: false, error }` e notificam a
própria store por `erroAcao`/`acaoRecusada`; `ComputerAccess.request` também devolve
a recusa e notifica seu observável.

O recheck usa 30, 60, 120 e no máximo 240 segundos. Descarte, reinício e destrave
cancelam a espera. A rechecagem segue ativa em todo computador, selecionado ou não.
Um hello compatível destrava sem reabrir o app.

## Changing what crosses the wire

The policy is to move people forward rather than carry old formats: a breaking change ships
in **one release** that raises the floor, and updates are held back only as long as someone
couldn't get the matching side yet.

- **Additive changes need no floor change**: new fields (clients decode leniently; new
  Swift fields are optional), new methods, new enum values (kept as strings).
- **Renaming, removing or changing the meaning** of anything an older client reads or sends:
  change it, set `MIN_APP` to the first client version that no longer reads the removed
  field — when the change shipped behind a transition release (e.g. a server-side adapter),
  that can be an earlier release than the one that removes the field; otherwise it is this
  release (coordinate the iPhone part before distribution), add a row to the floor history, and re-record
  the wire shape snapshot. That release isn't offered (downloads, updaters, Homebrew) until
  a compatible iPhone app is available; publication requires a separately configured process.
- **Raise `minHost`** when the client's core flows need something older hosts lack. Hosts
  update without review, so this only asks the person to update the computer. An optional
  feature that needs a newer host hides its control instead.
- Floors never exceed the release that ships them (unit tests in both crates check this).
- The encrypted channel's handshake version (`v` in its `hello`, rejected with
  `unsupportedVersion`, close code 4400) is separate: the host must keep accepting every `v`
  a supported client sends.

### Holding releases for the App Store

A release that raises `minApp` can lock out paired iPhones until a compatible app is available. The inherited publication pipeline was removed; no automated release hold is provided here. A maintainer must coordinate and validate distribution before publishing.

### Reminders

While everything still works, a client behind a newer release gets a reminder, dismissed per
version. A host behind a client gets none: computers update themselves, and the phone usually
trails them (App Store review), so nudging a computer from it would only be noise.

| Client | This app is behind |
|---|---|
| iPhone | Card at the top of the bot list (App Store version from the lookup), *Update* opens the App Store |
| Desktop | *Update to <version>*, then *Restart to update*, on the update card, in the menu bar's Settings → Updates and on the Settings Updates page |
| Terminal client | — (it is the host binary) |

### Wire shape snapshot

`wire_shape_matches_the_snapshot` (`host/tests/e2e.rs`) runs a full turn against a real host
(fake agent: user message, narration, tool call with a diff, plan, permission, final reply),
then records every field path and JSON type of `hello` and the event stream's catch-up (bot
and entry events, entries per kind) in `host/tests/fixtures/wire-shape.json`, together with
the host's `minApp`.

| Change | Test |
|---|---|
| Nothing on the wire | Passes |
| New field | Fails until recorded: `CODYNC_UPDATE_WIRE_SHAPE=1 cargo test --test e2e wire_shape`, no decision needed |
| Field removed, renamed or retyped | Fails, and recording is refused, until `MIN_APP` differs from the recorded one; then record |

So a breaking change can't land by accident: it either keeps the old field or raises the
floor. `null` fields match any type. Not covered: subtrees that depend on the machine rather
than the code (`backends`, `screen`, `urls`, `usage`), fields the sample turn doesn't produce
(replies, group chats, routines and other method responses), and what the host
accepts in requests. Changes there still follow the rule by review.

## Floor history

Update this table in the release that changes a floor, with the reason.

| Release | Host `minApp` | Client `minHost` | Why |
|---|---|---|---|
| 0.1.x (bot groups N+1) | 2.20.3 | 2.20.1 | The `botSections`/`botSectionAction` fields and events are removed after one transition release whose host translated them over the new tables. The floor is 2.20.3 — the first client line that only reads `botGroups` — not this release: 0.1.0 desktops (with or without the transition client) stay served, and a pre-transition 0.1.0 desktop simply shows the flat bot list. Releases 2.20.1/2.20.2 that only read the legacy field are refused with "Update this app". |
| 0.1.0 | 2.20.1 | 2.20.1 | First Mexerica release: the version line moves to `0.x`, ordered after every inherited `2.x` (see the comparison rule above). Nothing changed on the wire, so the floors stay. |
| 2.20.1 | 2.20.1 | 2.20.1 | Reply threads gave way to quote replies: the `thread` method, `send.threadId`, `markRead.threadId`, entry `threadId`, `data.thread` and bot `workingThread` are gone, and old thread replies sit in the main chat. Older apps would open empty threads. The clients no longer leave thread replies out of the chat or read threads, so an older host would show them inline, routine results twice and thread unread counts that never clear. |
| 2.18.0 | 2.18.0 | 2.12.0 | Memory moved from Engram into the host: the memory methods take and return the new two layers, and the Engram ones (`memoryDetail`, `reviewMemory`, `exportMemory`, `importMemory`) are gone. Clients still talk to older hosts; only their memory screen shows an error there. |
| 2.12.0 | 2.3.0 | 2.12.0 | The clients require Engram memory management fields and methods. Older clients can still list and forget memories on the new host. |
| 2.4.0 | 2.3.0 | 2.3.0 | First release with the check. Since 2.3.0 the wire only gained `minApp` and lost the unused `protocol`; 2.3.0 is the oldest pairing verified to work, and a higher floor would lock out hosts that never auto-update. |

## Trying it on a simulator

Run a throwaway Debug host (it registers with the dev cloud) on its own data and port, never
on `~/.codync` or 19222:

1. Build a copy with a patched floor or version into a separate target, then restore the
   source: e.g. `MIN_APP = "9.0.0"` in `host/src/compat.rs` (app too old) or `"version": "2.2.0"`
   in `hello` in `host/src/api/host.rs` (host too old), built with
   `CARGO_TARGET_DIR=<scratch>/hosttarget cargo build`.
2. `CODYNC_HOME=<scratch>/hubhome <scratch>/hosttarget/debug/codync-host serve --port 19333`.
3. Pair the simulator: `POST /api/pairing` with `<scratch>/hubhome/token`, then launch with
   `SIMCTL_CHILD_CODYNC_PAIR_URL=<pairingUrl>`.
4. Swap hosts on the same `CODYNC_HOME` (patched ↔ normal) to see the notice appear in the
   list and in place of an open chat's composer, and go away after the reconnect.

The simulator has no App Store app, so *Update Codync* ends in a Safari error there; on a
device it opens the App Store.
