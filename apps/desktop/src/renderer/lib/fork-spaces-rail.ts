import type { Space, SpacesSnapshot } from '../../shared/fork-spaces.ts'
import { frase, juntas, type Frase } from './fork-i18n/frase.ts'

// D169 (components/SpaceRail; docs/reference/spaces.md): the rail's pure rules over the Hub's list. They decide
// which Hub's spaces it shows, the spaces after Pessoal, a move as the whole order the Hub takes, and what to tell
// the person when the Hub says no. The calls themselves are in store/fork-spaces.ts. D454 (fatia 9c) e D507: o que
// dizer volta como `Frase` (chave e parâmetros), traduzida só na tela.

/** What the rail reads of a computer's store: its hello and the Hub's list. */
export interface RailHub {
  hello: { hostId: string; spacesContract?: number } | null
  spaces: SpacesSnapshot | null
}

/** The Hub whose spaces the rail shows: the selected computer's if it has them, else the only one that does. */
export function railStore<S extends RailHub>(stores: readonly S[], selected: S | null): S | null {
  const ready = stores.filter((s) => s.hello?.spacesContract === 1 && !!s.hello.hostId && !!s.spaces)
  if (selected && ready.includes(selected)) return selected
  return ready.length === 1 ? ready[0]! : null
}

/** The spaces after Pessoal, in the owner's order. */
export function railSpaces(snapshot: SpacesSnapshot | null): Space[] {
  if (!snapshot) return []
  return snapshot.spaces.filter((s) => s.kind === 'space' && s.id !== snapshot.personalSpaceId).sort((a, b) => a.position - b.position)
}

/** Every id with one space moved one place, Pessoal first (the Hub takes only the whole list); null at an end. */
export function movedOrder(snapshot: SpacesSnapshot, id: string, delta: -1 | 1): string[] | null {
  const ids = railSpaces(snapshot).map((s) => s.id)
  const at = ids.indexOf(id)
  const to = at + delta
  if (at < 0 || to < 0 || to >= ids.length) return null
  ;[ids[at], ids[to]] = [ids[to]!, ids[at]!]
  return [snapshot.personalSpaceId, ...ids]
}

/** Where a change starts: the Hub's id from its hello, the live link, its generation and the list's revision. */
export interface OriginMark<C> {
  hostId: string | null
  client: C | null
  epoch: number
  revision: number | null
}

/**
 * Why a change prepared at `origin` (a form when it opened, a move when clicked) must not go out `now`; null when
 * nothing moved. Another Hub in the same store, a new link or generation, or a list newer than the one the draft
 * came from all stop it before the Hub is called: an event or a re-read never lets an old draft overwrite a newer
 * list, and nothing is sent again by itself.
 */
/** D543 E1: ativar/excluir o Berga do Space só existe no Hub com `spaceLifecycleContract`. */
export const spaceLifecycleOk = (hello: { spaceLifecycleContract?: number } | null | undefined) => hello?.spaceLifecycleContract === 1
/** O Pessoal nunca oferece exclusão; só Spaces (`kind: 'space'`). */
export const spaceCanDelete = (space: { kind: string } | null | undefined) => space?.kind === 'space'
/** A confirmação do nome digitado só habilita a exclusão com o nome exato do Space. */
export const spaceDeleteReady = (spaceName: string, typed: string, supported: boolean) => supported && typed === spaceName

export function originRefusal<C>(now: OriginMark<C>, origin: OriginMark<C>): Frase | null {
  if (now.hostId !== origin.hostId) return frase('spaces.origem.outroHub')
  if (!now.client) return frase('spaces.origem.semConexao')
  if (now.client !== origin.client || now.epoch !== origin.epoch) return frase('spaces.origem.conexaoMudou')
  if (now.revision !== origin.revision) return frase('spaces.origem.listaMudou')
  return null
}

/** How a failed change ends: unknown (no answer), a list read again, or a plain refusal. */
export type SpaceFailure = 'unknown' | 'conflict' | 'gone' | 'order' | 'refused'

