import type { EscolhaDeConta } from '@shared/fork-criar-bot-destino'
import type { ManagedAccount } from '@shared/fork-engine-accounts'
import type { Space } from '@shared/fork-spaces'
import { ChoicePicker, Spinner } from '../../components/Controls'
import type { EscolhaEquipe, OpcaoEquipe } from '../fork/criar-bot-equipe/equipe'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'

/** Passo 2, conta de IA (DON-865 f2): padrão do Hub ou uma conta gerenciada conectada do mesmo motor. Só intenção. */
export function ContaDaIa({ escolha, contas, carregando, falhou, conta, disabled, padraoDisponivel = true, onConta }: {
  escolha: EscolhaDeConta
  contas: { conta: ManagedAccount; escolhivel: boolean }[]
  carregando: boolean
  falhou: boolean
  conta: string | null
  disabled: boolean
  padraoDisponivel?: boolean
  onConta: (id: string | null) => void
}) {
  const t = useT()
  if (escolha === 'motor-sem-conta') return <p className="nb-nota">{t('botCreate.contaMotorSemConta')}</p>
  if (escolha === 'sem-posse') return <p className="nb-nota nb-lugar" role="note">{t('botCreate.contaSemPosse')}</p>
  if (carregando) return <span className="nb-carregando" role="status"><Spinner size={12} /> {t('botCreate.contasCarregando')}</span>
  const linhas = [
    { id: null, nome: t('botCreate.contaPadrao'), linha: t('botCreate.contaPadraoLinha'), escolhivel: padraoDisponivel },
    ...contas.map(({ conta: c, escolhivel }) => ({ id: c.id, nome: c.label, linha: t('botCreate.contaLinha', { estado: t(`botCreate.contaEstado.${c.state}`) }), escolhivel })),
  ]
  return <>
    <div role="radiogroup" aria-label={t('botCreate.contaDeIa')} className="nb-lista">
      {linhas.map((l) => <button key={l.id ?? ''} type="button" role="radio" aria-checked={conta === l.id} disabled={disabled || !l.escolhivel}
        className="nb-lista-item" onClick={() => onConta(l.id)}>
        <span className="nb-radio" aria-hidden="true" />
        <span className="nb-lista-texto">
          <span className="nb-lista-nome">{l.nome}</span>
          <span className="nb-lista-linha">{l.linha}</span>
        </span>
      </button>)}
    </div>
    <p className="nb-nota">{falhou ? t('botCreate.contasFalha') : contas.length ? t('botCreate.contaNota') : t('botCreate.contaNenhuma')}</p>
  </>
}

/** Passo 1, onde ele trabalha: Pessoal ou um Space que o app já conhece. O Hub confere o Space. */
export function OndeTrabalha({ spaces, personalId, contrato, spaceId, travado = false, onSpace }: {
  spaces: Space[]
  personalId: string | null
  contrato: boolean
  spaceId: string | null
  /** Depois de criar, com o passo 2 pendente: o lugar não muda mais. */
  travado?: boolean
  onSpace: (id: string | null) => void
}) {
  const t = useT()
  if (!contrato) return <p className="nb-nota nb-lugar">{t('botCreate.ondeSemContrato')}</p>
  const outros = spaces.filter((s) => s.id !== personalId)
  if (!outros.length) return <p className="nb-nota nb-lugar">{t('botCreate.ondeSoPessoal')}</p>
  const atual = spaceId && spaceId !== personalId ? spaceId : null
  return <div role="radiogroup" aria-label={t('botCreate.onde')} className="nb-lista">
    {[{ id: null, nome: t('botCreate.ondePessoal'), linha: t('botCreate.ondePessoalLinha') }, ...outros.map((s) => ({ id: s.id, nome: s.name, linha: t('botCreate.ondeSpaceLinha') }))].map((l) =>
      <button key={l.id ?? ''} type="button" role="radio" aria-checked={atual === l.id} disabled={travado} className="nb-lista-item" onClick={() => onSpace(l.id)}>
        <span className="nb-radio" aria-hidden="true" />
        <span className="nb-lista-texto">
          <span className="nb-lista-nome">{l.nome}</span>
          <span className="nb-lista-linha">{l.linha}</span>
        </span>
      </button>)}
  </div>
}

/** Passo 1, equipe e chefe (fatia 3): as equipes do quadro deste lugar; a gravação é o segundo passo, depois de criar. */
export function EscolhaDaEquipe({ estado, opcoes, escolha, travado = false, onEscolha }: {
  estado: { kind: 'carregando' } | { kind: 'pronto' } | { kind: 'erro'; falha: string }
  opcoes: OpcaoEquipe[]
  escolha: EscolhaEquipe | null
  /** Depois de criar, com o passo 2 pendente: equipe e chefe ficam como foram pedidos. */
  travado?: boolean
  onEscolha: (escolha: EscolhaEquipe | null) => void
}) {
  const t = useT()
  const idioma = useIdioma()
  if (estado.kind === 'carregando') return <span className="nb-carregando" role="status"><Spinner size={12} /> {t('botCreate.equipeCarregando')}</span>
  if (estado.kind === 'erro') return <p className="nb-nota nb-lugar">{t(estado.falha === 'owner' || estado.falha === 'old_hub' ? `botCreate.equipeLeitura.${estado.falha}` : 'botCreate.equipeLeitura.outro')}</p>
  if (!opcoes.length) return <p className="nb-nota nb-lugar">{t('botCreate.equipeNenhuma')}</p>
  const atual = escolha ? opcoes.find((o) => o.id === escolha.areaId) ?? null : null
  const chefe = atual?.chefes.find((c) => c.id === escolha?.chefeId)
  return <>
    <div role="radiogroup" aria-label={t('botCreate.equipe')} className="nb-lista">
      {[{ id: null as string | null, nome: t('botCreate.equipeSem') }, ...opcoes].map((o) => {
        const opcao = o.id ? opcoes.find((x) => x.id === o.id)! : null
        const linha = !opcao ? t('botCreate.equipeSemLinha')
          : opcao.chefePadrao ? t('botCreate.equipeComChefe', { chefe: opcao.chefes[0]!.nome }) : t('botCreate.equipeSemChefe')
        return <button key={o.id ?? ''} type="button" role="radio" aria-checked={(atual?.id ?? null) === o.id} disabled={travado} className="nb-lista-item"
          onClick={() => onEscolha(opcao ? { areaId: opcao.id, chefeId: opcao.chefePadrao } : null)}>
          <span className="nb-radio" aria-hidden="true" />
          <span className="nb-lista-texto">
            <span className="nb-lista-nome">{o.nome}</span>
            <span className="nb-lista-linha">{linha}</span>
          </span>
        </button>
      })}
    </div>
    {atual && atual.chefes.length > 1 ? <div className="nb-linha-campo">
      <span className="nb-rotulo-linha">{t('botCreate.chefe')}</span>
      <ChoicePicker language={superficieIdioma(idioma)} selection={escolha?.chefeId ?? ''} disabled={travado} options={atual.chefes.map((c) => ({ id: c.id, label: c.nome }))}
        onChange={(chefeId) => onEscolha({ areaId: atual.id, chefeId: chefeId || null })} />
    </div> : null}
    {atual ? <p className="nb-nota">{chefe ? t('botCreate.equipeComChefe', { chefe: chefe.nome }) : t('botCreate.equipeSemChefe')} {t('botCreate.equipeDoisPassos')}</p> : null}
  </>
}
