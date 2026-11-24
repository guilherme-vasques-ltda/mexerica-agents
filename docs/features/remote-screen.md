# Mexerica — tela remota

Remote screen lets an authorized phone view and control a computer. Signaling travels through the end-to-end encrypted host API; video and input use WebRTC, including when a TURN server forwards the encrypted packets.

## Connection path

1. The phone calls `screenPrepare {relay}`. `relay` is true when its active host channel uses Cloudflare; direct LAN/Tailscale connections do not contact a STUN/TURN provider.
2. The host checks the `screen` scope and that screen access is enabled. For relay connections it signs `POST /v1/host/screen-ice {deviceKey}`. The Worker requires a registered, active computer belonging to an active account. The host attests the device's local authorization, so account grants and local QR pairing both work on a claimed computer.
3. The Worker issues Cloudflare Realtime credentials with a one-hour TTL, tags usage with `computerId:deviceFingerprint` (the fingerprint is the base64url encoding of the first 16 bytes of SHA-256 of the device public key; the resulting 45-character tag fits Cloudflare’s 64-character limit), and returns `Cache-Control: no-store`. Permanent TURN keys stay in Worker secrets.
4. The host reserves a random session ID bound to the calling device and keeps the exact ICE configuration in memory. The phone and helper use that configuration. Clients cannot supply arbitrary ICE servers to the host.
5. `screenOffer {session, sdp, display?}` carries the completed offer to the helper. Both peers gather for up to ten seconds. ICE prefers direct candidates and uses TURN when necessary, with UDP, TCP and TLS on port 443 available.
6. The phone reconnects with fresh credentials five minutes before expiry. O Host verifica sessões ativas a cada cinco segundos e pede fechamento para expiração, dispositivo revogado ou lease expirado. Em erro de controle, mantém a monitoração periódica do mesmo registro/Link, sem adotar sucessor; resposta de controle não confirma término nativo. A prepared session must start within one minute.

Connections prepared through the cloud use a **4 Mbps / 30 fps** encoding limit even if ICE subsequently finds a direct path. Direct sessions retain their platform defaults (Mac: 16 Mbps / 60 fps). These are encoding limits, not exact bandwidth or billing caps. TURN does not carry plaintext desktop content; WebRTC DTLS-SRTP and data-channel encryption remain between the phone and helper.

A cloud outage or missing TURN configuration produces an explicit error. A successful chat relay connection alone does not prove that media or input works.

## Phone orientation

Opening the iPhone viewer enables portrait and both landscape orientations without
requesting a forced turn. UIKit chooses the orientation based on how the phone is
held and the system rotation lock. The Rotate button remains available for an
explicit override. Closing the last viewer restores the app's portrait layout.
The video fits the available bounds, and touch coordinates use the resized viewport.

The iPad already supports all orientations. The desktop app and the TUI do not
use the iPhone viewer or its orientation policy; the capture helpers stream the
computer display without choosing the phone's orientation.

The Android viewer follows auto-rotate like the rest of the Android app. Rotate locks
the other orientation until the viewer closes. The activity handles orientation and
keyboard changes itself, so turning the phone or attaching a hardware keyboard keeps
the stream. In the background the Android viewer closes its stream and hands control
back to the bots, then reconnects when the app returns.

## Boundaries

