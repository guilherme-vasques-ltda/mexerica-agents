import { randomBytes } from 'node:crypto'
import { promises as fsp } from 'node:fs'
import { join } from 'node:path'
import { COFRES_LINUX_ACEITOS, formatoDeChave, VOZ_PROVEDORES_APARELHO, type EstadoChaves, type SituacaoChave, type VozProvedorAparelho } from '../../shared/fork-voz.ts'

// voz-04 (plano §2): o cofre da chave de voz deste aparelho. Só conteúdo cifrado pelo cofre do sistema
// (`safeStorage`: Keychain no Mac, DPAPI no Windows, Chaveiro do GNOME/KWallet no Linux), um arquivo por
// provedor em `userData/voz-chaves/`, com escrita atômica. Separado da conta DONNA: nenhum arquivo, registro
// ou chave dela é lido ou reutilizado. Nenhum texto de erro daqui leva a chave.

/** O pedaço do `safeStorage` que o cofre usa (injetado nos testes). */
export interface CofreDoSistema {
  isEncryptionAvailable(): boolean
  getSelectedStorageBackend?(): string
  encryptString(texto: string): Buffer
  decryptString(cifrado: Buffer): string
}

export interface DependenciasCofre {
  /** A pasta de dados do app (`app.getPath('userData')`). */
  dados: string
  sistema: CofreDoSistema
  plataforma: NodeJS.Platform
  fs?: Pick<typeof fsp, 'mkdir' | 'open' | 'rename' | 'rm' | 'readFile'>
}

/** Uma operação do cofre não deu certo; só a categoria, sem texto do sistema. */
export class FalhaCofre extends Error {
  readonly categoria: 'cofre' | 'sem-cofre' | 'registro' | 'cancelado'
  constructor(categoria: 'cofre' | 'sem-cofre' | 'registro' | 'cancelado') {
    // D454 (fatia 10b): a mensagem é a própria categoria (código); só ela sai do main, e o app a traduz.
    super(categoria)
    this.name = 'FalhaCofre'
    this.categoria = categoria
  }
}

export class CofreVoz {
  private readonly pasta: string
  private readonly fs: NonNullable<DependenciasCofre['fs']>
  /** A chave já aberta nesta sessão, para não pedir o cofre do sistema a cada chamada. Limpar descarta. */
  private readonly aberta = new Map<VozProvedorAparelho, string>()
  private readonly travadas = new Set<VozProvedorAparelho>()
  private readonly recusadas = new Set<VozProvedorAparelho>()
  private readonly reserva = new Set<VozProvedorAparelho>()
  private readonly pendentes = new Map<VozProvedorAparelho, 'incerta' | 'limpeza-pendente'>()
  private readonly geracoes = new Map<VozProvedorAparelho, number>()
  private fila: Promise<unknown> = Promise.resolve()
  private readonly deps: DependenciasCofre

  constructor(deps: DependenciasCofre) {
    this.deps = deps
    this.pasta = join(deps.dados, 'voz-chaves')
    this.fs = deps.fs ?? fsp
    // A primeira operação da fila é a reconciliação. Nem estado nem chave saem antes dela.
    this.fila = this.reconciliar()
  }

  /** O sistema tem cofre seguro aceito. No Linux, só a lista permitida; `basic_text` e `unknown`, nunca. */
  seguro(): boolean {
    const { sistema, plataforma } = this.deps
    try {
      if (!sistema.isEncryptionAvailable()) return false
      if (plataforma !== 'linux') return true
      return COFRES_LINUX_ACEITOS.includes(sistema.getSelectedStorageBackend?.() ?? 'unknown')
    } catch {
      return false
    }
  }

  arquivo(p: VozProvedorAparelho) { return join(this.pasta, `${p}.bin`) }
  private marcador(p: VozProvedorAparelho) { return join(this.pasta, `${p}.pendente`) }

  private bloquear(p: VozProvedorAparelho, situacao: 'incerta' | 'limpeza-pendente') {
    this.aberta.delete(p)
    this.pendentes.set(p, situacao)
  }

