# Composer behavior and regression checks

The Electron composer restores the draft and layout behavior of per-conversation
drafts, animated field growth, and Shift-Return. Drafts also survive restarting the client.

## Text and keyboard

- Desktop: Return sends; Shift-Return and Option/Alt-Return insert a newline.
  Confirming a CJK input-method candidate or holding Return does not send.
- **Atalho de envio** (Ajustes → Conversa → Envio; preferência deste aparelho,
  `composerSendMode`): três modos, regra pura em `views/thread/composer-keyboard.ts`.
  - **Enter** (padrão): Enter envia; Shift+Enter e Alt+Enter quebram a linha.
  - **⌘+Enter para várias linhas**: Enter envia enquanto o rascunho tem uma linha só; quando o
    rascunho já tem uma quebra de linha, Enter quebra a linha e ⌘+Enter envia.
  - **⌘+Enter sempre**: Enter sempre quebra a linha; só ⌘+Enter envia.
  Em qualquer modo ⌘+Enter (Ctrl+Enter fora do Mac) envia e Shift+Enter quebra; o rótulo mostra
  Ctrl fora do Mac. As guardas do método de entrada e do Enter segurado valem nos três. O
  `aria-keyshortcuts` do campo acompanha o modo. Com o menu `/` aberto, Enter continua escolhendo o
  comando em qualquer modo; ⌘/Ctrl+Enter envia o que está escrito.
- **Seletor de modelo** (`views/fork/seletor-modelo/`): um chip na linha de baixo do
  campo, ao lado do escudo de aprovação, mostra o modelo e o esforço da conversa. A lista abre presa ao chip,
  acima da caixa inteira do campo, e cabe sempre na janela (rolagem interna em janela baixa). Ele abre o seletor só quando o Hub anuncia a troca por
  conversa (`sessions.sessionActions` com `engine`) e a sessão traz o `engine` com revisão; sem
  isso, é só leitura. O seletor é compacto: barra de ícones (Favoritos, provedores), busca e lista (estrela,
  ⌘1–⌘3 nos três primeiros favoritos, "Padrão do motor", sem o "Default (recommended)" do motor), esforço do
  catálogo real, Padrão/Rápido só se o motor tiver, e o acesso, que leva às configurações do bot. A escolha vale
  só para aquela conversa (`sessionEngineSet` com `expectedRevision`); os erros do Hub viram texto por código.
  O Hub anuncia `engine` nas conversas do Pessoal (principal e extras): a escolha vale a partir
  do próximo turno daquela conversa (um turno em andamento termina com a seleção com que começou), o rótulo do topo
  mostra a seleção configurada e editar o padrão do bot só afeta conversas novas. Trocar para outro motor exige a
  conta destino do cadastro do Hub: com uma conta conectada desse motor, ela vem marcada no campo Conta; com várias,
  a pessoa escolhe antes; sem nenhuma, o motor aparece indisponível. Modelos fora da lista da conta ficam desativados
  (sem lista, nada é filtrado). Um favorito ou atalho ⌘1–⌘3 de OUTRO motor primeiro abre a aba desse motor com o
  campo Conta (e o motivo, se não der para trocar), sem gravar; a troca sai na escolha seguinte, já com a conta à
  vista. Um favorito ou atalho do MESMO motor troca direto, como antes, inclusive com a confirmação dos 50 mil
  tokens quando ela se aplica. Se o app não conseguir ler as contas, o seletor avisa "Não consegui ler suas contas;
  abra de novo para tentar" (diferente de "nenhuma conta conectada") e não troca de motor; fechar e reabrir o seletor
  lê de novo.
  Trocar de modelo no mesmo provedor com 50 mil tokens ou mais no medidor de contexto pede confirmação
  ("Trocar o modelo agora vai gastar bastante do seu limite de IA…"); sem medida, troca direto. O mesmo componente
  serve ao criar bot (modo `padraoDoBot`), com os favoritos compartilhados (`modelPickerFavorites`).
