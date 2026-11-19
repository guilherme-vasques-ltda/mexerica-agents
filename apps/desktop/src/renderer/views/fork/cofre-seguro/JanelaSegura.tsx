import { useEffect, useRef, useState, type FormEvent } from 'react'
import { CODIGO_CAIXAS, CODIGO_MAX, codigoValido, soDigitos, type DadosJanela, type ErroSalvar } from '@shared/fork-cofre-seguro'
import { Button } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { HStack, VStack } from '../../../components/Stack'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { Olho } from './olho'
import { chaveDoErro } from './erros'
import './cofre-seguro.css'

// Exemplo no campo do site: dado técnico, igual em todo idioma e da esquerda para a direita.
const EXEMPLO_SITE = 'github.com' // textos-ok: exemplo de domínio, igual em todo idioma

/** Busca o que mostrar com o main (só texto do pedido) e desenha o cartão; sem dados, avisa em vez de abrir um campo. */
export function JanelaSegura() {
  const t = useT()
  const [dados, setDados] = useState<DadosJanela | null | undefined>(undefined)
  useEffect(() => { void window.mxCofre.dados().then(setDados, () => setDados(null)) }, [])
  if (dados === undefined) return null
  if (!dados) return <main className="cofre-janela"><Text size="body" tone="danger">{t('extensoes.cofre.erro.dados')}</Text></main>
  return <CartaoSeguro dados={dados} />
}

/**
 * MX-209: o cartão seguro (tela 2). Vive só na BrowserWindow própria, com o preload estreito (`window.mxCofre`):
 * aqui o valor existe num `<input type=password>` e sai por um IPC só, direto ao processo principal.
 */
