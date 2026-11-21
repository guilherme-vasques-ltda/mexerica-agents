# Desktop styles

How the desktop renderer (`apps/desktop/src/renderer/`) styles its views. Views hold no styling of their own: text, layout and visuals come from the shared primitives and tokens below, and an inline `style` is left only for values computed at runtime.

## Tokens (`styles/tokens.css` + `styles/theme.css`)

The design system's tokens (`canvas`, `surface`, `raised`, `ink`/`ink-2`/`ink-3`, `line`, `hover`, `selected`, `field`, `group`, `accent`/`accent-soft`, glass, wallpaper veil, tiles, sizes, blur) come from `apps/desktop/design/tokens.json`, compiled into `styles/tokens.css` (`node --experimental-strip-types apps/desktop/tools/design-tokens.mjs`; never edit the CSS by hand). Consumers use those canonical names and the scoped roles from the migration; the final migration removes the 28 desktop aliases, including local overrides. The theme is `data-theme` on `<html>`, written by `lib/appearance.ts`; `--motion-hover` is timing and `--hover` is color.

- **Color**: `ink`, `ink-2`, `ink-3`, semantic text foregrounds, `canvas`, `surface`, `raised`, glass, `line`/`line-strong`, `field`/`selected`, `accent`/`on-accent`, `danger-fill`/`on-danger`, `warning`/`on-warning`, `success`, `scrim`, `chip-on` and `shadow-chip`. Shared consumers use scoped `content-ground`, `context-ink-2`, `text-primary`, `text-tertiary`, `control-fill-muted/selected` and `action-fill` with canonical fallbacks. Only Sheet, island and welcome specialize their contracted roles; Sheet keeps `surface=group`, while Dialog/menu portals inherit root. `rgba(var(--foreground-rgb, var(--ink-rgb)), a)` preserves alpha; ink channels are generated from JSON. `surface-opaque` and `diff-removal-ink` resolve at root before local surface/danger overrides. Text's semantic Tone map and generic IconSlot map remain separate. `font-sans/font-mono` keep their stacks. The independent site/marketing namespaces and external `dbe-*` API are unchanged.

As variantes `laranja-claro`/`verde-claro` de Aparência usam tokens canônicos com valores próprios por tema e a mesma preferência local `appearanceAccent`. Não altere os IDs/cores anteriores nem o padrão ao acrescentar uma variante; preserve também as guardas de foreground e vidro.

- **Type ramp**: `--text-large-title` 26, `--text-title` 22, `--text-title2` 17, `--text-title3` 15, `--text-body` 13, `--text-callout` 12, `--text-caption` 11 (points, scaled by the text size preference). `lib/fonts.ts` maps the macOS style names onto it (`headline` and `body` → body, `compactBody` → callout, `subheadline`, `footnote`, `caption`, `caption2`, `compactSecondary` → caption).
- **Radius**: `--radius-xs` 6, `--radius-s` 10, `--radius-m` 12, `--radius-l` 16, `--radius-xl` 22, `--radius-pill`. **Shadow**: `--shadow-s` (resting), `--shadow-m` (floating chrome), `--shadow-l` (menus, panels), `--shadow-xl` (sheets, dialogs).
- **Font:** `--font-sans` follows Ajustes → Aparência → Fonte (`lib/appearance-choices.ts`, applied by `lib/appearance.ts`): DM Sans by default, or Inter, Geist, Manrope, IBM Plex Sans, each in front of `--font-system` (the system stack, SF Pro on the Mac); "Sistema" leaves the token. The five fonts are embedded files (`assets/fonts/`, `styles/fonts.css`, OFL in `THIRD_PARTY.md`), never downloaded at run time.
- **Sidebar tags:** `tag-<cor>-bg`/`tag-<cor>-ink` for azul, jade, ambar, rosa, violeta and grafite (tokens.json). Rows read `--tag-l1`…`--tag-l3` (team height) and `--tag-l0` (Agentes), which `lib/appearance.ts` points at the colors chosen in Ajustes → Barra lateral → Cores das etiquetas (Grafite by default).
- **Glow:** `--glow-respira` (9 s), `--glow-quadros` (`steps(32)`), `--glow-altura`, `--glow-largura`, `--glow-linha` and the themed `--glow-halo-pico/vale/parado` come from the `motion` group of tokens.json; `views/shell/janela-header.css` draws the line and halo under the header (opacity only; still with reduced motion; paused while `lib/glow-visibility.ts` sets `data-janela-oculta` (background) or `data-janela-ociosa` (60 s without pointer, key, wheel or touch input; the first input resumes it). The same idle attribute rests the sleeping bot face and the livre/dormindo Mexeriquinha, so a parked app has no ambient animation keeping the compositor awake).
- **Motion**: `--motion-hover`, `--fade`, `--press`, `--morph`, `--layout`, `--ease`, `--ease-out`, `--spring`. The island has its own (`--island-spring`, `--island-fold`) in `views/island/island.css`. Folding a sidebar section, team or group uses `Fold` (`views/fork/times-sidebar/Fold.tsx`, rules in `lib/fold.ts`): the state changes at once and the body animates from where it is (a second click mid-way reverses), height and opacity in 240 ms out and 180 ms in, clipped with `clip-path`, off with reduced motion. A folded body mounts hidden before it is first opened (three rows per batch, in idle time, or right away on pointer or focus over its title) and stays mounted, hidden, when folded again. A bot's state moves its face (`.face-state` in `components/avatar.css`), transform and opacity only.

