// Ferramentas de auditoria consultam lockfiles; não instalam nem atualizam pacotes.
import { spawnSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
const rows = [];
for (const dir of ['apps/desktop', 'motor']) {
  const result = spawnSync('npm', ['audit', '--audit-level=high', '--json'], { cwd: dir, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  try {
    const data = JSON.parse(result.stdout);
    if (![0, 1].includes(result.status) || data.error || !data.metadata?.vulnerabilities) throw new Error();
    const counts = data.metadata.vulnerabilities;
    rows.push(`${dir}: ${counts.high} alta(s), ${counts.critical} crítica(s), ${counts.total} total.`);
    for (const [name, v] of Object.entries(data.vulnerabilities)) {
      if (['high', 'critical'].includes(v.severity)) rows.push(`- ${name}: ${v.severity}; correção disponível: ${JSON.stringify(v.fixAvailable)}.`);
    }
  } catch { rows.push(`${dir}: ERRO de auditoria; resultado indisponível.`); }
}
for (const file of ['host/Cargo.lock']) {
  const result = spawnSync(process.env.CARGO_AUDIT_BIN || 'cargo-audit', ['audit', '--file', file, '--json'], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 });
  try {
    const data = JSON.parse(result.stdout);
    if (![0, 1].includes(result.status) || !data.vulnerabilities) throw new Error();
    rows.push(`${file}: ${data.vulnerabilities.count} vulnerabilidade(s).`);
    for (const v of data.vulnerabilities.list) rows.push(`- ${v.package.name} ${v.package.version}: ${v.advisory.id}; corrigido: ${v.versions.patched.join(' ou ') || 'não informado'}.`);
    for (const [kind, warnings] of Object.entries(data.warnings || {})) rows.push(`- Avisos ${kind}: ${warnings.length}.`);
  } catch { rows.push(`${file}: ERRO de auditoria; resultado indisponível.`); }
}
console.log(rows.join('\n'));
console.log('::warning::Auditorias de dependências são informativas nesta fase; consulte o resumo do job.');
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `## Dependências — aviso\n\n${rows.join('\n')}\n`);
