// Ações perigosas (fatia 2): as ferramentas de UMA conta que o Hub deixa desligadas num bot (escrevem, enviam, publicam,
// apagam ou mexem em dinheiro, ou são novas ou mudaram). Só a pessoa liga, com confirmação; o Hub confere de novo.
import { useCallback, useEffect, useState } from 'react'
import { Button } from '../../../components/Controls'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { desligarFerramenta, ferramentasDoBot, ligarFerramenta } from './estado'
import { acoesQueAPessoaDecide } from './regras.ts'
import type { FerramentaDaConta, FerramentasDaConta } from './tipos.ts'
import { Aviso, Carregando, Erro, frasesDaRecusa } from './pecas'

export function AcoesDaConta({ botId, botNome, conta }: { botId: string; botNome: string; conta: { id: string; nome: string } }) {
  const store = useStore()
  const t = useT()
  const [dados, setDados] = useState<FerramentasDaConta | null>(null)
  const [falhou, setFalhou] = useState(false)
  const [erro, setErro] = useState<string | null>(null)
  const [confirmando, setConfirmando] = useState<string | null>(null)
  const [salvando, setSalvando] = useState(false)

  const ler = useCallback(async () => {
    setFalhou(false)
    try { setDados(await ferramentasDoBot(store, botId, conta.id)) } catch { setFalhou(true) }
  }, [store, botId, conta.id])
  useEffect(() => { setDados(null); setConfirmando(null); setErro(null); void ler() }, [ler])

  const aplicar = async (op: () => Promise<FerramentasDaConta>) => {
    setSalvando(true); setErro(null)
    try { setDados(await op()); setConfirmando(null) } catch (e) { setErro(frasesDaRecusa(t, e)); await ler() } finally { setSalvando(false) }
  }
  const ligar = (f: FerramentaDaConta) => aplicar(() => ligarFerramenta(store, { botId, conta: conta.id, ferramenta: f.nome, digest: f.digest, vinculo: dados?.vinculo ?? -1, confirmo: true }))
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
        const aberta = confirmando === f.nome
        return (
          <VStack key={f.nome} gap={6} className="mercado-acao">
            <HStack gap={10} shrink>
              <VStack gap={0} grow shrink>
                <Text size="body" tone="text" truncate>{nome}</Text>
                <Text size="caption" tone="secondary">{f.estado === 'ligada' ? t('mercado.acoes.estado.ligada') : t(`mercado.acoes.estado.${f.motivo ?? 'perigosa'}` as 'mercado.acoes.estado.perigosa')}</Text>
              </VStack>
              {f.estado === 'ligada'
                ? <Button kind="secondary" disabled={salvando} onClick={() => void desligar(f)}>{t('mercado.acoes.desligar')}</Button>
                : <Button kind="secondary" disabled={salvando || aberta} onClick={() => setConfirmando(f.nome)}>{t('mercado.acoes.ligar')}</Button>}
            </HStack>
            {aberta ? (
              <div className="mercado-aviso alerta" role="alertdialog" aria-label={t('mercado.acoes.confirmar.titulo', { acao: nome, bot: botNome })}>
                <VStack gap={8}>
                  <Text size="callout" weight="semibold" as="div">{t('mercado.acoes.confirmar.titulo', { acao: nome, bot: botNome })}</Text>
                  <Text size="callout" as="div">{t('mercado.acoes.confirmar.texto', { conta: conta.nome })}</Text>
                  <HStack gap={8}>
                    <Button disabled={salvando} onClick={() => void ligar(f)}>{t('mercado.acoes.confirmar.sim')}</Button>
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
