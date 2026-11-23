import { FALLBACK_LANGUAGE, sugerirIdioma } from '../../shared/fork-idioma.ts'
import { superficieIdioma, tradutor } from '../../renderer/lib/fork-i18n/index.ts'

export type RemoteViewStatus =
  | 'unconfigured'
  | 'loading'
  | 'signed_out'
  | 'unavailable'
  | 'empty'
  | 'switching'
  | 'active'
  | 'hub_offline'
  | 'revoked'

export type RemoteWorkspace =
  | { readonly id: string; readonly kind: 'personal'; readonly name?: string }
  | { readonly id: string; readonly kind: 'team'; readonly name: string }

export interface RemoteViewState {
  status: RemoteViewStatus
  workspaces: readonly RemoteWorkspace[]
  activeId: string | null
  signedIn?: boolean
}

export interface RemoteViewActions {
  signIn(): void
  signOut(): void
  select(id: string): void
  retry(): void
}

const CHAVE_STATUS = {
  unconfigured: 'web.login.indisponivel',
  loading: 'fundo.carregando',
  signed_out: null,
  unavailable: 'web.login.local.erro.geral',
  empty: null,
  switching: 'fundo.carregando',
  active: null,
  hub_offline: 'botCreate.erroCriacao.offline',
  revoked: 'conversa.arquivar.falha.notAllowed',
} as const satisfies Record<RemoteViewStatus, 'fundo.espaco.indisponivel' | 'fundo.carregando' | 'web.login.local.erro.geral' | 'botCreate.erroCriacao.offline' | 'conversa.arquivar.falha.notAllowed' | null>

const CHAVE_TIPO = {
  personal: 'ajustes.grupo.pessoal',
  team: 'botCreate.equipe',
} as const satisfies Record<RemoteWorkspace['kind'], 'ajustes.grupo.pessoal' | 'botCreate.equipe'>

export function mountRemoteView(root: HTMLElement, actions: RemoteViewActions): (state: RemoteViewState) => void {
  const idioma = sugerirIdioma(typeof navigator === 'undefined' ? [] : navigator.languages) ?? FALLBACK_LANGUAGE
  const superficie = superficieIdioma(idioma)
  const texto = tradutor(idioma)
  root.classList.add('remote-view')
  root.lang = superficie.lang
  root.dir = superficie.dir

  const title = document.createElement('h2')
  title.className = 'rv-title'
  title.tabIndex = -1
  title.textContent = texto('bots.editor.espaco')

  const spinner = document.createElement('span')
  spinner.className = 'rv-spinner'
  spinner.setAttribute('aria-hidden', 'true')
  const statusText = document.createElement('p')
  statusText.className = 'rv-status'
  statusText.setAttribute('role', 'status')
  const statusRow = document.createElement('div')
  statusRow.className = 'rv-status-row'
  statusRow.append(spinner, statusText)

  const placeholder = document.createElement('option')
  placeholder.textContent = texto('bots.editor.espaco')
  placeholder.value = ''
  placeholder.disabled = true
  const select = document.createElement('select')
  select.className = 'rv-select'
  select.append(placeholder)
  select.addEventListener('change', () => {
    if (select.value && select.value !== selectedId) actions.select(select.value)
  })
  const selectLabel = document.createElement('span')
  selectLabel.className = 'rv-label'
  selectLabel.textContent = texto('bots.editor.espaco')
  const selectField = document.createElement('label')
  selectField.className = 'rv-field'
  selectField.append(selectLabel, select)
  const workspaceBox = document.createElement('div')
  workspaceBox.className = 'rv-workspace'
  workspaceBox.append(selectField)

  const button = (label: string, onClick: () => void, variant = '') => {
    const node = document.createElement('button')
    node.type = 'button'
    node.className = `rv-button ${variant}`.trim()
    node.textContent = label
    node.addEventListener('click', onClick)
    return node
  }
  const signInButton = button(texto('web.login.entrar'), () => actions.signIn(), 'rv-primary')
  const retryButton = button(texto('contas.tentarDeNovo'), () => actions.retry(), 'rv-primary')
  const signOutButton = button(texto('conta.linha.sair'), () => actions.signOut())
  const actionsBox = document.createElement('div')
  actionsBox.className = 'rv-actions'
  actionsBox.append(signInButton, retryButton, signOutButton)

  root.replaceChildren(title, statusRow, workspaceBox, actionsBox)

  let selectedId: string | null = null
  let optionsKey = ''

  return (state) => {
    const { status, workspaces, signedIn } = state
    const selected = status === 'revoked' ? null : workspaces.find((workspace) => workspace.id === state.activeId) ?? null
    selectedId = selected?.id ?? null
    const busy = status === 'loading' || status === 'switching'
    const showWorkspaces = workspaces.length > 0 && status !== 'unconfigured' && status !== 'loading' && status !== 'signed_out'

    root.dataset.status = status
    root.setAttribute('aria-busy', String(busy))
    spinner.hidden = !busy
    const chave = CHAVE_STATUS[status]
    statusText.textContent = chave === null ? '' : texto(chave)

    workspaceBox.hidden = !showWorkspaces
    if (showWorkspaces) {
      const key = JSON.stringify(workspaces.map(({ id, kind, name }) => [id, kind, name ?? null]))
      if (key !== optionsKey) {
        optionsKey = key
        select.replaceChildren(
          placeholder,
          ...workspaces.map((workspace) => {
            const option = document.createElement('option')
            option.value = workspace.id
            option.textContent = workspace.name ?? texto(CHAVE_TIPO[workspace.kind])
            return option
          }),
        )
      }
      select.value = selectedId ?? ''
      select.setAttribute('aria-busy', String(status === 'switching'))
    }

    signInButton.hidden = status !== 'signed_out'
    retryButton.hidden = status !== 'hub_offline'
    signOutButton.hidden = signedIn !== true
    actionsBox.hidden = signInButton.hidden && retryButton.hidden && signOutButton.hidden

    const focused = root.ownerDocument.activeElement
    if (focused instanceof HTMLElement && root.contains(focused) && focused.closest('[hidden]')) title.focus()
  }
}