## Primitives (`components/`)

| Need | Use |
| --- | --- |
| Text in a type style and color | `<Text size="caption" tone="secondary" weight="semibold" mono as="div">` (`Text.tsx`). With `size` the weight defaults to regular (bold for `headline`); without it, size and weight come from around it. `truncate` ends one line with an ellipsis, `lines={n}` clamps to n lines, `grow` takes a stack's free space, `tabular` lines digits up |
| A row / a column | `<HStack gap={8}>` (centered vertically; `align="stretch"` or `"baseline"` otherwise) / `<VStack gap={8}>` (`Stack.tsx`); `grow` takes the free space, `shrink` lets it shrink so text inside can truncate; `justify`, `wrap`, `as` |
| Space between stack items | `<Spacer />`, `<Spacer min={8} />` |
| A fixed-width column that centers an icon, so labels line up | `<IconSlot width={18} tone="secondary">` |
| A row's title with a note under it | `RowLabel` (`Controls.tsx`) |
| Reorder by dragging | `pointerDrag` (`lib/pointer-drag.ts`), not native drag and drop: the pointer holds a closed hand, and a roster row lifts as a smaller, translucent copy that settles into its slot |
| A sheet's content | `<SheetPage>` with a `ModalHeader` (`Overlay.tsx`); `ModalHeader` takes an `icon` (an SF Symbol name on a tile, or a logo element), a title that wraps to two lines, a `subtitle` (up to two lines, the rest in its tooltip), and `onClose` when it heads a panel instead of a modal. A `Sheet` without `height` fits its content up to `maxHeight` (and the window); `FormSheet` does that unless `fixed` (content that fills the card or slides in pages that do: a terminal, an agent sheet) |
| One line with an ellipsis on an element that isn't `Text` | `.truncate` |
| Controls | `Controls.tsx`, `Overlay.tsx`, `Icon.tsx`; `Hairline`, `Pill`, `Switch` and `ProgressBar` take a `className` for a view's own sizing, and a panel beside an anchor is placed with `anchorPlacement` |

## Where a style goes