export function failureOf(e: unknown): SpaceFailure {
  const status = typeof e === 'object' && e !== null ? (e as { status?: unknown }).status : undefined
  if (status === 0) return 'unknown'
  const message = e instanceof Error ? e.message : String(e)
  if (message.includes('spaces_revision_conflict')) return 'conflict'
  if (message.includes('space_not_found')) return 'gone'
  if (message.includes('invalid_space_order')) return 'order'
  return 'refused'
}

/** The Hub's refusals by code, most specific first; the code itself never reaches the screen. */
const REFUSALS: [readonly string[], Frase][] = [
  [['invalid_space_name'], frase('spaces.recusa.nome')],
  [['invalid_tile_letters'], frase('spaces.recusa.letras')],
  [['invalid_tile_icon'], frase('spaces.recusa.icone')],
  [['invalid_tile_color', 'invalid_tile_style', 'invalid_tile_symbol'], frase('spaces.recusa.tile')],
  [['asset_too_large'], frase('spaces.recusa.logoGrande')],
  [['invalid_png_dimensions'], frase('spaces.recusa.logoDimensoes')],
  [['unsupported_png_chunk'], frase('spaces.recusa.pngExtra')],
  [['invalid_png', 'empty_png_data', 'incomplete_png'], frase('spaces.recusa.pngIlegivel')],
  [['active_svg_refused', 'invalid_svg'], frase('spaces.recusa.svg')],
  [['asset_requires_inline_png_or_passive_svg'], frase('spaces.recusa.formato')],
  [['personal_space_immutable', 'immutable_space_field'], frase('spaces.recusa.pessoal')],
  [['remote_setup_unavailable'], frase('spaces.recusa.remoto')],
  [['space_principal_name_fixed'], frase('spaces.recusa.principalFixo')],
  [['space_principal_fixed'], frase('spaces.recusa.principalCamposFixos')],
  [['space_principal_exists', 'space_principal_nonce_conflict'], frase('spaces.recusa.principalExiste')],
  [['space_principal_appearance_invalid', 'space_principal_appearance_style', 'space_principal_appearance_model'], frase('spaces.recusa.principalAparencia')],
  [['space_delete_confirmation_mismatch', 'space_delete_confirm_mismatch', 'invalid_field'], frase('spaces.recusa.confirmacao')],
  [['space_delete_pending'], frase('spaces.recusa.exclusaoPendente')],
  [['space_delete_membership_changed'], frase('spaces.origem.listaMudou')],
  [['space_delete_desktop_only'], frase('spaces.recusa.soComputador')],
  [['hub_context_mismatch', 'cross_hub_response', 'cross_hub_reference', 'cross_space_response'], frase('spaces.recusa.outroHub')],
  [['expected_revision_required', 'spaces_unavailable', 'invalid_patch', 'space_required'], frase('spaces.recusa.atualize')],
]

/**
 * What to say after a failed change. `reread` is whether the Hub's list was read again on the same link after the
 * failure (null when nothing was read): never the same change sent again, so a person decides with the list in view.
 */
export function spaceErrorText(e: unknown, reread: boolean | null): Frase {
  const again = reread ? frase('spaces.erro.relida') : reread === false ? frase('spaces.erro.naoRelida') : null
  switch (failureOf(e)) {
    case 'unknown':
      return reread ? frase('spaces.erro.semRespostaRelida') : frase('spaces.erro.semRespostaNaoRelida')
    case 'conflict':
      return juntas(frase('spaces.erro.conflito'), again, frase('spaces.erro.confiraSalve'))
    case 'gone':
      return juntas(frase('spaces.erro.sumiu'), again)
    case 'order':
      return juntas(frase('spaces.erro.ordem'), again)
    case 'refused': {
      const message = e instanceof Error ? e.message : String(e)
      return REFUSALS.find(([codes]) => codes.some((code) => message.includes(code)))?.[1] ?? frase('spaces.recusa.generica')
    }
  }
}

/** Nome com origem nas listas mistas; o principal do Pessoal segue sem etiqueta. */
export function spaceBotLabel(bot: { id: string; name: string }, snapshot: SpacesSnapshot | null) {
  const space = snapshot?.spaces.find((s) => s.kind === 'space' && s.principalBotId === bot.id)
  return space ? `${bot.name} · ${space.name}` : bot.name
}