- **Skills no menu /** (Ajustes → Conversa → Avançado; `slashMenuSkills`, padrão ligado):
  desligado, o menu `/` mostra só os comandos próprios (`/fast`). Digitar `/<id-da-skill> ` à mão
  continua funcionando: o Hub expande a mensagem como antes. Regra em `views/thread/slash-commands.ts`.
- The field grows and shrinks from one to eight lines, then scrolls internally.
  Window width and text-size changes remeasure wrapped text. The live textarea
  remains mounted during measurement, preserving selection and composition.
- The call, send, stop and interrupt actions share one button. Its symbol and
  colors transition without replacing the button or losing keyboard focus.
- Attachment rows, mention suggestions and the conversation inspector animate
  both opening and closing. Rapid toggles cancel pending reveal callbacks.
  Reduced Motion removes these transitions.
- iOS keeps its native multiline field, keyboard and existing Motion animations.
  The terminal uses immediate text layout, with Shift/Alt-Enter (where supported)
  or Ctrl+J for a newline; it has no voice button or graphical inspector animation.

## Attachments

iPhone and the desktop app show local image attachments as thumbnails above the text field,
with a remove button in the upper-right corner. The entire image fits inside the
preview; its original data remains the attachment sent to the host. On iPhone,
ImageIO creates display-sized thumbnails off the main actor; the desktop app
(`views/thread/ComposerAttachment.tsx`) lets the browser decode the image. Invalid image data falls
back to the filename chip, as do non-image files. Multiple attachments scroll
horizontally and remain removable individually.

On the iPhone a copied image goes in through **Paste** in the + menu; the
composer never suggests the clipboard on its own, and focusing it does not read
or attach clipboard contents. Picking a photo or image file uses the same preview.
A draft containing only attachments shows Send, including when another reply is
running (the message is queued).

The terminal UI keeps its filename row and backspace removal: its text renderer does
not implement terminal image protocols. Sending and limits: [file attachments](../features/file-attachments.md).

While replying to a message, the composer shows its quote on top: `text.bubble`, “text”
on one line and an X that cancels (the terminal UI: `↳ “…”`, esc cancels). The
placeholder reads "Reply…", and leaving the chat cancels the reply. See
[replies](../features/groups-and-replies.md).

## Draft lifetime

Text is saved exactly, including leading/trailing whitespace, blank lines and emoji.
Each chat has its own draft. Switching chats or reopening the client restores its
text. Deleting a bot removes its drafts.

Desktop stores text in a versioned localStorage record per account context and
computer, independently of transcript caches. iOS uses its account-scoped
SharedStore defaults with a separate map per computer. Signing out on iOS erases
these values with the rest of the account data. Retired desktop/iOS stores cannot
write late edits over a newly opened store.

The terminal saves text under `MEXERICA_HOME` (padrão `~/.mexerica`), in a private,
atomically replaced `tui-drafts-<URL hash>.json` file for each host endpoint.
Its existing send flow keeps a draft until delivery succeeds and preserves newer
text if an earlier send finishes later. Save failures leave text in the session
and display an error. Desktop/iOS submission transfers the text to their existing
message/outbox flow and clears only that destination's draft.

Drafts are local to each client, not synchronized between devices. Attachment
bytes are not persisted across restarts.

## Verification

Run desktop type checking and unit tests, the iOS package tests, and host CI checks
as described in [development](development.md#component-checks). Regression tests
cover exact-text restoration, independent destinations, selective clearing,
retired stores, and send acknowledgements arriving after new text is entered.

For a host-free visual check using the actual Electron renderer components:

```sh
cd apps/desktop
node tools/composer-check/serve.mjs
# Open http://localhost:5198/tools/composer-check/
```

The fixture never connects to the host or sends to a real bot. Check typing,
Shift-Return, CJK composition, eight-line overflow, width changes, bot switching,
replies (Toggle reply), reload restoration, offline sending, call/send/stop/interrupt changes,
attachment removal, inspector opening/closing and Reduced Motion. The fixture's
sent-message log and drafts are separate from real account data.
