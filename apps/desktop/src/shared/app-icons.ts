// MX-9: o seletor de ícone do app é só dados. O renderer manda o `id`; o main resolve o PNG em
// resources/app-icons pelo manifesto empacotado. A fonte única é o pacote final
// brand/propostas-icone-app/pacote-opcoes-r2-r3-20261110 @ e91caed5 (manifesto.json v1): os `id`
// são os estáveis do pacote e cada escolha leva UM PNG 256×256, o `previa256` byte a byte, usado
// na grade e na aplicação (o teste confere o SHA256 contra o SHA256SUMS do pacote). O `padrao.png`
// é só a miniatura da grade; aplicar o padrão usa o `resources/icon.png` real, resolvido no main.
// Uma opção sai ou entra só com dados: linha aqui, o PNG e a chave `aparencia.icone.opcoes.*` nos seis idiomas.
export const APP_ICON_DEFAULT_ID = 'padrao' as const
export const APP_ICON_DEFAULT = {
  id: APP_ICON_DEFAULT_ID,
  numero: '00',
  file: 'padrao.png',
  nameKey: 'aparencia.icone.padrao',
} as const

export const APP_ICON_OPTIONS = [
  { id: 'icone-01-laranja-close', numero: '01', file: 'icone-01-laranja-close.png', nameKey: 'aparencia.icone.opcoes.laranjaClose' },
  { id: 'icone-02-laranja-inteiro', numero: '02', file: 'icone-02-laranja-inteiro.png', nameKey: 'aparencia.icone.opcoes.laranjaInteiro' },
  { id: 'icone-03-laranja-degrade-feliz', numero: '03', file: 'icone-03-laranja-degrade-feliz.png', nameKey: 'aparencia.icone.opcoes.laranjaDegradeFeliz' },
  { id: 'icone-04-verde-close', numero: '04', file: 'icone-04-verde-close.png', nameKey: 'aparencia.icone.opcoes.verdeClose' },
  { id: 'icone-05-verde-piscando', numero: '05', file: 'icone-05-verde-piscando.png', nameKey: 'aparencia.icone.opcoes.verdePiscando' },
  { id: 'icone-06-creme-inteiro', numero: '06', file: 'icone-06-creme-inteiro.png', nameKey: 'aparencia.icone.opcoes.cremeInteiro' },
  { id: 'icone-07-branco-piscando', numero: '07', file: 'icone-07-branco-piscando.png', nameKey: 'aparencia.icone.opcoes.brancoPiscando' },
  { id: 'icone-08-branco-close', numero: '08', file: 'icone-08-branco-close.png', nameKey: 'aparencia.icone.opcoes.brancoClose' },
  { id: 'icone-09-escuro-inteiro', numero: '09', file: 'icone-09-escuro-inteiro.png', nameKey: 'aparencia.icone.opcoes.escuroInteiro' },
  { id: 'icone-10-creme-close-feliz', numero: '10', file: 'icone-10-creme-close-feliz.png', nameKey: 'aparencia.icone.opcoes.cremeCloseFeliz' },
  { id: 'icone-11-gomo-laranja', numero: '11', file: 'icone-11-gomo-laranja.png', nameKey: 'aparencia.icone.opcoes.gomoLaranja' },
  { id: 'icone-12-gomo-creme', numero: '12', file: 'icone-12-gomo-creme.png', nameKey: 'aparencia.icone.opcoes.gomoCreme' },
  { id: 'icone-13-lima-close', numero: '13', file: 'icone-13-lima-close.png', nameKey: 'aparencia.icone.opcoes.limaClose' },
  { id: 'icone-14-limao-inteiro', numero: '14', file: 'icone-14-limao-inteiro.png', nameKey: 'aparencia.icone.opcoes.limaoInteiro' },
  { id: 'icone-15-toranja-close', numero: '15', file: 'icone-15-toranja-close.png', nameKey: 'aparencia.icone.opcoes.toranjaClose' },
  { id: 'icone-16-designer', numero: '16', file: 'icone-16-designer.png', nameKey: 'aparencia.icone.opcoes.designer' },
  { id: 'icone-17-pesquisa', numero: '17', file: 'icone-17-pesquisa.png', nameKey: 'aparencia.icone.opcoes.pesquisa' },
  { id: 'icone-18-financeiro', numero: '18', file: 'icone-18-financeiro.png', nameKey: 'aparencia.icone.opcoes.financeiro' },
  { id: 'icone-19-berga', numero: '19', file: 'icone-19-berga.png', nameKey: 'aparencia.icone.opcoes.berga' },
] as const

export const APP_ICON_CHOICES = [APP_ICON_DEFAULT, ...APP_ICON_OPTIONS] as const
export type AppIconChoice = (typeof APP_ICON_CHOICES)[number]
export type AppIconId = AppIconChoice['id']

const byId: ReadonlyMap<string, AppIconChoice> = new Map(APP_ICON_CHOICES.map((choice) => [choice.id, choice]))

/** Só um `id` do manifesto resolve arquivo; qualquer outra coisa (caminho, URL, bytes) vira `null`. */
export function appIconAsset(id: unknown): AppIconChoice | null {
  return typeof id === 'string' ? byId.get(id) ?? null : null
}
