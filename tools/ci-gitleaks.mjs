// Só imprime localização/regra/commit; nunca imprime a saída bruta do scanner.
import { readFileSync, writeFileSync, mkdtempSync, rmSync, appendFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
const sha = (value) => {
  if (!/^[a-f0-9]{40}$/.test(value ?? '') || /^0+$/.test(value)) throw new Error('SHA ausente/inválido; varredura recusada');
  return value;
};
const git = (...args) => {
  const result = spawnSync('git', args, { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  if (result.status !== 0) throw new Error('Contexto Git indisponível');
  return result.stdout.trim();
};
let base, head;
switch (process.env.GITHUB_EVENT_NAME) {
  case 'pull_request':
    head = sha(event.pull_request.head.sha);
    base = git('merge-base', sha(event.pull_request.base.sha), head);
    break;
  case 'push':
    base = sha(event.before);
    head = sha(event.after);
    break;
  case 'merge_group':
    base = sha(event.merge_group.base_sha);
    head = sha(event.merge_group.head_sha);
    break;
  case 'workflow_dispatch':
    head = sha(process.env.GITHUB_SHA);
    base = sha(git('rev-parse', `${head}^`));
    break;
  default: throw new Error('Evento não suportado');
}
// remerge pula merges octopus (3+ pais) sem erro; falha fechada antes de varrer.
if (git('rev-list', '--min-parents=3', `${base}..${head}`) !== '') {
  throw new Error('merge octopus não suportado na varredura de segredos; refaça como merges de dois pais');
}
const dir = mkdtempSync(join(tmpdir(), 'ci-gitleaks-'));
try {
  // Política da main aprovada, separada do intervalo de commits analisado.
  const trustedMain = sha(process.env.GITLEAKS_TRUSTED_MAIN_SHA);
  if (trustedMain !== git('rev-parse', 'refs/remotes/origin/main')) throw new Error('Main confiável diverge do checkout');
  const config = spawnSync('git', ['show', `${trustedMain}:.gitleaks.toml`], { encoding: 'utf8' });
  if (config.status !== 0) throw new Error('Política obrigatória indisponível na main');
  writeFileSync(join(dir, 'config.toml'), config.stdout);
  // A baseline é opcional: só a main confiável pode criá-la. Sem ela, nada é suprimido.
  const baselinePath = '.github/security/gitleaks-baseline.fingerprints';
  const hasBaseline = spawnSync('git', ['cat-file', '-e', `${trustedMain}:${baselinePath}`], { stdio: 'ignore' }).status === 0;
  const baseline = hasBaseline ? spawnSync('git', ['show', `${trustedMain}:${baselinePath}`], { encoding: 'utf8' }) : { status: 0, stdout: '' };
  if (baseline.status !== 0) throw new Error('Baseline da main ilegível');
  writeFileSync(join(dir, 'baseline'), baseline.stdout);
  // Um clone bare isolado não contém .gitleaksignore nem arquivos do PR.
  // O Gitleaks também busca source/.gitleaksignore, mesmo com -i explícito.
  const source = join(dir, 'repository.git');
  const clone = spawnSync('git', ['clone', '--bare', '--shared', '--no-hardlinks', '--', process.cwd(), source], { encoding: 'utf8' });
  if (clone.status !== 0) throw new Error('Não foi possível isolar a origem Git');
  const report = join(dir, 'report.json');
  // remerge: o merge mostra só o que a resolução manual mudou em relação ao merge
  // automático (evil merge continua coberto), sem reapresentar a main como novidade.
  // Os commits próprios do intervalo continuam varridos um a um. Requer git >= 2.36.
  const result = spawnSync(process.env.GITLEAKS_BIN || 'gitleaks', [
    'git', '--redact=100', '--no-banner', '--ignore-gitleaks-allow',
    '--config', join(dir, 'config.toml'), '--gitleaks-ignore-path', join(dir, 'baseline'),
    '--log-opts', `--diff-merges=remerge ${base}..${head}`, '--report-format', 'json', '--report-path', report, source,
  ], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  if (![0, 1].includes(result.status)) throw new Error('Gitleaks não concluiu a análise');
  const findings = JSON.parse(readFileSync(report, 'utf8'));
  for (const finding of findings) console.log(JSON.stringify({ arquivo: finding.File, linha: finding.StartLine, regra: finding.RuleID, commit: finding.Commit }));
  const summary = `Gitleaks: ${findings.length} ocorrência(s) nova(s). Intervalo ${base}..${head}.\n`;
  console.log(summary);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
  process.exitCode = findings.length === 0 && result.status === 0 ? 0 : 1;
} finally {
  rmSync(dir, { recursive: true, force: true });
}