1. Text styling is `Text` props; layout is `HStack`/`VStack`/`Spacer`.
2. Anything else static (padding, background, radius, size, position, borders) is a class in the view's CSS file, named after the view (`.bot-row`, `.unread-badge`), written with the tokens above.
3. No static inline styles. Inline `style` only for values computed at runtime: a size or color from props or data, a measured width, a transform that follows state. Keep the static part of that element in its class.
4. A pattern repeated across views becomes a primitive or a shared class here; don't copy it.
5. Anything clickable shows the pointing hand (a `:where` rule in `theme.css`, so a view's own `cursor` wins); text fields show the text cursor.


## Toques Mexerica

Os SVGs/geometria ficam em `components/toques-mexerica/`; o CSS optativo fica em
`views/fork/toques-mexerica/toques.css`. Controles compartilhados não importam de
views. A preferência validada e seu hook ficam em `lib/toques-mexerica.ts`;
`appearance.ts` escreve `data-toques-mexerica` antes de pintar. O interruptor
persiste e sincroniza com `pref`.

`Spinner` mantém o arco original girando, com fiapos decorativos ao redor.
`animation-visibility.ts` pausa as Web Animations fora da janela, em documento
oculto ou ancestral invisível, somente com Toques ligados. Compartilha um
IntersectionObserver/MutationObserver e listeners por documento, removidos
quando o último arco é desligado/desmontado. O wrapper conserva o tamanho
original; fiapos transbordam 4px sem participar do layout.
A pausa não escreve estilos ou atributos. Desligado, o SVG fica sem wrapper.

Pausa ambiente (`iniciarPausaAmbiente`, ligada em `inicio.tsx` em toda janela): o mesmo serviço adota
qualquer animação CSS infinita que começar no documento (avatares, `ldot`, pontos de trabalho, giros, halo,
inclusive as de `::before`/`::after`) e a pausa com o documento oculto, a janela sem foco (`blur`/`hasFocus`)
ou o elemento fora da vista; retoma ao voltar. Animação finita não é tocada, movimento reduzido continua no CSS
(sem animação, nada a pausar) e a pausa também não escreve estilos ou atributos. Motivo: um laço infinito
sob o vidro (`backdrop-filter`) obriga o compositor a redesenhar o vidro todo a cada quadro.
`ProgressBar` exige `taskProgress` para o gomo: somente o download da atualização
adere. Os demais chamadores, inclusive UsageBar/cotas, preservam DOM e geometria.
Só a tarefa decorada reserva 18 px; o gomo anda por transform, sem animar inset.
Limita valor a 0…1 e omite o gomo/valor ARIA
em NaN/infinito; `fill` é respeitado. RTL fica sob o mesmo gate do interruptor.
Vazios aderem explicitamente com `toque`, nunca por nome do ícone; desligados,
retornam o fallback original sem wrapper. Marca estática e ações são preservadas.

Não se escreve `box-shadow` em `:focus-visible`. Calor dos botões/cartões
exclui esse estado, preservando os anéis canônicos inclusive quando desenhados
com sombra. O compositor soma calor no seu contêiner, sem substituir o foco
do controle. Mais contraste/cores forçadas suprimem decoração e calor; movimento
reduzido corta animações e transições. Os outros seis easter eggs ficam fora.

O hover é limitado a seletores explícitos: faixa (incluindo tiles), linhas clicáveis de lista (roster/Times, Spaces,
membros/sessões e roles option/listitem), icon-button neutro e menu-row. O papel global `--hover`
não muda. Fundo com 18% de `var(--accent)` sobre `var(--surface)`, borda interna
1 px sem alterar medidas e tinta calculada por `toquesHoverInk` usando o mesmo
`legibleOn` existente, a 4,5:1 sobre a composição real. A base opaca é exclusiva
do item em hover para proteger texto/ícone sobre fotos/vidro extremos. O accent,
pisos e opacidade das demais superfícies não mudam. A borda usa outline interno
apenas fora de `:focus-visible`; o foco preserva seu anel e sua sombra originais.

Compositor compacto: largura não amplia fonte/ícones. Use `--conversation`, `--scale` e espaçamentos existentes; não ajuste zoom ou preferência para compensar CSS.

Ajuste do compositor: o padding do campo caiu de `sp-8` para
`sp-4`, baixando a pílula sem mudar largura nem fonte. `.composer-surface:hover`
soma `drop-shadow` discreto derivado do accent, só em `hover:hover`, sem
preferência de contraste e sem cores forçadas; não desloca layout e não compete
com `:focus-visible`. O indicador de contexto não abre mais por hover/foco —
é ativação explícita.
