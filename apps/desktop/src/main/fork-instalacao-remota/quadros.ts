// MEX-1307 F0 (plano §4.6): parser de quadros tipados da saída remota. Só dois tipos de quadro existem:
//   `MEX1 E <codigo> <estado>`                                   evento exibível (só o catálogo do app vira texto)
//   `MEX1 D <tipo> <n>` LF <n bytes> LF `MEX1 FIM <tipo>` LF     mensagem de dados (nunca exibida)
// Qualquer outra linha é descartada e só incrementa um contador. Quadro de dados fora do teto, da gramática ou sem o
// `FIM` falha a fase inteira. Limite total de 64 KiB por processo: estourar = a fase falha (quem chama mata o processo).
import {
  CODIGOS_EVENTO, ESTADOS_EVENTO, TETO_DADO, TIPOS_DADO, validarDado,
  type CodigoEvento, type Dado, type EstadoEvento, type TipoDado,
} from './esquemas.ts'

export const LIMITE_TOTAL = 64 * 1024
/** Linha que não é quadro de dados: passa disso sem LF, é descartada até o próximo LF. */
export const LIMITE_LINHA = 256

export type MotivoFalha = 'estouro-total' | 'cabecalho-invalido' | 'teto-excedido' | 'fim-ausente' | 'gramatica-invalida' | 'saida-truncada'

export interface Evento { codigo: CodigoEvento; estado: EstadoEvento }
export interface ResultadoQuadros {
  eventos: Evento[]
  dados: Dado[]
  /** Linhas fora dos dois formatos: contadas, nunca exibidas nem registradas em texto. */
  descartadas: number
  /** Preenchido quando a fase falhou; eventos e dados lidos até ali não valem. */
  falha: MotivoFalha | null
}

const CODIGOS: ReadonlySet<string> = new Set(CODIGOS_EVENTO)
const ESTADOS: ReadonlySet<string> = new Set(ESTADOS_EVENTO)
const TIPOS: ReadonlySet<string> = new Set(TIPOS_DADO)
const EVENTO = /^MEX1 E ([a-z0-9]{1,16}) (ok|pulado|falha)$/
const UTF8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true })
/** Começo de um cabeçalho de dados: linha que começa assim nunca é descartada em silêncio. */
const INICIO_DADO = Buffer.from([0x4d, 0x45, 0x58, 0x31, 0x20, 0x44]) // bytes de MEX1 D: token de protocolo, não texto de interface
const iniciaDado = (b: Buffer) => b.length >= INICIO_DADO.length && b.subarray(0, INICIO_DADO.length).equals(INICIO_DADO)
const CABECALHO = /^MEX1 D ([a-z0-9]{1,16}) ([0-9]{1,5})$/

export class AnalisadorQuadros {
  private pendente: Buffer = Buffer.alloc(0)
  private total = 0
  private descartando = false
  private emDado: { tipo: TipoDado; n: number } | null = null
  readonly eventos: Evento[] = []
  readonly dados: Dado[] = []
  descartadas = 0
  falha: MotivoFalha | null = null

  /** Dá mais bytes de `stdout`. Depois de uma falha os bytes são ignorados (o processo deve ser morto). */
  empurrar(pedaco: Buffer): void {
    if (this.falha) return
    this.total += pedaco.length
    if (this.total > LIMITE_TOTAL) return this.falhar('estouro-total')
    this.pendente = this.pendente.length === 0 ? pedaco : Buffer.concat([this.pendente, pedaco])
    this.processar()
  }

  /** Fim da saída: dado pela metade ou linha sem LF no fim é truncamento; truncamento falha. */
  finalizar(): ResultadoQuadros {
    if (!this.falha) {
      if (this.emDado) this.falhar('saida-truncada')
      else if (this.pendente.length > 0 && !this.descartando) {
        // Cabeçalho de dados sem LF final (cortado): a fase falha; o que veio antes não vale.
        if (iniciaDado(this.pendente)) this.falhar('cabecalho-invalido')
        else this.descartadas++
      }
    }
    return { eventos: this.falha ? [] : this.eventos, dados: this.falha ? [] : this.dados, descartadas: this.descartadas, falha: this.falha }
  }