- `host/src/screen/` coordinates access to the local helper and owns viewer sessions. Another device cannot renegotiate or close a session it does not own.
- `apps/screen-macos/` is the macOS capture/input helper (Xcode `Screen` target, `CodyncScreen.app`). The desktop app bundles it and registers it through `SMAppService` (`apps/desktop/src/main/screen.ts`); it is responsible for the OS permissions, including those of the computer-use driver it starts.
- `apps/screen-linux/` implements the Linux helper using desktop portals and GStreamer, including ICE URL conversion and TURN transport configuration. The host starts it from beside its own executable (then `PATH`). Linux host releases ship `codync-screen` in the same archive (built on Ubuntu 24.04: glibc 2.39+ and GStreamer 1.22+ with the base, good and bad plugins, PipeWire and xdg-desktop-portal at run time); `install.sh`, Homebrew and `codync-host update` put it next to the host.
- O socket local é `screen.sock` na pasta de dados resolvida pelo Host. No Linux, o Host captura esse caminho uma vez e envia `--socket <caminho absoluto>` ao `codync-screen`, sem shell. O helper exige esse argumento único, sem caminho vazio, relativo, NUL ou barra final. Ele recusa configuração inválida antes de GStreamer, conexão e portal; não procura sockets por HOME, CODYNC_HOME ou DONNABOT_HOME. Reconnect reutiliza o caminho capturado. O token `screen-restore-token` fica na pasta desse socket; não se migra nem se procura token em outra pasta. SDP continua non-trickle; input usa os canais `input` e `input-fast`.
- Clientes móveis são mantidos em repositório separado.
- No auxiliar Linux, `open_app` recebe o ID exato do arquivo desktop, com sufixo `.desktop` opcional (por exemplo, `firefox` ou `firefox.desktop`). O auxiliar preserva maiúsculas e minúsculas e passa o ID validado ao lançador, que é quem resolve o aplicativo. Não há busca por nome de exibição.
- No Linux, somente `open_app` requer `gtk-launch`, fornecido pelo pacote [`libgtk-3-bin` no Ubuntu 24.04](https://packages.ubuntu.com/noble/amd64/libgtk-3-bin/filelist).
- A restauração automática do Portal exige uma pasta de estado regular, sob controle exclusivo do usuário, sem link simbólico no último componente. Se a pasta não atender a esse requisito, a conexão do auxiliar continua, mas o Portal volta a pedir permissão. A gravação publica no diretório já aberto e validado, sem reabrir o caminho nem detectar se ele passou a apontar para outro diretório.
- The Linux helper answers with the packetized H.264 payload the phone offers, feeds the encoder 8-bit 4:2:0 video and timestamps capture on the system clock. It answers before capture starts, so the profile is negotiated before the encoder fixes its output.
- Screen access is on by default on macOS and on Linux while a graphical session (Wayland or X11) is running; headless servers stay off. Turning it on or off is remembered. Enabling through `setScreenEnabled` requires a loopback caller. Interactive OS permission prompts must be completed on the computer.
- Bots with their computer capability enabled receive the built-in `computer` MCP tools, run by the computer-use driver that Codync Screen starts on macOS ([computer use](computer-use.md)). Their permission policy still applies. An interactive phone can take over; bots may still look.
- Each device may hold four sessions; the host allows 32 total, including pending sessions. Credential issuance is limited to 12 requests per minute per account by `TURN_LIMITER` (Cloudflare's per-location rate limiter).
- O fechamento pede ao helper original que encerre a sessão e trata sua resposta de controle; isso não atesta término nativo de captura, portal ou operação em voo. As credenciais TURN emitidas permanecem utilizáveis até seu TTL de uma hora. Issuance limits and encoder limits do not constitute a hard monthly spending cap. Track Realtime egress by the credential's custom identifier.

## Cloud setup

A emissão de credenciais TURN exige credenciais de serviço e limitação de requisições em configuração própria autorizada; a configuração padrão não as provisiona.

O procedimento herdado de configurar segredos e publicar foi retirado: `npm run deploy:dev` e `npm run deploy:main` recusam, e não há implantação de TURN autorizada por este guia. A seleção e configuração de credenciais exigem tarefa própria. Computadores sem vínculo ainda podem usar conexões diretas de tela, conforme suas condições.

References: [Cloudflare credential issuance](https://developers.cloudflare.com/realtime/turn/generate-credentials/), [usage analytics](https://developers.cloudflare.com/realtime/turn/analytics/), [pricing](https://developers.cloudflare.com/realtime/sfu/platform/pricing/).

## Verification

Run cloud tests/type checking, host formatting/Clippy/tests, iOS Kit package tests, Android `:core:test` and `:app:testDebugUnitTest`, and builds of the iOS app, the Android app and the macOS `Screen` helper target. Run Linux helper tests in a Linux environment with GStreamer and PipeWire development packages.

Live acceptance requires the configured Worker and fresh host/helper/phone builds:

- Check screen-recording and input permissions, capture, click/type/scroll, clipboard, display changes and closing the viewer.
- Run the checks below on an Android phone too, including a hardware keyboard and IME composition (Zhuyin, Pinyin).
- Open the iPhone viewer while upright and while already held sideways, then rotate
  in both directions. Check video fitting and direct taps after each turn. Repeat
  with rotation lock on and with the Rotate button; closing returns to portrait.
- Repeat with the phone on cellular and the computer on a different network, without Tailscale. Verify a selected ICE candidate pair with `candidateType=relay` in WebRTC diagnostics and observe Realtime egress; signaling via Cloudflare alone is insufficient.
- Verify UDP-blocked connectivity using TURN over TCP/TLS 443.
- Verify Wi-Fi/cellular changes, reconnect after sleep and renewal before the one-hour expiry.
- Revoke the phone or disable screen access while streaming; input and media must stop. Verify a control-only device cannot call `screenPrepare` or `screenOffer`.
- Verify LAN/direct-only mode without internet access, and the explicit error when relay secrets are absent.

See the Cloudflare documentation for transport checks and [architecture](../architecture/overview.md) for data flow.


## Estado da tela por Space na Mexerica

O estado público de uso por agente é projetado para o Space solicitado:

| agentUse | agentBot | Significado |
| --- | --- | --- |
| idle | null | O broker não observa posse ativa de agente. Não concede permissão. |
| using | ID | Posse ativa com bot vivo, runtime observado e conversa canônica aberta nesse Space. |
| unavailable | null | A posse não tem audiência comprovada nesse contexto. Não afirmar Ready, nome, destino ou contagem. |

Para o Berga principal trabalhando na Loja, o Pessoal recebe unavailable e
null; a Loja pode receber using e sua identidade quando a seleção atual de
principal e a conversa real confirmam o vínculo. Não se altera o Space do
perfil nem se cria conversa/grant para produzir esse resultado. Lane ausente,
desconhecida, conversa fechada, vínculo revogado ou exclusão retêm a identidade.

A fonte única atende hello RPC, hello do stream, `screenStatus`, respostas de
`screenTakeover`, `setScreenEnabled`, `requestScreenPermission` após await e
eventos screen. O stream entrega a cada Space sua própria projeção e relê
broker/audiência ao entregar eventos anteriores. Mudanças pertinentes de runtime,
perfil/canal, conversa ou exclusão atualizam o evento screen existente, para não
conservar o usuário de tela de um contexto anterior. As permissões Screen/Control,
limites, posse, prazo do broker, vídeo e disputa de controle permanecem iguais.

A UI trata unknown/ausente como indisponível e mostra **Uso do computador
indisponível** sem nome, contagem ou Ready. No app do Mac, todas as respostas e
eventos de tela passam pelo mesmo normalizador; só `using` com ID mostra o cursor
na linha do bot e "*Bot* está usando" na bandeja, e só `idle` permite "Pronto";
na bandeja, uso indisponível vem antes da contagem de quem vê. Num space aberto,
a vista lê o estado do stream daquele space e mostra quem usa (principal ou bot
do space) ou "Uso do computador indisponível", sem ligar o uso a uma conversa.
Sem o link, o app esquece quem usa (fica indisponível) até o próximo hello, sem
mudar `computerUse` nem permissões. Uma resposta de tela só nomeia quem usa se
voltar na mesma época local (sem queda, hello ou evento de tela no meio) e pelo
link vivo que perguntou; reconectar com o mesmo cliente não a torna atual.
Reconectar à mão (⌘R/Reconectar) ou um resync também tiram quem usa antes do
novo hello, e nada de um stream já encerrado chega à tela, no Pessoal ou num space.
Uma época nova só nomeia depois de observada pelo próprio stream (hello ou evento
de tela): uma resposta de tela pedida antes disso, mesmo pelo cliente novo, não traz
quem usa.
O schema atual é 13. Cache gravado com schema anterior entra com runtime
indisponível até a resposta do Hub.
Não se editaram clientes iOS/Android nem MIN_APP e não se declara compatibilidade
móvel entregue.

## Conclusões de viewer vinculadas ao registro original

`screenOffer`, `screenClose` e o vigia conservam o registro e a conexão do helper
originais ao esperar. Resposta ou erro atrasado não inicia, expira, remove ou
fecha um registro sucessor com o mesmo id. Notificação de uma conexão anterior
não remove o viewer da conexão nova. O vigia não procura um novo registro pelo
id para concluir uma tarefa antiga. Se close falha e a origem continua atual,
mantém a monitoração e o controle periódico herdados, com intervalo de cinco
segundos; não é replay de recibo ou promessa de término nativo. Ao mudar a
origem, o vigia antigo termina sem adotar o sucessor. Controles atuais continuam
sob os escopos/permissões existentes.

É um pré-requisito de identidade/protocolo, não evidência de término/exclusão
nativa nem L3 fechada. `close` com resposta `{}`, EOF, remoção da UI, timeout e
drop do waiter não provam que o observador parou. Confirmação por pipeline,
portal/stream e CUA continua pendente; a caixa e a revisão humana não são liberadas.
Veja o mapa e os limites e o contrato Linux proposto.
No modo suportado, caixa no Mac, observadores remotos e a recusa correspondente são
condições distintas. Nenhum novo canal, filtro de janela ou API privada aqui.

### Troca de helper e estado incerto

Ao trocar/perder a conexão do helper, somente seus registros saem do conjunto
ativo e deixam de ocupar as quotas32/4 e o controle do usuário se não houver
viewer ativo restante. O Viewer canônico permanece em RAM como Incerto, com o
Link e dono conservados, sem ICE, servidores, credenciais TURN ou prazo de
preparo. Conclusão/notificação/EOF antigo não afeta o sucessor. Há no máximo256
registros Incertos: ao exceder, os mais antigos saem do detalhe e um resumo de
incerteza fica ligado até reinício. Lista e resumo pertencem ao mesmo estado
canônico; o resumo significa incerto, nunca permissão ou término. Não há coleta
por idade, recibo nativo ou API privada consumível. Ao reiniciar, boot sem lista
ou resumo continua DESCONHECIDO até fato nativo L3; não prova ausência de
observador. A limpeza do registro não promete apagar bytes já lidos/copiados.


No Hub, validar a origem e enfileirar answer/close compartilham a mesma seção
síncrona curta. Close marca o Viewer original em fechamento antes do enqueue:
nenhuma oferta posterior dessa origem é enviada, mesmo sem ACK ou após erro,
cancelamento ou reenable. Answer já enfileirado não bloqueia close na espera de
SDP; sucesso tardio não reabre o registro. A espera ocorre fora das travas.
O vigia conserva o ciclo de5s sobre a mesma origem em fechamento. Disable marca
o snapshot original antes de closeAll; conclusão antiga não retira sucessores
nem sobrescreve seu controle. Isso ordena controle legado, sem fato nativo.

### Produtor Linux

A fonte do helper Linux passa a pedir NULL e conferir o retorno e o estado do
MESMO pipeline original. O lifecycle conserva a operação antes dos awaits;
close cancela a espera assíncrona Rust do answer e impede registro/Playing
tardios. As promises GStreamer/SDP e input/datachannel não são drenados nem
atestados. O consumidor de input/clipboard/view guarda a Operation original da
Session (referência fraca) e confere que ela segue aberta depois de retirar cada
mensagem e outra vez imediatamente antes de chamar o portal. Fechada ou
descartada, ele encerra e descarta as mensagens pendentes sem efeito novo; um
`perform` já iniciado não é interrompido nem repetido. A prova usa um sink D-Bus
privado de fixture, não tecla nativa nem compositor real. Callback antigo não remove nem notifica sucessor com o mesmo id.
Close sem entrada no helper e closeAll vazio devolvem `{}` como controle legado,
permitindo liberar a reserva preparada no Hub; isso não confirma ausência nem
parada nativa. Quando existe operação pending sem pipeline ou a parada NULL
falha, o resultado continua incerto/erro; o registro fechado permanece no mapa
do helper até conclusão nativa ou fim do Link, sem reabertura automática.

Isso é implementação interna ainda aguardando as provas GStreamer na fonte
publicada. O recibo positivo para o Hub está descrito abaixo como candidata (C2),
sem disponibilidade.
Não há ferramenta nova ou liberação privada. Resposta `{}`/closed continua controle
legado. Portal, screenshot, AX, input, PipeWire global, outros viewers/helpers,
caixa e revisão humana ficam fora.

### C2 — confirmação individual Linux

A fonte candidata liga o primeiro id inteiro de `answer` ao Viewer/Link e à
Operation originais. Restart conserva essa admissão e exige que ela ainda
exista. Só o primeiro fechamento individual manual de um Viewer Started pede
a confirmação: NULL verificado no mesmo pipeline, registrado antes do close,
e identidade original conferida novamente no helper e no Hub. A resposta ao
pedido de close tem seu próprio id; ele não substitui a admissão do answer.
O resultado interno empresta o Original, é descartado pelo close público e
não é guardado para recuperação ou consumido por ferramenta privada.
O resultado positivo exige objeto com somente `nativeStop`, também objeto com
somente `answerId` inteiro i64 igual à admissão e `state` string `"null"`.
Campos extras em qualquer nível são divergência, nunca confirmação.

Prepared/abertura, retry, vigia5s, Drop/Attempt, ended, closeAll, `{}`,
notificação String, EOF, timeout e remoção da UI permanecem controle ou
incerto. Parâmetro answerId ausente é legado; presente com tipo/id inválido
recusa antes de agir, sem fallback por String. Abrir sem registro não produz
positiva, mesmo se o cleanup alcançar NULL. ACK perdido não permite replay
positivo. Helper antigo continua sem confirmação; helper novo com Hub antigo
sem answerId usa controle legado. O ramo C2 é Linux, sem inferir suporte por
status ou plataforma anunciada.

É implementação candidata, ainda sem aceitação: exige prova própria e revisão de
segurança sobre a fonte congelada. O transporte e a correlação são testados,
sem provar GStreamer. A prova C1 histórica não cobre esta mudança.
Não fecha portal, PipeWire global, screenshots, AX, outros viewers/helpers ou
macOS e não libera caixa, revisão humana ou L3 global. Não há ferramenta,
permissão, estado de app/MIRROR ou serviço novo.

No Linux, override relativo da pasta do Host é composto uma única vez com o
CWD do Host para obter o socket absoluto compartilhado por listener e spawn.
A configuração global de pasta permanece intacta. Componentes internos do
caminho são preservados para manter a resolução do sistema (inclusive
symlinks); falha ao obter o CWD recusa listener e supervisor, sem fallback.
