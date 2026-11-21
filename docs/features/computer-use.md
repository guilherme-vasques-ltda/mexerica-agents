# Computer use

Bots with **Use the computer** on get the built-in `computer` MCP server (`codync-host mcp computer`, `host/src/mcp.rs`). Every agent sees the same server; the host lists its tools and runs every call (`computerTools`, `computerCall`, loopback only), so Codync's rules apply whatever the harness:

- One bot at a time. While a phone has taken over Remote screen, bots may only use tools that look.
- `type_login` types a saved login only into its own site or app, and a password only into a password field. The agent never sees the value, and only the logins turned on for that bot exist for it.
- The harness's own approval rules still apply to each call.

## Computer access setup

Desktop onboarding includes **Computer access**, also available from **Settings → Computer access** and the tray's **Set up computer access…** action. Users can choose **Set up later** and keep using chat. Setup choices stay on this installation.

The desktop setup uses a compact device header and one outlined permission group, matching the toolbar and composer outlines. Rows distinguish ready, current and waiting steps; host and setup errors appear before the checklist. This requested outline is an exception to the default borderless filled-card rule. The shared Electron layout applies to macOS, Linux and Windows. iOS and terminal retain their existing status and setup guidance: neither runs this local graphical permission onboarding, so their layout is unaffected.

- macOS: explicitly enable the background helper, then request Screen Recording and computer control. Depending on macOS, control is listed as **Device Control and Data Access** or **Accessibility**. Setup identifies the exact helper name. Codync Screen includes the Codync icon for system permission lists. A fresh installation registers the helper only when enabled; existing registrations are maintained across updates.
- Opening setup, starting the macOS helper and returning from System Settings only check grants. Native prompts require an explicit setup action through local-only `requestScreenPermission`. The helper refuses to start the embedded driver until both grants are present. Newly granted screen recording triggers the existing helper restart mechanism.
- Linux: setup opens the system screen-sharing portal for a display, mouse and keyboard. Helper startup never opens a chooser. The session survives host reconnections; after the helper itself restarts, reopen setup to restore sharing explicitly. Denied or partial grants can be retried.
- Windows: computer-use bots need no macOS-style grants. Setup explains that phone screen viewing is not yet supported.
- iPhone: pair through QR / Ask Access, with no SSH configuration. The viewer explains missing permissions and points to setup on the computer. Capture-only access allows viewing; takeover remains disabled until input is allowed, and revoking input exits takeover.
- Terminal UI: the bot editor shows current helper/permission status and points to **Computer access**. System Settings, background-service registration and portal choosers require the graphical desktop, so the terminal does not issue those prompts. Status follows host events.

Desktop setup polls read-only status while open and checks again on focus. Opening Settings never counts as a grant. Failures retain retry and Settings actions; completion requires actual platform-specific status. Skipping grants no permissions. Older clients ignore the additive `permissionApp` status field.

## The driver