  private falhar(motivo: MotivoFalha) {
    this.falha = motivo
    this.pendente = Buffer.alloc(0)
  }

  private processar() {
    for (;;) {
      if (this.emDado) {
        const { tipo, n } = this.emDado
        const rodape = `\nMEX1 FIM ${tipo}\n`
        const precisa = n + rodape.length
        if (this.pendente.length < precisa) {
          // Rodapé divergente já visível antes de chegar tudo: não espera mais bytes para falhar.
          const visivel = this.pendente.subarray(n, precisa).toString('latin1')
          if (this.pendente.length > n && !rodape.startsWith(visivel)) return this.falhar('fim-ausente')
          return
        }
        if (this.pendente.subarray(n, precisa).toString('latin1') !== rodape) return this.falhar('fim-ausente')
        // UTF-8 estrito: `n` conta bytes; byte inválido falha a fase em vez de virar caractere trocado.
        let conteudo: string
        try { conteudo = UTF8.decode(this.pendente.subarray(0, n)) } catch { return this.falhar('gramatica-invalida') }
        const dado = validarDado(tipo, conteudo)
        if (!dado) return this.falhar('gramatica-invalida')
        this.dados.push(dado)
        this.pendente = this.pendente.subarray(precisa)
        this.emDado = null
        continue
      }
      const lf = this.pendente.indexOf(0x0a)
      if (lf < 0) {
        if (this.pendente.length > LIMITE_LINHA) {
          // Cabeçalho de dados acima de 256 bytes: falha a fase, não é descartado.
          if (!this.descartando && iniciaDado(this.pendente)) return this.falhar('cabecalho-invalido')
          if (!this.descartando) this.descartadas++
          this.descartando = true
          this.pendente = Buffer.alloc(0)
        }
        return
      }
      const bruta = this.pendente.subarray(0, lf)
      this.pendente = this.pendente.subarray(lf + 1)
      if (this.descartando) { this.descartando = false; continue }
      if (bruta.length > LIMITE_LINHA) {
        if (iniciaDado(bruta)) return this.falhar('cabecalho-invalido')
        this.descartadas++
        continue
      }
      const linha = bruta.toString('latin1')
      const cab = CABECALHO.exec(linha)
      if (iniciaDado(bruta)) {
        // Cabeçalho de dados malformado, de tipo desconhecido ou fora do teto: o quadro inteiro e a fase falham.
        if (!cab || !TIPOS.has(cab[1]!) || (cab[2]!.length > 1 && cab[2]!.startsWith('0'))) return this.falhar('cabecalho-invalido')
        const tipo = cab[1] as TipoDado
        const n = Number(cab[2])
        if (n < 1 || n > TETO_DADO[tipo]) return this.falhar('teto-excedido')
        this.emDado = { tipo, n }
        continue
      }
      const ev = EVENTO.exec(linha)
      if (ev && CODIGOS.has(ev[1]!) && ESTADOS.has(ev[2]!)) this.eventos.push({ codigo: ev[1] as CodigoEvento, estado: ev[2] as EstadoEvento })
      else this.descartadas++
    }
  }
}

/** Conveniência: lê uma saída inteira de uma vez. */
export function analisarSaida(saida: Buffer): ResultadoQuadros {
  const a = new AnalisadorQuadros()
  a.empurrar(saida)
  return a.finalizar()
}

/** Monta quadros (usado pelos fixtures e pelos testes; o carregador faz o mesmo em `sh`). */
export const quadroEvento = (codigo: CodigoEvento, estado: EstadoEvento) => `MEX1 E ${codigo} ${estado}\n`
export const quadroDado = (tipo: TipoDado, conteudo: string) => `MEX1 D ${tipo} ${Buffer.byteLength(conteudo)}\n${conteudo}\nMEX1 FIM ${tipo}\n`
