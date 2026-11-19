// MX-158: a máquina de estados do cadastro de contas, sem React (o `useCadastroContas` só a liga ao render). Guarda a
// última leitura com a identidade em que foi feita (Hub + época + client); uma leitura mais nova ou o cleanup do
// efeito descarta a que está em voo; sem conexão esquece o que tinha. O render só enxerga o guardado se a identidade
// de agora for a mesma.
import { daMesmaLeitura, type ContaCadastro } from './modelo.ts'

export type EstadoCadastro = { kind: 'lendo' } | { kind: 'pronto'; contas: ContaCadastro[] } | { kind: 'falha' }
export type Guardado = { chave: string; client: unknown; valor: EstadoCadastro }
const LENDO: EstadoCadastro = { kind: 'lendo' }

export class LeituraCadastro {
  private seq = 0
  private atual: Guardado | null = null
  private ouvintes = new Set<() => void>()

  /** Para `useSyncExternalStore`: o guardado é trocado, nunca mutado. */
  assinar = (ouvinte: () => void) => { this.ouvintes.add(ouvinte); return () => { this.ouvintes.delete(ouvinte) } }
  foto = () => this.atual

  /** Começa uma leitura na identidade dada; a que estava em voo deixa de valer. */
  ler(chave: string, client: unknown, buscar: () => Promise<ContaCadastro[]>) {
    const minha = ++this.seq
    const guardar = (valor: EstadoCadastro) => { if (this.seq === minha) this.trocar({ chave, client, valor }) }
    buscar().then((contas) => guardar({ kind: 'pronto', contas }), () => guardar({ kind: 'falha' }))
  }

  /** Cleanup do efeito (identidade mudou, aba saiu, desmontou): a resposta em voo é descartada. */
  soltar() { this.seq++ }

  /** Sem conexão: voltar com o mesmo objeto de client não ressuscita o cadastro anterior. */
  esquecer() { if (this.atual) this.trocar(null) }

  /** O que o render mostra para a identidade de agora. */
  estado(chave: string, client: unknown): EstadoCadastro { return daMesmaLeitura(this.atual, chave, client) ?? LENDO }

  private trocar(proximo: Guardado | null) {
    this.atual = proximo
    for (const ouvinte of [...this.ouvintes]) ouvinte()
  }
}
