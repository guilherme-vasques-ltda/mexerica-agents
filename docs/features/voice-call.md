# Chamada de voz: botão e atalho

Há uma só chamada de voz em tempo real (`CallView`). O botão do compositor e o atalho do teclado são entradas para
ela: o atalho não cria sessão própria, não usa outro provedor e não escuta nada antes de a chamada começar. A regra
única de quando o atalho abre, encerra ou recusa está em `apps/desktop/src/shared/fork-voz-entrada.ts`.

## Atalho global (desktop)

Em **Ajustes → Voz → Ao vivo → Abrir a chamada**. Desligado de fábrica e sem combinação padrão: a pessoa liga e grava uma.
É uma preferência deste aparelho.

- O processo principal registra o atalho do sistema (`globalShortcut`). Registrar **não abre o microfone**: o sistema só
  avisa que as teclas foram apertadas. O microfone abre quando a chamada começa, como no botão.
- Funciona com o app em segundo plano (traz a janela à frente).
- Chamada parada: abre a chamada na **conversa principal do bot que está à vista**. Não abre em canal (grupo), em aba de
  sessão, com Ajustes, Rotinas ou outra tela na frente, com diálogo aberto, com o aviso de senha colada aberto ou com a voz
  indisponível. Nesses casos nada liga e a conversa mostra o motivo por 6 segundos.
- Já em chamada: o atalho **encerra** pelo mesmo caminho do botão vermelho (libera o microfone, registra a chamada uma vez,
  para os relógios). O botão não abre uma segunda sessão.
- Pressões repetidas (tecla segurada, duplo toque) valem como uma só dentro de 700 ms.
- Conflito de teclas: exige dois modificadores (ou um com F1 a F24) e recusa combinações que a Mexerica, a Ilha ou o sistema
  já usam e as que outro app já registrou. O motivo aparece em Ajustes; se falhar, nada fica registrado.
- O atalho é solto ao desligar, ao trocar a combinação e ao fechar o app. Web e demo não registram atalho.

## Limites

- Testado com um `globalShortcut` falso: a lógica está provada, o comportamento do sistema (macOS, Windows, X11, Wayland)
  não. No Wayland o registro pode ser negado; o app então mostra "indisponível".
- **Não existe palavra de ativação.** Não há detector local incorporado, então nada fica escutando e a opção não existe.
- Sem conversa montada na janela, o atalho não faz nada.
- A chamada é a conversa em tempo real, não o Ditado (escrita por turnos).
