import { useEffect, useRef, useState } from 'react'
import { Button, CardForm, CardSection, IconButton, Spinner } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useModel } from '../../../lib/observable'
import { ErrorText, Field } from '../../marketplace/pieces'
import type { CatalogoStore, IntencaoHub } from './catalogo-store'
import { textoDe, frase, frases, juntas, rotuloAcao, type Frase } from './textos'
import { useTextosSkills } from './textos-tela'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'

/**
 * Página Skills → Adicionar → Escrever à mão: uma skill de texto no Hub e na conexão em que a
 * folha abriu (`origem`), pelo RPC manual forte. O texto é da pessoa e vai como foi escrito. Falha
 * ou troca de conexão deixam o rascunho aqui, com o motivo.
 *
 * Resultado incerto (enviado, sem resposta confiável): o rascunho fica, a folha diz que o Hub pode ter
 * salvo ou não e só libera "Salvar de novo" depois de a lista do Hub ser relida. Nada é reenviado
 * sozinho, e nada é concluído por nome ou texto (duas skills iguais são possíveis).
 *
 * Cada abertura é uma sessão: só enquanto esta continua `aberta` ela fecha a folha ou mostra erro.
 * O resultado de uma sessão já fechada não mexe na seguinte; o que ela enviou fica no Hub dela.
 */
export function EscreverSkill({ loja, origem, aberta, onConcluir, rascunhoInicial }: {
  loja: CatalogoStore
  origem: IntencaoHub
  /** Cópia inicial de texto passivo; lida só na montagem de uma sessão nova (key do chamador). */
  rascunhoInicial?: Readonly<{ name: string; description: string; instructions: string }>
  /** Esta sessão ainda é a folha aberta? */
  aberta: () => boolean
  /** Fecha esta sessão (e só ela). */
  onConcluir: () => void
}) {
  useModel(loja)
  const [nome, setNome] = useState(() => rascunhoInicial?.name ?? '')
  const [quando, setQuando] = useState(() => rascunhoInicial?.description ?? '')
  const [instrucoes, setInstrucoes] = useState(() => rascunhoInicial?.instructions ?? '')
  // Lição da R1 do #572: o erro fica como frase e é traduzido ao mostrar.
  const [erro, setErro] = useState<Frase | null>(null)
  const textos = useTextosSkills()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  /** O pedido em andamento é desta folha (e não de uma sessão anterior). */
  const [enviando, setEnviando] = useState(false)
  /** O pedido desta sessão ficou incerto; o nome é o do rascunho que saiu. */
  const [incerto, setIncerto] = useState<string | null>(null)
  const montada = useRef(true)
  useEffect(() => {
    montada.current = true
    return () => { montada.current = false }
  }, [])

  // A loja grava uma skill à mão por vez; o pedido pode ser de uma sessão já fechada.
  const ocupada = loja.pedido('escrever') !== null
  const salvando = ocupada && enviando
  const outraSalvando = ocupada && !enviando
  const mesmaConexao = loja.conexaoAtual(origem)
  /** Um pedido incerto (desta sessão ou de uma anterior) que a lista do Hub ainda não conferiu. */
  const pendente = loja.incertos.get('escrever') ?? null
  const aguardando = pendente !== null && !pendente.conferido
  const pronta = montada.current && aberta() && mesmaConexao && !ocupada && !aguardando && !loja.semProtocolo && nome.trim() !== '' && instrucoes.trim() !== ''

  const salvar = async () => {
    if (!pronta || !montada.current || !aberta()) return
    setErro(null)
    setIncerto(null)
    setEnviando(true)
    const resultado = await loja.escrever({ name: nome, description: quando, instructions: instrucoes }, origem)
    // Fechada ou trocada enquanto salvava: nada a fechar nem a mostrar aqui.
    if (!montada.current || !aberta()) return
    setEnviando(false)
    if (resultado === 'apareceu') onConcluir()
    else if (resultado === 'incerto') setIncerto(nome)
    else setErro(loja.conexaoAtual(origem) ? loja.falhas.get('escrever') ?? frases.escreverHubMudou : frases.escreverHubMudou)
  }
  /** O aviso desta sessão, ou o de um pedido anterior ainda sem conferência (que segura o Salvar). */
  const aviso = incerto !== null && pendente
    ? tx(juntas(pendente.mensagem, pendente.conferido ? frases.incertoEscreverDeNovo : frases.incertoConferir))
    : aguardando && pendente ? textos.incertoEscreverAnterior(pendente.nome) : null

  return (
    <SheetPage>
      <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
      <ModalHeader
        closeTitle={tx(frase('skills.fechar'))}
        icon="square.and.pencil"
        title={tx(frase('skills.escrever.titulo'))}
        trailing={salvando ? <Spinner /> : <IconButton title={tx(frase(incerto !== null ? 'skills.escrever.salvarDeNovo' : 'skills.escrever.salvar'))} icon="checkmark" disabled={!pronta} onClick={() => void salvar()} />}
      />
      <CardForm>
        {mesmaConexao && aberta() ? null : <CardSection><Text tone="danger" className="selectable">{textos.escreverHubMudou}</Text></CardSection>}
        {mesmaConexao && loja.semProtocolo ? <CardSection><Text tone="danger" className="selectable">{tx(frase('skills.semProtocoloFrase'))} {textos.semProtocoloDetalhe}</Text></CardSection> : null}
        {mesmaConexao && aviso ? (
          <CardSection>
            <VStack gap={8} align="flex-start">
              <Text tone="attention" className="selectable">{aviso}</Text>
              {aguardando ? <Button kind="secondary" disabled={loja.carregando} onClick={() => { void loja.carregar() }}>{tx(rotuloAcao.conferir)}</Button> : null}
            </VStack>
          </CardSection>
        ) : null}
        {mesmaConexao && outraSalvando ? <CardSection><Text tone="secondary">{textos.escreverOutraSalvando}</Text></CardSection> : null}
        <CardSection footer={textos.escreverRodape}>
          <Field value={nome} onChange={setNome} placeholder={tx(frase('skills.escrever.nome'))} />
          <Field value={quando} onChange={setQuando} placeholder={tx(frase('skills.escrever.quando'))} lines={[2, 4]} />
        </CardSection>
        <CardSection title={tx(frase('skills.instrucoes'))} footer={textos.rodapePagina}>
          <Field value={instrucoes} onChange={setInstrucoes} placeholder={tx(frase('skills.escrever.instrucoesExemplo'))} lines={[6, 20]} />
        </CardSection>
        {erro && mesmaConexao ? <CardSection><ErrorText text={tx(erro)} /></CardSection> : null}
      </CardForm>
      </div>
    </SheetPage>
  )
}
