# Avatares dos bots no iPhone

O iPhone mostra o mesmo avatar do desktop: a **Mexeriquinha**. Antes ele desenhava a matriz de pontos herdada do Codync.

## Causa

- Desktop: `apps/desktop/src/shared/fork-mexeriquinha-config.ts` (`avatarDoBot` e `normalizarAvatarDoBot`) escolhe o avatar a partir de `bot.mexeriquinha` ou, se faltar, de `bot.avatarConfig`. O componente é `apps/desktop/src/renderer/fork/avatars/Avatar.tsx` (`BotAvatar`).
- Hub: `host/src/hub/sync.rs` serializa o `BotConfig` inteiro. Cada bot chega com `avatarConfig`, `mexeriquinha` (quando existe), `avatarColor` e `avatarShape`. Os dois últimos são só uma aproximação para clientes antigos (`host/src/fork_avatars/mod.rs`, `legacy_avatar`).
- iPhone (antes): `Models.swift` lia só `avatarColor` e `avatarShape`, e `CharacterAvatar.swift` desenhava pontos com eles.

## Regra (igual à do desktop)

1. `mexeriquinha` válida vale, se não houver `avatarConfig` válido ou se este for a projeção dela.
2. Senão, vale o `avatarConfig` válido, aproximado para a Mexeriquinha Simples (cor mais próxima, óculos, acessório, gravata, selo).
3. Senão, o mascote padrão: Mexeriquinha laranja, sem roupa nem acessório.

Código: `apps/mobile/ios/Kit/Sources/CodyncKit/Models/BotAvatar.swift`. Desenho: `Design/MexeriquinhaView.swift` (porta de `render-simples.ts`, com os mesmos textos SVG lidos por `Design/SVGPath.swift`). O estado (livre, trabalhando, precisa de você, erro, dormindo) segue `estado.ts`, e o desenho é parado. `CharacterAvatar(bot:)` usa tudo isso, então lista, conversa, detalhes, grupos, widgets, ilha dinâmica e notificações mudam juntos.

## Assets

`apps/mobile/ios/Kit/Sources/CodyncKit/Resources/Mexeriquinhas.xcassets`: 22 conjuntos de feltro × 5 quadros (livre, trabalhando, dormindo, precisa de resposta, erro), 256 px, como Data Sets WebP.

- Origem: os mesmos quadros WebP da [biblioteca de Mexeriquinhas do desktop](../../apps/desktop/src/renderer/lib/mexeriquinhas/), em `assets/<conjunto>/256/<quadro>.webp`. Os arquivos foram copiados sem alteração.
- Licença: ilustrações originais do projeto, sob Apache-2.0 (veja a [licença do projeto](../../LICENSE) e o [guia de Mexeriquinhas](../../apps/desktop/src/renderer/lib/mexeriquinhas/README.md)).

## O que falta

- **Conjuntos 3D gerados pelo Hub (v3, `assetId`)**: o desktop baixa os quadros por `mxq://conjunto/<assetId>/...`, um transporte do Electron. O iPhone não tem equivalente e mostra o conjunto de reserva (`fallbackConjuntoId`). Falta um comando no Hub, pelo relay, que entregue esses quadros.
- **Notificações e Live Activity**: o push do Hub (`host/src/remote/push.rs`, `faces`) manda só `avatarColor` e `avatarShape`. O app completa com a cópia do roster (`NotificationService.swift`). Para bot novo, ou sem cópia, aparece o mascote padrão. Falta o Hub incluir `mexeriquinha` e `avatarConfig` em `faces`, atento ao limite de tamanho do push.
- **Bot sem `avatarConfig` (Hub antigo)**: o desktop tenta adivinhar a função pelo nome (`configForRole`, 30 funções). Isso não foi portado; o iPhone usa o mascote padrão.
- Não portado: animação (o desktop anima), textura de pontinhos do corpo (5% de opacidade) e o limite de 1024 bytes do campo.
- O avatar é editado no app do Mac (já era assim).
