# Chat messages

How a bot's work turns into what the user reads, copied from Grok Bot.

## Markdown links

On desktop, Markdown links in bot replies only open `http:` and `https:` in the
default browser, and `mailto:` in the default email client. These links are
underlined in both light and dark themes. All other schemes, including `file:`,
`vscode:` and `x-apple.systempreferences:`, are blocked and appear as plain text,
without a popup. Hovering a link or its blocked label shows the destination URL.
Websites never load inside the app window.

iPhone links remain underlined and keep the system link interaction. The terminal
already underlines Markdown link labels; it has no pointer tooltip or link-opening
action.

## Message layout

On desktop, a message is a stack of Markdown blocks, and the space between them
shows its structure. The gaps between lines, all scaled with the chosen text
size, are:

- unchanged between items of one list (nested ones too), from a heading to its
  content, and from a sentence to the list it introduces;
- 10px between paragraphs;
- 14px after the end of a list before text, a quote or code;
- 20px before a `#` or `##` heading and 16px before `###` or lower, never above the
  first block.

Tables and rules keep their own margins. A `###` heading is never smaller than the
body text. A one-block message looks the same as before. The layout is CSS only:
nothing is added to the text and stored messages are untouched.

The `chat` instructions (`INSTRUCTIONS` in `host/src/chat/outbox.rs`) ask bots to:

- keep short messages plain;
- structure longer ones with a short `##` heading per part, lists for steps or
  parallel items, and a blank line between blocks;
- use emojis sparingly and only where they add meaning: a few per message at most,
  ⚠️ only for a real risk, never in place of words.

Emojis are never added to old messages.

## Messages are sent, not streamed

- Every bot gets the built-in `chat` MCP server (`codync-host mcp chat`,
  `host/src/chat/outbox.rs`) with `send_message {text}` and `send_file {path, name?}`. In the bot's own chat, each call is one chat bubble, added to the turn's lane at once as an `agent` entry with
  `final: true`. A turn can send several.
- The agent's own reply text (`agent_message_chunk`) stays in the trace when the turn sent
  text through `send_message`. A turn that sent nothing (an agent that ignores the tool, or has no MCP) falls back to
  the old rule: its last text becomes the turn's final message, unless that text is `(pass)`: a message that
  needs neither words nor a reaction gets nothing, as in a group.
