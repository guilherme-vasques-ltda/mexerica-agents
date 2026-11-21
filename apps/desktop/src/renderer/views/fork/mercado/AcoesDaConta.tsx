// Ações perigosas (fatia 2): as ferramentas de UMA conta que o Hub deixa desligadas num bot (escrevem, enviam, publicam,
// apagam ou mexem em dinheiro, ou são novas ou mudaram). Só a pessoa liga, com confirmação; o Hub confere de novo.
import { useCallback, useEffect, useRef, useState } from 'react'
import { Button } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { desligarFerramenta, ferramentasDoBot, ligarFerramenta } from './estado'
import { acoesQueAPessoaDecide, listaDeNomes, podeLigar, temCaracteresOcultos, textoNaoLido } from './regras.ts'
import type { FerramentaDaConta, FerramentasDaConta } from './tipos.ts'
import { Aviso, Carregando, Erro, frasesDaRecusa } from './pecas'

export function AcoesDaConta({ botId, botNome, conta }: { botId: string; botNome: string; conta: { id: string; nome: string } }) {
  const store = useStore()
  const t = useT()
  const [dados, setDados] = useState<FerramentasDaConta | null>(null)
  const [falhou, setFalhou] = useState(false)
  const [erro, setErro] = useState<string | null>(null)
  // A confirmação guarda o que a pessoa viu ao abri-la (digest e época do vínculo); recarregar a lista a descarta.
  const [confirmando, setConfirmando] = useState<{ nome: string; digest: string; vinculo: number; apresentacao: string } | null>(null)
  const [salvando, setSalvando] = useState(false)
  const leitura = useRef(0)

  const ler = useCallback(async () => {
    const minha = ++leitura.current
    setFalhou(false); setConfirmando(null)
    try {
      const lida = await ferramentasDoBot(store, botId, conta.id)
      if (minha === leitura.current) { setDados(lida); setConfirmando(null) }
    } catch { if (minha === leitura.current) setFalhou(true) }
  }, [store, botId, conta.id])
  useEffect(() => { setDados(null); setConfirmando(null); setErro(null); void ler(); return () => { leitura.current += 1 } }, [ler])

  const aplicar = async (op: () => Promise<FerramentasDaConta>) => {
    setSalvando(true); setErro(null)
    try { setDados(await op()); setConfirmando(null) } catch (e) { setErro(frasesDaRecusa(t, e)); await ler() } finally { setSalvando(false) }
  }
  const ligar = (visto: { nome: string; digest: string; vinculo: number; apresentacao: string }) => aplicar(() => ligarFerramenta(store, { botId, conta: conta.id, ferramenta: visto.nome, digest: visto.digest, vinculo: visto.vinculo, apresentacao: visto.apresentacao, confirmo: true }))
  const desligar = (f: FerramentaDaConta) => aplicar(() => desligarFerramenta(store, botId, conta.id, f.nome))

  const titulo = <Text size="callout" weight="semibold" tone="text">{t('mercado.acoes.titulo', { conta: conta.nome })}</Text>
  if (falhou) return <VStack gap={6} className="mercado-acoes">{titulo}<Erro>{t('mercado.acoes.erro')}</Erro><div><Button kind="secondary" onClick={() => void ler()}>{t('mercado.catalogo.tentar')}</Button></div></VStack>
  if (!dados) return <VStack gap={6} className="mercado-acoes">{titulo}<Carregando texto={t('mercado.acoes.carregando')} linhas={1} /></VStack>
  const acoes = acoesQueAPessoaDecide(dados)
  return (
    <VStack gap={8} className="mercado-acoes">
      {titulo}
      {erro ? <Erro>{erro}</Erro> : null}
      {!dados.inventarioPronto ? <Aviso>{t('mercado.acoes.semLista')}</Aviso> : null}
      {dados.inventarioPronto && acoes.length === 0 ? <Text size="caption" tone="secondary">{t('mercado.acoes.vazio')}</Text> : null}
      {acoes.map((f) => {
        const nome = f.rotulo ?? f.nome
        const aberta = confirmando?.nome === f.nome
        return (
          <VStack key={f.nome} gap={6} className="mercado-acao">
            <HStack gap={10} shrink>
              <VStack gap={0} grow shrink>
                <Text size="body" tone="text" truncate>{nome}</Text>
                <Text size="caption" tone="secondary">{f.estado === 'ligada' ? t('mercado.acoes.estado.ligada') : t(`mercado.acoes.estado.${f.motivo ?? 'perigosa'}` as 'mercado.acoes.estado.perigosa')}</Text>
                {textoNaoLido(f) ? <Text size="caption" tone="secondary">{t('mercado.acoes.descricaoDesconhecida')}</Text> : null}
                {f.mudou?.length ? <Text size="caption" tone="secondary">{t('mercado.acoes.mudou', { partes: listaDeNomes(f.mudou.map((p) => t(`mercado.acoes.parte.${p}` as 'mercado.acoes.parte.descricao')), t('mercado.bot.e')) })}</Text> : null}
              </VStack>
              {f.estado === 'ligada'
                ? <Button kind="secondary" disabled={salvando} onClick={() => void desligar(f)}>{t('mercado.acoes.desligar')}</Button>
                : podeLigar(f) ? <Button kind="secondary" disabled={salvando || aberta} onClick={() => f.apresentacao && setConfirmando({ nome: f.nome, digest: f.digest, vinculo: dados.vinculo, apresentacao: f.apresentacao })}>{t('mercado.acoes.ligar')}</Button> : null}
            </HStack>
            {aberta ? (
              <div className="mercado-aviso alerta" role="alertdialog" aria-label={t('mercado.acoes.confirmar.titulo', { acao: nome, bot: botNome })}>
                <VStack gap={8}>
                  <Text size="callout" weight="semibold" as="div">{t('mercado.acoes.confirmar.titulo', { acao: nome, bot: botNome })}</Text>
                  <Text size="callout" as="div">{t('mercado.acoes.confirmar.texto', { conta: conta.nome })}</Text>
                  <Text size="caption" tone="secondary" as="div">{t('mercado.acoes.confirmar.descricao')}</Text>
                  <div className="mercado-acao-descricao"><Text size="caption" as="div">{f.descricao || t('mercado.acoes.confirmar.semDescricao')}</Text></div>
                  {f.campos?.length ? (
                    <>
                      <Text size="caption" tone="secondary" as="div">{t('mercado.acoes.confirmar.campos')}</Text>
                      <div className="mercado-acao-descricao">{f.campos.map(([campo, texto], i) => <Text key={i} size="caption" as="div">{texto ? t('mercado.acoes.confirmar.campo', { campo, texto }) : campo}</Text>)}</div>
                    </>
                  ) : null}
                  {temCaracteresOcultos([f.descricao ?? '', ...(f.campos ?? []).flat()].join('\n')) ? <Aviso>{t('mercado.acoes.ocultos')}</Aviso> : null}
                  <HStack gap={8}>
                    <Button disabled={salvando} onClick={() => confirmando && void ligar(confirmando)}>{t('mercado.acoes.confirmar.sim')}</Button>
                    <Button kind="secondary" disabled={salvando} onClick={() => setConfirmando(null)}>{t('mercado.acoes.confirmar.cancelar')}</Button>
                  </HStack>
                </VStack>
              </div>
            ) : null}
          </VStack>
        )
      })}
    </VStack>
  )
}