The tools come from [cua-driver](https://github.com/trycua/cua) (MIT), which works on apps in the background: by accessibility element where it can (`element_token` from `get_window_state`), by pixels posted to the app otherwise, and in the foreground only when a call asks for `delivery_mode: "foreground"`. The user's cursor and front app stay theirs.

`host/src/screen/cua.rs` owns it. One daemon serves every bot, started on first use and stopped with whoever started it (`CUA_DRIVER_PARENT_LIVENESS_STDIN`: its stdin closes when the parent ends, crash included). Requests use its line protocol on a private socket (Windows: a random named pipe), with the session `codync-<bot>`. Telemetry and update checks are off.

| | Who starts the driver | Permissions |
| --- | --- | --- |
| macOS | Codync Screen (`apps/screen-macos/Driver.swift`, `driver` request), spawned directly with `CUA_DRIVER_EMBEDDED=1` | Codync Screen's Accessibility and Screen Recording grants: macOS attributes the child to it, so the user grants nothing new and the driver never prompts |
| Windows | The host, from beside `codync-host.exe` | None; windows running as administrator are out of reach |
| Linux X11, Sway | The host, from beside `codync-host` (`DISPLAY` filled in when the service has none; Sway gets `CUA_DRIVER_RS_ENABLE_WAYLAND=1`) | None |
| Linux GNOME, KDE, other Wayland | Not used: bots get Codync Screen's portal tools instead (`helper_tools.rs`: screenshot, click, type… on screenshot pixels) | The Remote screen portal grant |

On macOS, windows on another Space have no accessibility tree, so background actions there are refused; the driver says so and suggests `delivery_mode: "foreground"`.

Computer use needs Remote screen on where Codync Screen takes part (macOS, Linux); on Windows it is always available. The host's screen state says which (`computerUse`), and the bot editor's toggle explains it.

### Na Mexerica

Os bots rodam no Hub, que pode ficar num computador com tela. O app
não usa mais o host deste Mac para decidir: o interruptor **Usar o computador** de
cada bot, no editor (Comportamento → Computador) e no painel da conversa, segue o
`computerUse` que o Hub informa. Com tela, aparece; com o bot ligado e sem tela,
continua para poder desligar; sem tela e desligado, dá lugar a uma linha que leva a
Ajustes → Computadores. A tela em si se liga no próprio computador do Hub
(`setScreenEnabled` é só local). Detalhes e textos em
[Editar um bot](editar-bot.md#usar-o-computador). A tela **Acesso ao computador**
deste Mac continua escondida enquanto não houver host local (`showsComputerAccess`).
O manual do harness não muda: o bot já recebe `MCP computer: true/false` a partir
de `cfg.computer` (`host/src/chat/context.rs`).

## Tools

The host offers 16 driver tools (`packaging/cua-driver/tools.mjs` lists them): `list_apps`, `list_windows`, `get_window_state`, `get_desktop_state`, `zoom`, `launch_app`, `click`, `double_click`, `right_click`, `drag`, `scroll`, `type_text`, `press_key`, `hotkey`, `set_value`, `invoke_menu`, plus `type_login` (not on Windows yet). Left out: `bring_to_front` and `kill_app` (they take over or end the user's apps), `clipboard_read`, the `browser_*` tools (each browser asks for Automation separately), and the driver's configuration, recording and session tools.

Descriptions come from the running driver, so they match the platform; when it can't start, the host lists the copy in `host/src/screen/cua_tools.json` and each call explains why it failed. Results pass through as MCP content. When the text is only a summary, the structured data follows as JSON; window snapshots add how to spell element tokens (`<snapshot_id>:<index>`).

`type_login {login, field, pid?}`: `login` é o id, o rótulo ou o site de um login ligado para o bot; um site com vários pede o id. O Codync Screen lê o controle em foco do app (`focusedField`: app, URL da página, campo seguro, papel/subpapel, dicas de rótulo e, no Linux, atributos multiline/maxlength/type/inputmode/autocomplete/contenteditable quando a ponte AT-SPI os expõe), o Hub confere com o login e digita pelo driver no app em segundo plano (macOS) ou na janela ativa conferida (Linux). A resposta do driver não volta. `field` é `username`, `password` ou `code`: `code` digita um código de dois fatores que o usuário enviou pelo cartão `request_login_code` — mesma conferência de site/app, sem exigir campo seguro, mais prova positiva de campo curto e numérico: `autocomplete=one-time-code` ou `type`/`inputmode` numérico, com tamanho declarado curto quando exposto. Papel ausente, papel/subpapel de busca, `inputmode=text`, maxlength/size maior que 10, tipo claramente não numérico, multilinha/contenteditable e a falta de metadados recusam — palavras de rótulo sozinhas não autorizam, e a plataforma que não expõe a prova manda digitar à mão. Qualquer recusa — sem prova, campo longo ou de texto livre, foco mudado antes de digitar ou ausência de código pronto — responde em pt-BR curto e pede para a pessoa digitar o código no site. O código e o Enter saem na mesma ação pelo caminho secreto (só eventos de input — sem screenshot, sem árvore, resposta fixa) e o código é consumido antes, então vale uma vez só, por 5 minutos mesmo sem `take`, e nunca fica guardado além disso. Erros do envio do segredo e do Enter viram mensagens fixas em pt-BR, sem eco do valor. O valor nunca volta no resultado da ferramenta, mas depois de digitado pode aparecer na tela como qualquer texto digitado.

Na Mexerica, a marca reservada `{{senha:` em `login`, inclusive sem fechamento
ou dentro de outro texto, recusa antes do cofre, da tomada de controle e do
driver. `request_login` também recusa a marca em `site` antes de criar pedido.
A resposta fixa é "Esta referência de senha exige um caminho protegido ainda
indisponível." Referências de senha ainda são inertes; não há coleta privada nem
concessão nova. Não remova a marca nem contorne a recusa. O comportamento legado
acima segue para entradas sem a marca.

## Packaging

`packaging/cua-driver/driver.json` pins the release and the SHA-256 of each archive; `fetch.mjs <target> <dir>` downloads, checks and copies the executables with `cua-driver.LICENSE`:

- macOS: the `Screen` target's *Embed cua-driver* build phase puts it in `CodyncScreen.app/Contents/Helpers/`. electron-builder leaves its Developer ID signature alone (`mac.signIgnore`).
- Windows: `tools/native-host.mjs` puts `cua-driver.exe` and `cua-driver-uia.exe` next to the host in `resources/`.
- Linux: the host release archive carries `cua-driver` (needs libX11, libXi and libxkbcommon); `install.sh`, Homebrew and `codync-host update` put it next to the host.

Updating the driver: change `driver.json` (version, tag, assets and sums from the release's `SHA256SUMS`), run `node packaging/cua-driver/tools.mjs <path to the new cua-driver>`, check the tool list diff, and run the checks below.

## Verification

- Host: `cargo test` covers the tool list, result shaping, the line protocol and takeover.
- macOS: with a fresh build, ask a computer-use bot to read a background app's window and click a control in it. The front app must not change and macOS must not ask for any new permission. Take over from the phone mid-task: actions stop, looking still works.
- Windows and Linux X11: the same task; Linux GNOME still gets the portal tools.
- `type_login` into a test site's password field, and a refusal for the wrong site or a non-password field.

## Origem até o transporte

Na Mexerica, `computerCall` conserva a origem original (credencial MCP/Ponte ou
autoridade de controle aplicável), a geração canônica
do perfil e o token original do turno antes dos awaits. O writer do helper e o
exchange CUA reconferem a origem em cada escrita, inclusive após Pending, e o
retorno revalida antes da projeção. Reconexão do helper não recupera chamadas
antigas. O waiter encerrado não envia pelo job seguinte.

Qualquer envio parcial/erro de transporte é incerto; descartar resposta ou fechar
writer não demonstra fim do observador. O caminho comum mantém suas permissões e
posse de tela. Esta etapa não identifica janela privada, não instala exclusão
nativa nem libera a caixa protegida.

A composição prevista no modo suportado tem caixa no Mac e observadores no
servidor/Lugares, com recusa local ao sinal de ajudante/erro. Attempt não exclui
a janela nem precisa de fatos da caixa neste modo. Plataformas sem cobertura
mantêm a caixa indisponível; ausência de fato não significa ausência de caixa.