  private async reconciliar() {
    for (const p of VOZ_PROVEDORES_APARELHO) {
      let registro: { op?: unknown; id?: unknown }
      try {
        registro = JSON.parse((await this.fs.readFile(this.marcador(p))).toString('utf8'))
        if (!registro || typeof registro.id !== 'string' || !/^[a-f0-9]{32}$/.test(registro.id) ||
          (registro.op !== 'guardar' && registro.op !== 'limpar')) throw new Error()
      } catch (e) {
        if ((e as NodeJS.ErrnoException).code !== 'ENOENT') this.bloquear(p, 'incerta')
        continue
      }
      this.bloquear(p, registro.op === 'limpar' ? 'limpeza-pendente' : 'incerta')
      try {
        if (registro.op === 'limpar') await this.limparAgora(p)
        else {
          const conteudo = await this.lerChave(p)
          // Mesmo id: a nova gravação chegou. Outro id (inclusive formato antigo): ficou a chave
          // anterior. Ausência: não gravou. Todos são resultados conhecidos pelo disco atual.
          this.pendentes.delete(p)
          if (conteudo) this.aberta.set(p, conteudo.chave)
          await this.removerMarcador(p)
        }
      } catch { /* Arquivo ilegível ou limpeza não confirmada: mantém o bloqueio. */ }
    }
  }

  /** Mesmo protocolo para chave e marcador. O marcador recebe só operação e id aleatório, nunca a chave. */
  private async escreverAtomico(destino: string, dados: Buffer, renomeou: () => void = () => {}) {
    const temporario = `${destino}.${randomBytes(12).toString('hex')}.tmp`
    try {
      await this.fs.mkdir(this.pasta, { recursive: true, mode: 0o700 })
      const f = await this.fs.open(temporario, 'wx', 0o600)
      try { await f.writeFile(dados); await f.sync() } finally { await f.close() }
      await this.fs.rename(temporario, destino)
      renomeou()
      await this.sincronizarPasta()
    } finally {
      await this.fs.rm(temporario, { force: true }).catch(() => {})
    }
  }

  private escreverMarcador(p: VozProvedorAparelho, op: 'guardar' | 'limpar', id: string, renomeou: () => void) {
    return this.escreverAtomico(this.marcador(p), Buffer.from(JSON.stringify({ op, id })), renomeou)
  }

  /** A operação já foi confirmada. Um marcador velho é resolvido ao iniciar; não muda o resultado. */
  private async removerMarcador(p: VozProvedorAparelho) {
    try {
      await this.fs.rm(this.marcador(p), { force: true })
      await this.sincronizarPasta()
    } catch { /* Nunca repor: a chave cifrada ou a intenção de limpar já determinam o resultado. */ }
  }

  private async iniciarOperacao(p: VozProvedorAparelho, op: 'guardar' | 'limpar', id: string) {
    let renomeou = false
    try { await this.escreverMarcador(p, op, id, () => { renomeou = true }) } catch {
      if (renomeou) {
        if (op === 'limpar') {
          // A intenção pode estar no disco: não restaurar o registro anterior, nem voltar a usar a chave.
          this.bloquear(p, 'limpeza-pendente')
          this.reserva.delete(p)
        }
        throw new FalhaCofre('cofre')
      }
      throw new FalhaCofre('registro')
    }
  }

  geracao(p: VozProvedorAparelho): number { return this.geracoes.get(p) ?? 0 }

  /** Trocar e Limpar invalidam toda resposta do provedor vinculada à chave anterior. */
  novaGeracao(p: VozProvedorAparelho): number {
    const geracao = this.geracao(p) + 1
    this.geracoes.set(p, geracao)
    return geracao
  }

  /** Confere e aplica uma resposta na mesma passagem pela fila. O callback é síncrono, sem rede. */
  naGeracao(p: VozProvedorAparelho, geracao: number, aplicar: () => void = () => {}): Promise<void> {
    return this.emFila(() => {
      if (geracao !== this.geracao(p)) throw new FalhaCofre('cancelado')
      aplicar()
    })
  }