export function CartaoSeguro({ dados }: { dados: DadosJanela }) {
  const t = useT()
  const idioma = useIdioma()
  const [valor, setValor] = useState('')
  const [site, setSite] = useState('')
  const [codigo, setCodigo] = useState('')
  const [usuario, setUsuario] = useState('')
  const [rotulo, setRotulo] = useState('')
  const [visivel, setVisivel] = useState(false)
  const [salvando, setSalvando] = useState(false)
  const [erro, setErro] = useState<ErroSalvar | null>(null)
  const olho = useRef<Olho | null>(null)
  const grupo = useRef<HTMLSpanElement>(null)
  olho.current ??= new Olho({ aoMudar: setVisivel })

  useEffect(() => {
    // O valor nunca fica à vista com a janela fora de foco nem ao fechar (MX-154).
    const esconder = () => olho.current?.esconder()
    window.addEventListener('blur', esconder)
    window.addEventListener('pagehide', esconder)
    return () => {
      window.removeEventListener('blur', esconder)
      window.removeEventListener('pagehide', esconder)
      esconder()
    }
  }, [])

  const { pedido } = dados
  // MX-277: `loginNovo` é salvar um login sem pedido do bot (vindo do aviso de senha colada): a janela abre VAZIA, com o
  // campo do site, e a pessoa cola a senha aqui; o trecho escondido nunca veio do chat.
  const novo = pedido.tipo === 'loginNovo'
  const login = pedido.tipo === 'login' || novo
  // MX-286: `code` é o código de login (2º fator): 6 caixinhas de número, sem olho (é um número curto, que a pessoa precisa
  // conferir enquanto digita), sem usuário, rótulo nem "ligada a". O Hub guarda só na memória, uma vez, por 5 minutos.
  const ehCodigo = pedido.tipo === 'code'

  const salvar = async (e?: FormEvent) => {
    e?.preventDefault()
    if (salvando) return
    if (ehCodigo ? !codigoValido(codigo) : !valor || (novo && !site.trim())) return setErro('vazio')
    olho.current?.esconder()
    setSalvando(true)
    setErro(null)
    const resposta = await window.mxCofre.salvar(ehCodigo ? { valor: codigo, usuario: '', rotulo: '' } : { valor, usuario, rotulo, ...(novo ? { site } : {}) }).catch(() => ({ ok: false as const, erro: 'conexao' as const }))
    // `ok`: o main fecha esta janela; o cartão da conversa muda pela entrada do Hub.
    if (resposta.ok) return ehCodigo ? setCodigo('') : setValor('')
    setSalvando(false)
    setErro(resposta.erro)
  }

  return (
    <main className="cofre-janela idioma-superficie" {...superficieIdioma(idioma)}>
      <HStack gap={12} justify="space-between">
        <span className="cofre-janela-cadeado"><Icon name="lock" size={20} /></span>
        <span className="cofre-janela-selo"><Icon name="checkmark.shield" size={12} /> {t('extensoes.cofre.janelaTitulo')}</span>
      </HStack>
      <VStack gap={4}>
        <Text as="h2" size="title2" weight="semibold" dir="auto">
          {ehCodigo ? t('extensoes.cofre.janelaTituloCodigo', { site: pedido.site ?? pedido.titulo }) : novo ? t('extensoes.cofre.janelaTituloNovo') : login ? t('extensoes.cofre.janelaTituloLogin', { site: pedido.site ?? pedido.titulo }) : t('extensoes.cofre.janelaTituloChave', { conector: pedido.titulo })}
        </Text>
        <Text size="callout" tone="secondary" dir="auto">
          {ehCodigo
            ? t('extensoes.cofre.janelaPediuCodigo', { bot: pedido.botNome, site: pedido.site ?? pedido.titulo })
            : novo
            ? t('extensoes.cofre.janelaColeAqui')
            : login
            ? t('extensoes.cofre.janelaPediuLogin', { bot: pedido.botNome, site: pedido.site ?? pedido.titulo })
            : t('extensoes.cofre.janelaPediuChave', { bot: pedido.botNome, campo: pedido.campo ?? '', conector: pedido.titulo })}
        </Text>
      </VStack>
      <form onSubmit={(e) => void salvar(e)} className="cofre-form" autoComplete="off">
        {ehCodigo ? (
          <VStack gap={12}>
            <label>
              {t('extensoes.cofre.rotuloCodigo')}
              <CaixasDeCodigo valor={codigo} aoMudar={setCodigo} rotulo={t('extensoes.cofre.rotuloCodigo')} digito={(n, total) => t('extensoes.cofre.codigoDigito', { n, total })} />
            </label>
          </VStack>
        ) : (
        <VStack gap={12}>
          {novo ? (
            <label>
              {t('extensoes.cofre.rotuloSite')}
              <span className="cofre-campo">
                <input value={site} onChange={(e) => setSite(e.target.value)} dir="ltr" autoComplete="off" spellCheck={false} autoCapitalize="off" autoFocus placeholder={EXEMPLO_SITE} />
              </span>
            </label>
          ) : null}
          {login ? (
            <label>
              {t('extensoes.cofre.rotuloUsuario')}
              <span className="cofre-campo">
                <input value={usuario} onChange={(e) => setUsuario(e.target.value)} dir="ltr" autoComplete="off" spellCheck={false} autoCapitalize="off" placeholder={t('extensoes.cred.usuario')} />
              </span>
            </label>
          ) : null}
          <label>
            {t(login ? 'extensoes.cofre.rotuloSenha' : 'extensoes.cofre.rotuloChave')}
            <span className="cofre-campo" ref={grupo}
              onBlur={(e) => { if (!grupo.current?.contains(e.relatedTarget as Node | null)) olho.current?.esconder() }}>
              {/* Sempre um <input>: password enquanto escondido; o olho troca para texto por no máximo 10 s. */}
              <input type={visivel ? 'text' : 'password'} value={valor} onChange={(e) => setValor(e.target.value)} dir="ltr" autoFocus={!novo}
                autoComplete="new-password" spellCheck={false} autoCapitalize="off" autoCorrect="off" data-1p-ignore />
              <button type="button" className="cofre-olho" aria-pressed={visivel}
                onMouseDown={(e) => e.preventDefault()} onClick={() => olho.current?.alternar()}>
                <Icon name={visivel ? 'eye.slash' : 'eye'} size={14} /> {t(visivel ? 'extensoes.cofre.esconder' : 'extensoes.cofre.mostrar')}
              </button>
            </span>
          </label>
          {login ? (
            <label>
              {t('extensoes.cofre.rotuloNome')}
              <span className="cofre-campo">
                <input value={rotulo} onChange={(e) => setRotulo(e.target.value)} dir="auto" autoComplete="off" spellCheck={false} placeholder={t('extensoes.pedido.rotuloExemplo')} />
              </span>
            </label>
          ) : null}
        </VStack>
        )}
      </form>
      <HStack gap={8} className="cofre-caixa" align="baseline">
        <Icon name="checkmark.shield" size={14} className="icone-ok" />
        {ehCodigo
          ? <Text size="callout"><b>{t('extensoes.cofre.codigoNota')}</b> {t('extensoes.cofre.codigoNaoGuardado')}</Text>
          : <Text size="callout"><b>{t('extensoes.cofre.caixaNunca')}</b> {t('extensoes.cofre.caixaVaiCofre')}</Text>}
      </HStack>
      {novo || ehCodigo ? null : <HStack gap={8}>
        <Text size="callout" tone="secondary">{t('extensoes.cofre.ligadaA')}</Text>
        <Text size="callout" dir="auto">
          {login && usuario.trim()
            ? t('extensoes.cofre.ligadaConta', { bot: pedido.botNome, conta: usuario.trim() })
            : t('extensoes.cofre.ligadaBot', { bot: pedido.botNome })}
        </Text>
      </HStack>}
      {/* Login novo não tem pedido do bot a pedir de novo: a recusa tem texto próprio. */}
      {erro ? <Text size="footnote" tone="danger" dir="auto">{t(chaveDoErro(erro, novo))}</Text> : null}
      <HStack gap={8} justify="end" className="cofre-janela-acoes">
        <Button kind="secondary" disabled={salvando} onClick={() => window.mxCofre.cancelar()}>{t('extensoes.cofre.cancelar')}</Button>
        <Button disabled={salvando || (ehCodigo ? !codigoValido(codigo) : !valor || (novo && !site.trim()))} onClick={() => void salvar()}>{t(salvando ? (ehCodigo ? 'extensoes.cofre.usandoCodigo' : 'extensoes.cofre.salvando') : ehCodigo ? 'extensoes.cofre.usarCodigo' : 'extensoes.cofre.salvar')}</Button>
      </HStack>
      <Text size="caption" tone="secondary" className="cofre-janela-rodape">{t(ehCodigo ? 'extensoes.cofre.rodapeJanelaCodigo' : novo ? 'extensoes.cofre.rodapeJanelaNovo' : login ? 'extensoes.cofre.rodapeJanelaSenha' : 'extensoes.cofre.rodapeJanelaChave')}</Text>
    </main>
  )
}

