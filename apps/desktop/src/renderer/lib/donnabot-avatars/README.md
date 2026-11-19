# Mexerica — avatares (`@donnabot/avatars`)

Biblioteca própria de avatares animados dos bots da Mexerica. React + TypeScript, zero dependência além do React.

- 13 formatos, 7 tipos de olho, 24 cores (ou qualquer hex)
- 22 acessórios: cabeça, rosto, orelhas, queixo, pescoço, bochechas
- 36 selos (add-on no canto), em 3 estilos
- 12 estados animados + "vida automática" (pisca e olha em volta sozinho)
- 30 funções prontas (Donna, Amazon, Meta Ads, SDR, Psicóloga…)
- Editor completo pronto pra tela de perfil do bot

Abra `demo/index.html` no navegador pra ver tudo funcionando. Não precisa de build.

---

Documento adaptado em 28/10/2026; o nome do pacote, os exemplos e os arquivos `src/` foram preservados. A integração no produto passa pelos adaptadores existentes, sem copiar outra biblioteca por causa deste guia.

## 1. Instalar

Copie a pasta inteira para o seu projeto. Por exemplo, para `packages/avatars` ou `src/lib/donnabot-avatars`.

Precisa só de React 18+. Os arquivos são `.ts/.tsx` puros, e qualquer bundler (Vite, Next, Electron + Vite, esbuild) compila direto.

```ts
import { BotFace, AvatarEditor, useBotMood, ROLES, configFromRole } from './lib/donnabot-avatars/src';
```

As animações são injetadas sozinhas no `<head>`. Se o seu app tiver CSP que bloqueia `<style>` inline, importe `src/styles/botface.css`.

## 2. Mostrar um avatar

```tsx
<BotFace config={bot.avatarConfig} size={44} mood="idle" label="Donna, assistente geral" />
```

| prop | tipo | padrão | o que faz |
|---|---|---|---|
| `config` | `AvatarConfig` | obrigatório | aparência (salve isso no banco) |
| `mood` | `Mood` | `'idle'` | estado visual |
| `size` | `number` | `48` | lado em px; o desenho escala inteiro |
| `autoLife` | `boolean` | `true` | piscadas e olhadas aleatórias |
| `label` | `string` | `'Avatar do bot'` | texto acessível |

Tamanhos recomendados: **24–32** em listas compactas, **44** na sidebar, **64–96** em cards e **200+** em destaque.

## 3. Estados (ligar no runtime do bot)

Use `useBotMood` para controlar o estado. Ele separa o que é contínuo do que é reação rápida.

```tsx
const { mood, setBase, play } = useBotMood('idle');
<BotFace config={cfg} mood={mood} />
```

| Evento na Mexerica | Chamada | Visual |
|---|---|---|
| bot parado | `setBase('idle')` | respira, pisca, olha em volta |
| usuário digitando / falando | `setBase('listen')` | inclina a cabeça, olhos suaves, ondas de som |
| gerando resposta | `setBase('think')` | olha pra cima, "…" |
| rodando ferramenta / automação | `setBase('work')` | olhos focados, scanner no visor |
| bot pausado / desligado | `setBase('sleep')` | olhos fechados, "zzz", cores apagadas |
| mensagem recebida | `play('blink')` | piscada |
| buscando contexto | `play('left')` / `play('right')` / `play('up')` | olha pro lado |
| tarefa concluída | `play('happy')` | olhos sorrindo, pulinho |
| erro / notificação nova | `play('surprise')` | olhos arregalados, pulo |
| bot reativado | `play('wake')` | acorda, pisca, olha em volta e volta pra `idle` |

Reações de `play()` duram de 0,2 a 3 s e voltam sozinhas para a base.

## 4. Salvar a aparência

`AvatarConfig` é um objeto pequeno e serializável. O schema está em `schema/avatar-config.schema.json`.

```json
{ "shape": "oval", "color": "#9F1239", "eye": "cat", "head": "none", "face": "none",
  "ears": "hoops", "chin": "none", "neck": "necklace", "cheeks": "none",
  "badge": "sparkle", "badgeStyle": "light" }
```

Sugestão: uma coluna `avatar_config JSONB` na tabela de bots. Ao criar um bot por função, use `configFromRole(preset)`.

Os 30 presets também estão em `presets/roles.json` (nome, função, área, descrição e aparência), caso o backend precise.

## 5. Tela de edição (Perfil → Avatar)

```tsx
const [cfg, setCfg] = useState(bot.avatarConfig);
<AvatarEditor
  value={cfg}
  onChange={(next) => { setCfg(next); saveBot({ avatarConfig: next }); }}
  onPickRole={(role) => renameBot(role.name)}   // opcional
  hideTabs={[]}                                  // ex.: ['role'] para esconder presets
/>
```

O tema vem de variáveis CSS com fallback escuro: `--dbe-surface`, `--dbe-tile`, `--dbe-tile-active`, `--dbe-border`, `--dbe-text`, `--dbe-muted`, `--dbe-ring`, `--dbe-seg-bg`, `--dbe-seg-active` e `--dbe-seg-active-text`.

## 6. Sem React?

`buildFace(config, mood, size, life)` (em `src/core/model.ts`) devolve o desenho como dados: peças com posição, cor e transform em `em`. Dá pra renderizar com Vue, Svelte ou DOM puro usando a mesma estrutura do `BotFace.tsx`.

## Estrutura

```
src/
  core/types.ts      tipos (AvatarConfig, Mood, …)
  core/data.ts       formatos, olhos, ícones, paleta, 30 presets, rótulos pt-BR
  core/model.ts      monta o desenho (sem framework)
  react/BotFace.tsx  componente do avatar
  react/hooks.ts     useBotMood, useBotLife
  react/AvatarEditor.tsx  editor com abas
  react/styles.ts    keyframes (injeção automática)
  styles/botface.css mesmos keyframes, se preferir importar
presets/roles.json   30 funções
presets/palette.json 24 cores
schema/              JSON Schema do AvatarConfig
demo/                index.html + demo.js (abre direto)
```

## Notas

- Acessibilidade: `role="img"` + `aria-label`, botões reais no editor, e `prefers-reduced-motion` desliga as animações.
- Performance: cada avatar é ~30 divs e anima por CSS. Os timers da vida automática são por avatar e só disparam a cada 2–4 s. Em listas com mais de 50 avatares, passe `autoLife={false}` nos que estão fora da tela.
- Marcas: os selos de Amazon e Meta Ads usam ícones genéricos (caixa e alvo), sem logos.

## Procedência e integração na Mexerica

Biblioteca própria fornecida pelo dono da Mexerica em 21/10/2026 (D115).
`src/` preservado integralmente; adaptações do Hub, persistência, humores e UI
ficam fora desta pasta. Leia o guia de integração do produto.
O Hub incorpora o schema e os presets diretamente destes arquivos, sem rede.