  /** Uma operação por vez: guardar, limpar e abrir nunca se cruzam. */
  private emFila<T>(acao: () => T | Promise<T>): Promise<T> {
    const proxima = this.fila.catch(() => {}).then(acao)
    this.fila = proxima
    return proxima
  }

  private async existe(p: VozProvedorAparelho): Promise<boolean> {
    try { await this.fs.readFile(this.arquivo(p)); return true } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') return false
      throw new FalhaCofre('cofre')
    }
  }

  /** Abre a chave guardada; null se não há chave. Cofre que não abre lança `FalhaCofre` e preserva o arquivo. */
  abrir(p: VozProvedorAparelho): Promise<string | null> {
    return this.emFila(() => this.abrirAgora(p))
  }

  private async abrirAgora(p: VozProvedorAparelho): Promise<string | null> {
    if (this.pendentes.has(p)) throw new FalhaCofre('cofre')
    const ja = this.aberta.get(p)
    if (ja !== undefined) return ja
    try {
      const conteudo = await this.lerChave(p)
      this.travadas.delete(p)
      if (!conteudo) return null
      this.aberta.set(p, conteudo.chave)
      return conteudo.chave
    } catch {
      this.travadas.add(p)
      throw new FalhaCofre('cofre')
    }
  }

  /** O id e a chave só existem juntos dentro do conteúdo protegido pelo safeStorage. */
  private async lerChave(p: VozProvedorAparelho): Promise<{ id: string | null; chave: string } | null> {
    let cifrado: Buffer
    try { cifrado = await this.fs.readFile(this.arquivo(p)) } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') return null
      throw e
    }
    try {
      if (!this.seguro()) throw new Error()
      const texto = this.deps.sistema.decryptString(cifrado)
      // Compatibilidade com as chaves cifradas antes do identificador de operação.
      if (!texto.startsWith('{') && formatoDeChave(texto)) return { id: null, chave: texto }
      const conteudo = JSON.parse(texto)
      if (!conteudo || typeof conteudo.id !== 'string' || !/^[a-f0-9]{32}$/.test(conteudo.id) ||
        typeof conteudo.chave !== 'string' || !formatoDeChave(conteudo.chave)) throw new Error()
      return conteudo
    } finally { cifrado.fill(0) }
  }

  /** Cifra e grava (temporário exclusivo, fsync, rename, fsync da pasta). Sem cofre aceito, nada é gravado. */
  guardar(p: VozProvedorAparelho, chave: string, geracao = this.geracao(p)): Promise<void> {
    return this.emFila(async () => {
      // A verificação precisa estar dentro da fila: Limpar pode chegar depois da validação de rede.
      if (geracao !== this.geracao(p)) throw new FalhaCofre('cancelado')
      if (this.pendentes.get(p) === 'limpeza-pendente') throw new FalhaCofre('cofre')
      if (!this.seguro()) throw new FalhaCofre('sem-cofre')
      const id = randomBytes(16).toString('hex')
      let cifrado: Buffer
      try { cifrado = this.deps.sistema.encryptString(JSON.stringify({ id, chave })) } catch { throw new FalhaCofre('cofre') }
      try {
        await this.iniciarOperacao(p, 'guardar', id)
        try {
          await this.escreverAtomico(this.arquivo(p), cifrado, () => {
            // Quem abriu a chave anterior enquanto esta troca validava também ficou superado.
            // Uma intenção posterior já mudou a geração e precisa manter sua vez.
            if (geracao === this.geracao(p)) this.novaGeracao(p)
            this.bloquear(p, 'incerta')
            this.reserva.delete(p)
          })
        } catch { throw new FalhaCofre('cofre') }
      } finally { cifrado.fill(0) }
      this.pendentes.delete(p)
      this.aberta.set(p, chave)
      this.travadas.delete(p)
      this.recusadas.delete(p)
      this.reserva.delete(p)
      await this.removerMarcador(p)
    })
  }

  /** Limpar é intenção durável. Nenhuma falha posterior restaura a chave ou o marcador anterior. */
  limpar(p: VozProvedorAparelho): Promise<void> {
    this.novaGeracao(p)
    return this.emFila(async () => {
      await this.iniciarOperacao(p, 'limpar', randomBytes(16).toString('hex'))
      this.bloquear(p, 'limpeza-pendente')
      this.reserva.delete(p)
      try { await this.limparAgora(p) } catch { throw new FalhaCofre('cofre') }
    })
  }

  private async limparAgora(p: VozProvedorAparelho) {
    await this.fs.rm(this.arquivo(p), { force: true })
    await this.sincronizarPasta()
    this.pendentes.delete(p)
    this.recusadas.delete(p)
    this.reserva.delete(p)
    this.travadas.delete(p)
    await this.removerMarcador(p)
  }

  private async sincronizarPasta() {
    // No Windows não há `fsync` de pasta (abrir uma pasta como arquivo falha); o `rename` do NTFS já é
    // registrado pelo próprio sistema. A durabilidade da pasta vale no Mac e no Linux.
    if (this.deps.plataforma === 'win32') return
    let pasta
    try { pasta = await this.fs.open(this.pasta, 'r') } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') return
      throw e
    }
    try { await pasta.sync() } finally { await pasta.close() }
  }

  /** Tentar de novo confirma a pasta e a leitura antes de resolver a gravação incerta. */
  tentarAbrir(p: VozProvedorAparelho): Promise<string | null> {
    return this.emFila(async () => {
      if (this.pendentes.get(p) === 'limpeza-pendente') throw new FalhaCofre('cofre')
      if (this.pendentes.get(p) === 'incerta') {
        try {
          await this.sincronizarPasta()
          const conteudo = await this.lerChave(p)
          this.pendentes.delete(p)
          if (conteudo) this.aberta.set(p, conteudo.chave)
          await this.removerMarcador(p)
        } catch { throw new FalhaCofre('cofre') }
      }
      return this.abrirAgora(p)
    })
  }

  /** "Tentar de novo": esquece a cópia aberta e a marca de travada, para abrir do zero. */
  esquecer(p: VozProvedorAparelho) {
    this.aberta.delete(p)
    this.travadas.delete(p)
  }

  marcarRecusada(p: VozProvedorAparelho) { this.recusadas.add(p) }
  /** O provedor aceitou a chave. Não desfaz a escolha do Hub: isso só a pessoa faz (ver `voltarAoAparelho`). */
  marcarAceita(p: VozProvedorAparelho) { this.recusadas.delete(p) }
  /** "Usar a chave deste Mac" (Tentar de novo com sucesso): a pessoa volta à chave daqui. */
  voltarAoAparelho(p: VozProvedorAparelho) { this.reserva.delete(p) }

  /** Só com a chave deste aparelho travada, recusada ou incerta; vale até reiniciar, Trocar ou Limpar. */
  escolherReserva(p: VozProvedorAparelho): boolean {
    if (p === 'xai') return false
    if (this.pendentes.get(p) === 'limpeza-pendente') return false
    if (!this.travadas.has(p) && !this.recusadas.has(p) && this.pendentes.get(p) !== 'incerta') return false
    this.reserva.add(p)
    return true
  }

  /** O estado de cada provedor. Abre a chave (sem rede) para saber se o cofre está travado. */
  estado(): Promise<EstadoChaves> {
    return this.emFila(async () => {
      const chaves = {} as EstadoChaves['chaves']
      for (const p of VOZ_PROVEDORES_APARELHO) {
        let situacao: SituacaoChave
        try {
          situacao = this.pendentes.get(p) ?? ((await this.abrirAgora(p)) === null ? 'ausente' : this.recusadas.has(p) ? 'recusada' : 'guardada')
        } catch {
          situacao = (await this.existe(p).catch(() => true)) ? 'travada' : 'ausente'
        }
        // A escolha do Hub vale até reiniciar, Trocar, Limpar ou a pessoa voltar à chave daqui, mesmo que o
        // cofre volte a abrir sozinho: nunca passa a cobrar a chave deste aparelho em silêncio.
        chaves[p] = { situacao, reserva: situacao !== 'ausente' && this.reserva.has(p) }
      }
      return { cofre: this.seguro() ? 'seguro' : 'sem-cofre', chaves }
    })
  }
}
