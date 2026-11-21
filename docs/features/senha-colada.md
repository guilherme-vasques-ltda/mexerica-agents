# Aviso de senha colada

Quando a pessoa cola na caixa de mensagem algo que parece senha, chave ou token, o app esconde o trecho na hora
("•••• escondida"), mostra o aviso "Isto parece uma senha. Mover para o cofre?" acima da caixa e **não envia nada**
enquanto o aviso estiver aberto.

- **Não é senha:** todos os trechos escondidos voltam ao campo, onde estavam, como colados.
- **Mover para o cofre:** os trechos são **descartados** do chat (nunca vão para outra tela nem por IPC) e abre a
  **janela segura** do cofre **vazia**, para guardar um login novo (site, usuário, senha, nome). A pessoa cola a
  senha de novo, direto nela ("Cole aqui a senha que você colou na conversa"). Quem salva é o processo principal, pelo
  `credentialSaveLogin` que o Hub já tem (detalhes e guardas em [connector-credentials.md](connector-credentials.md)).
  **Nunca substitui:** se já existe login com o mesmo site e usuário, o Hub recusa sem alterar nada (senha, id e bots
  ligados intactos) e a janela diz "Já existe um login para este site e usuário. Para trocar a senha, use Extensões →
  Credenciais."
  Enquanto a janela está aberta, o cartão acima da caixa diz "Janela segura aberta"; quando ela salva, mostra **Guardado
  no cofre** (só o estado, sem valor); cancelar não guarda nada e o cartão some. Em conexão pela nuvem (relay) a janela
  não abre e o cartão explica. O gancho é `views/fork/compositor/gancho-cofre.ts` (só o id do computador),
  e o estado do cartão é `compositor/cofre-estado.ts`.

## O que fica bloqueado com o aviso aberto

- Enter e o botão Enviar; na fila: o atalho Shift+Enter, "Mandar agora" e salvar uma mensagem em edição.
- O próprio campo: fica **congelado**. Nenhum caminho o altera: digitar, soltar texto, ditado (um ditado em andamento
  termina ao abrir o aviso), escolher comando `/`, mencionar `@` (os menus somem) e inserção programática (rotina). Só
  "Não é senha" escreve, para o trecho voltar sempre ao texto exato em que foi escondido. Anexar arquivos continua livre.
- Outras entradas de envio ao mesmo bot: o botão **Ao vivo** (conversa e painel de detalhes) some, e
  `BotStore.send` recusa qualquer envio ao bot (chamada em andamento, painel, Ilha, operador de voz, Torre) enquanto a
  pendência estiver aberta (`store/fork-senha-bloqueio.ts`, por computador + bot). Respostas de permissão
  (`respond`/`respondPermission`) continuam livres.
- Uma colagem nova de texto: **não é inserida**. Uma linha no aviso (nos 6 idiomas) pede para resolver o aviso e
  colar de novo. É uma pendência por vez.

## A recusa nunca é silenciosa e nunca perde conteúdo

`BotStore.send` e `BotStore.retry` devolvem `aceito` ou `bloqueado`. Toda recusa mostra o motivo, nos 6 idiomas, **no
próprio cartão do aviso** ("Feche o aviso de senha antes de enviar."): é um aviso, não um erro, sem diálogo "Algo
deu errado" e sem confirmação extra. Quem chamou **preserva o conteúdo**:

- **Reenviar (mensagem que falhou):** o bloqueio é conferido ANTES de tirar a entrada falha; barrado, ela continua na
  conversa com o botão Reenviar, e depois de resolver o aviso o reenvio sai com o mesmo nonce.
- **Ilha:** é outra janela. O envio vai à janela da conversa e **só um aceite explícito dela** fecha a Ilha e limpa o
  campo. Barrado, ou **sem confirmação** (prazo de 4 s, destino ausente, resposta fora do formato, erro do IPC), o
  texto fica e a linha de baixo do nome do bot mostra o motivo ("Feche o aviso…" ou "Não deu para confirmar o envio.
  Seu texto continua aqui; tente de novo."). Sem confirmação NÃO é aceite. Como o envio pode ter saído sem a
  confirmação voltar, a Ilha guarda o nonce da tentativa: tentar de novo o mesmo texto para o mesmo bot reusa o nonce e o
  Hub não duplica. Enquanto espera, a Ilha fica ocupada: sem segundo envio, campo somente leitura, sem trocar de bot;
  resposta atrasada, repetida ou de outro envio (id) é ignorada.
- **Protocolo da Ilha (IPC):** `island:send` só vale vindo do frame principal da Ilha e com o formato validado (ref,
  texto, origem, nonce); `island:sendResult` só vale vindo do frame principal da janela da conversa e só com `aceito`
  ou `bloqueado` (`main/fork-chat-sender.ts`, `main/fork-island-resposta.ts`).
- **Torre (resposta por texto a uma pergunta):** o botão Enviar fica desligado com o motivo à vista; `answer` não
  descarta a entrada falha, não registra nonce e o rascunho fica. Resolvido o aviso, volta a enviar.
- **Chamada de voz:** a fala não é enviada e o motivo aparece no cartão; `Ao vivo` não abre com a pendência aberta.

## Onde está

- Detector local: `apps/desktop/src/shared/fork-senha-colada.ts` (puro, sem rede, sem log, sem ler a área de
  transferência do sistema). `acharSegredosColados` devolve **todos** os trechos: chave privada (todos os blocos) e
  chaves de provedores comuns (OpenAI/Anthropic `sk-`, GitHub `gh[pousr]_`/`github_pat_`, Slack `xox*`/`xapp-`,
  AWS `AKIA`/`ASIA`, Google `AIza`/`ya29.`, GitLab, Stripe `sk_`/`rk_` live/test e `whsec_`, npm, PyPI, Hugging Face,
  SendGrid, Shopify, DigitalOcean, RubyGems, JWT), em qualquer ponto do texto, inclusive atribuições e `.env` de
  várias linhas. Só um texto colado que é **um único trecho sem espaço** passa pela entropia; código e `.env`
  comuns não disparam. Não dispara em URL, e-mail, caminho, hash de commit, telefone nem palavras.
- Quarentena e decisão: `shared/fork-senha-colada-campo.ts`. O aviso: `views/fork/compositor/AvisoSenhaColada.tsx`.
- Fila: `useFila(botId, sessionId, bloqueada)` em `views/fork/conversa/BandejaFila.tsx`.
- Ligação: `views/thread/Composer.tsx`.

## Segurança

A colagem suspeita é posta em quarentena **inteira**: todos os trechos saem do texto antes de qualquer gravação, e só o
resto entra no campo e é marcado como colado. Os trechos vivem só no estado local do Composer; nunca vão para o
rascunho persistido (`composer-drafts`), a fila, o histórico, o espelho, analytics, relatórios de erro nem console.
A pendência pertence a **um editor** (o rascunho ou uma mensagem da fila em edição): ao cancelar ou trocar a
edição, ela é descartada (nunca restaurada em outro lugar). Se o app fechar, ou a caixa trocar de conversa, com o
aviso aberto, os trechos se perdem. Só colagem (Ctrl/⌘+V) passa pelo detector; soltar texto no campo fica
desligado enquanto o aviso está aberto, e fora dele o campo se comporta como antes.