- File cards are separate final entries. Sharing a file preserves an intended final text
  reply; a files-only turn uses its filename for the completion summary. See
  [file attachments](file-attachments.md#files-from-a-bot).
- Group room turns, `ask_bot` requests and routines reject `send_message` and `send_file`: their reply is the
  turn's last text, as before (`chat/group.rs`, `chat/team.rs`, `routines`).
- An `ask_bot` reply stays in the recipient's trace and the request notice; it does
  not also become a message to the user. The requesting bot uses it to answer the user.
  Independent `message_bot` turns report to the user in the recipient's main chat, through
  `send_message` or the final-text fallback, and do not add that report to the request notice.
- The rules reach the agent twice: the MCP server's instructions, and the frozen instruction
  snapshot (`chat/context.rs`).
- Na Mexerica, nenhum bot pode alterar o próprio nome ou instruções. `set_name` e
  `set_instructions` não são anunciadas pelo MCP e chamadas antigas são recusadas
  também no executor. Um pedido no chat não concede autoria humana: o bot orienta
  o usuário a abrir Configurações. A nomeação inicial feita pelo serviço do Hub
  continua no fluxo de criação. Veja [edição de bots](editar-bot.md).
- A bot-to-bot exchange (`ask_bot`, `message_bot`) is one compact notice row, "Messaged Owen" /
  "Message from Egan". Its text lives only in the bot conversation sheet it opens; follow-ups
  are normal bubbles. Consecutive exchanges with the same peer in the main chat (either
  direction, trace entries ignored) collapse into one "3 messages with Owen" row ([bot collaboration](bot-collaboration.md#notice-data)).
- Clients never stream text into a bubble: the chat shows user messages, `final` agent entries,
  permission cards and notices (`isChat`). Nothing pops in and out while a turn runs.

## Bot reactions

- The `chat` server's `react {emoji, to?}` (`host/src/chat/reactions.rs`) goes through the bot's actor,
  which puts one emoji on the message the turn answers: in its own chat, the user's message that
  started the turn; in a group, the newest message its prompt showed from `to` ("user" or a participant's
  name), or from anyone else. Routines and bot requests can't react. Each bot keeps one reaction per
  message; a new emoji replaces it.
- Stored as `data.botReactions: [{bot, emoji}]` beside the user's own `data.reactions`. The first one also
  records `data.contentRev`, the rev before it, which the unread count uses, so a read message stays read.
- Reactions are rare. The instructions allow them only where a person would react (thanks, a plain OK, big
  news, a real joke), never on a question or a request, and the host refuses a bot's reaction while it
  reacted to another of the chat's last 6 messages (`reacted_lately`); a turn whose only answer was that
  refused reaction says nothing and sends no push. A reaction can be the whole reply: a turn that reacted and sent no message
  doesn't fall back to its last text, and its push says "Reacted ❤️ to your message".
- Clients show each bot emoji as a chip with the avatars of the bots that chose it (up to three), after the
  user's own chips; it isn't tappable, and a bot that was deleted drops out. The terminal UI lists
  `❤️ Ann, Ben` after the user's reactions.

## While it works

The working line under the last message shows the bot's activity; composing a `send_message`
reads as **Typing…** (`activity()` in `agent/bot.rs`). Thoughts and tool calls stay in the
Full conversation sheet.

## Motion and scrolling

- A message arriving while the list follows the newest pops in: 0.24 s,
  `cubic-bezier(.23, 1, .32, 1)`, from 12 pt lower at 94 % scale (iPhone: `ConversationLayout` in
  `ConversationList.swift`; desktop: `.row-in` in `thread.css`; reduced motion: a 0.12 s fade).
- The list stays pinned to the bottom while the reader is there; scrolling up releases it and the
  jump pill brings it back (see below). Sending always returns to the bottom.

## Navegação e mensagens novas (desktop)

- **Barrinha (margem esquerda):** uma marca por mensagem (a sua com 22 px, a do bot com 14 px). As marcas das
  linhas na tela ficam realçadas; o hover aumenta a marca e as vizinhas; clicar ou arrastar pelo trilho pula. A
  prévia mostra a hora, a 1ª linha e, numa mensagem sua, a resposta do bot. A variante B é provisória:
  `TIMELINE_VARIANT` em `views/thread/chat-timeline.ts` é a única chave para a variante A (marcas só nas suas).
- **"Ir para o fim":** a pílula aparece só fora do fim e diz "Ir para o fim" ou "N novas" (ponto em accent), com as
  novas que ainda não passaram pela tela, abaixo dela. Mensagem nova com você lá em cima não move a lista.
- **Linha "Novas":**
  - O app guarda, por aparelho, o `createdAt` da última resposta do bot lida em sequência em cada bot e sessão
    (`prefs.chatReadMarks`). Não guarda texto da conversa e nada vai ao Hub. O mapa tem teto de 300 entradas e
    esquece as sessões que não estão mais abertas.
  - Ao abrir, as respostas do bot depois da marca são novas. Sem marca, a principal parte do `bot.unread` do Hub e
    a extra parte de "tudo lido".
  - A linha, as marcas em accent e o "N novas" leem o mesmo conjunto, congelado durante a visita. As suas
    mensagens nunca contam.
  - O cache e o catch-up do Hub (entradas com `rev` até o do `hello` do stream) entram como carga inicial. A chegada
    ao vivo (`rev` acima) só é nova com você fora do fim. Nenhuma regra compara horários do Hub com o relógio local.
  - A marca só avança sobre respostas que estiveram na tela, em sequência e sem lacuna: o que foi pulado pela
    barrinha ou pelo "Ir para o fim" continua novo. Ela é gravada durante a visita (no máximo 1 vez por segundo) e
    ao sair ou fechar a janela. A linha na tela só muda na próxima abertura.
  - Quando as novas não cabem numa tela, a conversa abre na linha; senão, abre no fim com a linha à vista.
  - Outro aparelho tem a sua marca.
- iPhone: earlier messages load when the top of the chat comes into view, so a short chat never
  shows a lasting spinner. Pulling down past the top sweeps in the `arrow.clockwise` symbol; letting
  go of a full sweep (`PullToRefresh.swift`) reconnects to the computer while the chat waits just
  below the turning arrow, then eases back. A pull that isn't full refreshes nothing. The desktop
  reconnects from the computer menu instead.
- The terminal UI lists the same messages; its steps line goes under the turn's last message.
