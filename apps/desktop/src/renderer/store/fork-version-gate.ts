import { checkVersions, isBelow, MIN_HOST, parseVersion, type VersionMismatch } from '@shared/compat'
import { HostError, type HostTransport } from '../client/host-client'
import { linkCaido } from '../client/fork-transporte-parado'
import { ErroFrase, frase } from '../lib/fork-i18n/frase'

/** Recusa local, traduzível e reconhecível sem interpretar uma mensagem. */
export class Desatualizado extends ErroFrase {
  readonly name = 'Desatualizado'
  constructor(readonly versao: string | null, readonly minimo: string) {
    super(frase('spaces.recusa.atualize'))
  }
}

const legivel = (v: unknown): v is string => typeof v === 'string' && !!parseVersion(v) && parseVersion(v)!.every(Number.isSafeInteger)

/** Uma prova de hello só autoriza o transporte que a obteve. Nunca persiste no cache. */
export class VersionGate {
  currentGeneration = 0
  provenGeneration = -1
  /** Último mismatch CONHECIDO: só informação retida para a UI e para proteger o cache. */
  mismatch: VersionMismatch | null = null
  /** Geração cujo hello PROVOU `mismatch`. Só essa evidência autoriza recusar uma ação. */
  mismatchGeneration = -1
  private closes = new Set<() => void>()

  constructor(private app: string, private refused: (error: Desatualizado) => void) {}

  invalidate() {
    this.currentGeneration++
    for (const close of this.closes) close()
    this.closes.clear()
    return this.currentGeneration
  }

  /**
   * Hello sem `minApp` legível = o Hub provavelmente é antigo (o Hub atual sempre manda): `updateHost`
   * com a versão lida do Hub (`""` se nem ela se lê; o getter `desatualizado` a expõe como `null`).
   * App ilegível ou abaixo do mínimo = `updateApp`.
   */
  prove(generation: number, host: { version: string; minApp?: string | null }, unreadable = false) {
    if (generation !== this.currentGeneration) return false
    const { version, minApp } = host
    this.mismatch = !legivel(version) ? { kind: 'updateHost', version: '', minimum: MIN_HOST }
      : !legivel(minApp) ? { kind: 'updateHost', version, minimum: MIN_HOST }
      : this.app !== 'dev' && !legivel(this.app) ? { kind: 'updateApp', minimum: minApp }
      : checkVersions(this.app, version, minApp)
    if (!this.mismatch && unreadable && isBelow(this.app, version)) this.mismatch = { kind: 'updateApp', minimum: version }
    this.provenGeneration = this.mismatch ? -1 : generation
    this.mismatchGeneration = this.mismatch ? generation : -1
    return this.allowed(generation)
  }

  allowed(generation = this.currentGeneration) {
    return generation === this.currentGeneration && this.provenGeneration === generation && !this.mismatch
  }

  error() {
    const m = this.mismatch
    return new Desatualizado(m?.kind === 'updateHost' ? m.version || null : this.app ?? null, m?.minimum ?? MIN_HOST)
  }

  /**
   * Só um mismatch PROVADO NA GERAÇÃO ATUAL (`mismatchGeneration`) recusa como `Desatualizado` (e só ele
   * grava motivo). `mismatch` persiste após a geração mudar, para o aviso não piscar a cada reconexão e
   * o cache seguir protegido, mas é só informação retida: não prova nada sobre a geração nova. Sem prova
   * da geração (hello pendente, reconexão, geração velha) não há motivo para culpar a versão: o transporte guardado falha como link caído — geração velha: `linkCaido()` (status 0,
   * sem replay por outro link, MX-217); geração atual ainda sem prova: `unreachable` transitório — e quem
   * espera o link (`ready`, `withLink` com replay) segue esperando.
   */
  refuseIfMismatch() {
    if (!this.mismatch || this.mismatchGeneration !== this.currentGeneration) return
    const error = this.error()
    this.refused(error)
    throw error
  }

  assert(generation = this.currentGeneration) {
    this.refuseIfMismatch()
    if (generation !== this.currentGeneration) throw linkCaido()
    if (!this.allowed(generation)) throw HostError.unreachable()
  }

  /** Inclui inSpace, referências antigas, respostas pendentes e entregas tardias de stream. */
  transport(raw: HostTransport, generation: number): HostTransport {
    return {
      call: async (method, body, timeout) => {
        this.assert(generation)
        try {
          const result = await raw.call(method, body, timeout)
          this.assert(generation)
          return result
        } catch (error) {
          this.refuseIfMismatch()
          throw error
        }
      },
      stream: (path, onData, onEnd) => {
        this.assert(generation)
        let active = true
        let stop = () => {}
        const close = () => { active = false; this.closes.delete(close); stop() }
        this.closes.add(close)
        stop = raw.stream(path, line => {
          if (active && this.allowed(generation)) onData(line)
        }, error => {
          if (!active) return
          active = false
          this.closes.delete(close)
          onEnd(error)
        })
        if (!active) stop()
        return close
      },
    }
  }
}