/**
 * As caixinhas do código (MX-286). Há UM só `<input>` de verdade, por cima das caixas (transparente): digitar, apagar, setas e
 * colar são do navegador. Só dígitos entram (colar "123 456" vira 123456), no máximo 10 como o Hub; o código inteiro colado
 * preenche as caixas e elas crescem de 6 até o tamanho do código. As caixas só desenham o que está no campo.
 */
export function CaixasDeCodigo({ valor, aoMudar, rotulo, digito }: { valor: string; aoMudar: (v: string) => void; rotulo: string; digito: (n: number, total: number) => string }) {
  const total = Math.max(CODIGO_CAIXAS, valor.length)
  return (
    <span className="cofre-digitos" dir="ltr">
      <input
        className="cofre-digitos-entrada" value={valor} inputMode="numeric" pattern="[0-9]*" autoComplete="one-time-code" aria-label={rotulo}
        spellCheck={false} autoCorrect="off" autoFocus data-1p-ignore
        onChange={(e) => aoMudar(soDigitos(e.target.value))}
      />
      <span className="cofre-digitos-caixas" aria-hidden="true">
        {Array.from({ length: total }, (_, i) => (
          <span key={i} className={`cofre-digito${i < valor.length ? ' cheia' : ''}${i === valor.length && valor.length < CODIGO_MAX ? ' atual' : ''}`} title={digito(i + 1, total)}>{valor[i] ?? ''}</span>
        ))}
      </span>
    </span>
  )
}
