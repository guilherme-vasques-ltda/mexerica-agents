import hashlib
import json
import os
import re
import stat
import sys
import unicodedata
from enum import Enum
from pathlib import Path
from urllib.parse import unquote, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

MAX_JSON = 4 * 1024 * 1024
MAX_PATCH = 512 * 1024
MAX_FILES = 100
MAX_FINDINGS = 20
MAX_BODY = 1500
MAX_ARTIFACT = 128 * 1024
TEXT = re.compile(r"[A-Za-z0-9À-ÖØ-öø-ÿ ,.!?;()'\-]+")
UNSAFE_TEXT = re.compile(
    r'[^\W_]\.[^\W_]|(?:https?|ftp|ssh|www|localhost|bearer|password|passwd|senha|api[ -]?key|private[ -]?key|'
    r'access[ -]?token|auth[ -]?token|token|secret|segredo|authorization)\b|'
    r'\b(?:sk|pk|rk|whsec|ghp|gho|glpat|xox[baprs]|AKIA|ASIA)[ -]?(?:[A-Za-z0-9]+-)*[A-Za-z0-9]{8,}|'
    r'[A-Za-z0-9]{20,}', re.I)
CATEGORIES = dict(bug='Bug', security='Segurança', performance='Desempenho',
                  maintainability='Manutenção', test='Teste', style='Estilo',
                  documentation='Documentação', other='Outro')
SEVERITIES = dict(critical='Crítica', high='Alta', medium='Média', low='Baixa')
SHA = re.compile(r'[0-9a-f]{40}')
PATH = re.compile(r'[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*')
HUNK = re.compile(r'@@ -(\d{1,8})(?:,(\d{1,8}))? \+(\d{1,8})(?:,(\d{1,8}))? @@(?: .*)?')
REVIEW_FIELDS = set('status llm trace_id message summary tool_calls comments groups warnings project_summary '
                    'resume session_id manifest retry_report'.split())
COMMENT_FIELDS = set('path content suggestion_code existing_code start_line end_line thinking category severity'.split())


class Refused(ValueError):
    def __init__(self, schema=None, warning_type=None):
        super().__init__()
        self.schema = schema
        self.warning_type = warning_type


class ProjectPhase(Enum):
    SNAPSHOT = 'Snapshot'
    ENVIRONMENT = 'Ambiente'
    READ = 'Leitura'
    JSON = 'Json'
    SCHEMA = 'Esquema'
    FINDING = 'Achado'
    RENDER = 'Render'
    WRITE = 'Gravacao'


class SchemaClass(Enum):
    STRUCTURE = 'Estrutura'
    COMMENTS = 'Comentarios'
    STATUS = 'Estado'
    SUMMARY = 'Resumo'
    WARNINGS_FORMAT = 'AvisosFormato'
    WARNINGS_COMPLETE = 'AvisosEmCompleto'
    WARNINGS_SKIPPED = 'AvisosEmIgnorado'
    WARNINGS_ITEM = 'AvisosItem'
    MANIFEST = 'Manifesto'
    COVERAGE = 'Cobertura'


class WarningType(Enum):
    MANIFEST_ERROR = 'ErroManifesto'
    TOKEN_BUDGET_REACHED = 'OrcamentoTokens'
    SUBTASK_ERROR = 'ErroSubtarefa'
    TOKEN_THRESHOLD_EXCEEDED = 'LimiarTokens'
    REVIEW_ROUND_FAILED = 'FalhaRodada'
    REVIEW_LIMIT_REACHED = 'LimiteRevisao'
    COMMENT_ARGS_REPAIRED = 'ArgumentosComentarioReparados'
    COMMENT_REFILED = 'ComentarioRealocado'
    OTHER = 'Outro'


WARNING_TYPES = {
    'manifest_error': WarningType.MANIFEST_ERROR,
    'token_budget_reached': WarningType.TOKEN_BUDGET_REACHED,
    'subtask_error': WarningType.SUBTASK_ERROR,
    'token_threshold_exceeded': WarningType.TOKEN_THRESHOLD_EXCEEDED,
    'review_round_failed': WarningType.REVIEW_ROUND_FAILED,
    'review_limit_reached': WarningType.REVIEW_LIMIT_REACHED,
    'comment_args_repaired': WarningType.COMMENT_ARGS_REPAIRED,
    'comment_refiled': WarningType.COMMENT_REFILED,
}


def refused_warning(schema, item):
    kind = item.get('type') if type(item) is dict else None
    return Refused(schema, WARNING_TYPES.get(kind, WarningType.OTHER) if type(kind) is str else WarningType.OTHER)


def require(condition, schema=None):
    if not condition:
        raise Refused(schema)


def unique_pairs(pairs):
    obj = dict(pairs)
    require(len(pairs) == len(obj))
    return obj


def parse(raw, maximum=MAX_JSON):
    require(0 < len(raw) <= maximum)
    try:
        return json.loads(raw.decode('utf-8'), object_pairs_hook=unique_pairs, parse_constant=lambda _: require(False))
    except (ValueError, RecursionError) as exc:
        raise Refused() from exc


def plain(text, secrets=()):
    if not isinstance(text, str) or len(text) > MAX_BODY or not TEXT.fullmatch(text):
        return False
    normalized = unicodedata.normalize('NFKC', text).casefold()
    if text != ' '.join(text.split()) or UNSAFE_TEXT.search(normalized):
        return False
    return not any(unicodedata.normalize('NFKC', value).casefold() in normalized
                   for value in secrets if value)


def contains_provider_value(finding, values):
    strings = [value for value in finding.values() if isinstance(value, str)]
    normalized = [unicodedata.normalize('NFKC', value).casefold()
                  for text in strings for value in (text, unquote(text))]
    return any(unicodedata.normalize('NFKC', secret).casefold() in value
               for secret in values if secret for value in normalized)


def review_status(report, findings):
    status, summary = report.get('status'), report.get('summary')
    require(status in ('complete', 'skipped', 'partial'), SchemaClass.STATUS)
    if summary is not None:
        require(type(summary) is dict and type(summary.get('comments')) is int
                and summary['comments'] == len(findings), SchemaClass.SUMMARY)
    warnings = report.get('warnings', [])
    require(type(warnings) is list, SchemaClass.WARNINGS_FORMAT)
    manifest = report.get('manifest')
    if manifest is not None:
        require(type(manifest) is dict and manifest.get('schema_version') == 'ocr.run-manifest/v1'
                and manifest.get('operation') == 'review' and manifest.get('terminal_state') == status
                and 'run_failure' not in manifest, SchemaClass.MANIFEST)
    if status != 'partial':
        if warnings:
            schema = SchemaClass.WARNINGS_COMPLETE if status == 'complete' else SchemaClass.WARNINGS_SKIPPED
            raise refused_warning(schema, warnings[0])
        require(summary is None or summary.get('budget_exceeded', False) is False, SchemaClass.SUMMARY)
        require(status != 'skipped' or not findings, SchemaClass.STATUS)
        if manifest is not None:
            require(type(manifest.get('coverage')) is dict
                    and manifest['coverage'].get('failed') in (None, []), SchemaClass.COVERAGE)
        return False
    require(summary is not None and summary.get('budget_exceeded') is True, SchemaClass.SUMMARY)
    require(manifest is not None, SchemaClass.MANIFEST)
    for item in warnings:
        if not (type(item) is dict and item.get('type') == 'token_budget_reached'):
            raise refused_warning(SchemaClass.WARNINGS_ITEM, item)
    coverage = manifest.get('coverage')
    require(type(coverage) is dict and type(coverage.get('completed')) is list and coverage['completed'],
            SchemaClass.COVERAGE)
    require(all(type(item) is dict and type(item.get('item_id')) is str and item['item_id']
                and 'classification' not in item for item in coverage['completed']), SchemaClass.COVERAGE)
    failed = coverage.get('failed')
    require(type(failed) is list and failed
            and all(type(item) is dict and item.get('classification') == 'budget' for item in failed), SchemaClass.COVERAGE)
    return True


def added_lines(patch, additions, deletions):
    require(isinstance(patch, str) and 0 < len(patch) <= MAX_PATCH)
    added = set()
    new = left = right = removed = 0
    seen = marker = False
    for row in patch.split('\n'):
        match = HUNK.fullmatch(row)
        if match:
            target = int(match[3])
            require(left == right == 0 and (not seen or target >= new))
            left, right = int(match[2] or '1'), int(match[4] or '1')
            require((int(match[1]) > 0 or left == 0) and (target > 0 or right == 0) and left + right > 0)
            new, seen, marker = target, True, False
            continue
        if row == r'\ No newline at end of file':
            require(marker)
            marker = False
            continue
        prefix = row[:1]
        require(seen and prefix in (' ', '+', '-'))
        if prefix != '+':
            require(left > 0)
            left -= 1
            removed += prefix == '-'
        if prefix != '-':
            require(right > 0)
            right -= 1
            if prefix == '+':
                added.add(new)
            new += 1
        marker = True
    require(seen and left == right == 0 and len(added) == additions and removed == deletions)
    return added


def indexed(files):
    require(type(files) is list and len(files) <= MAX_FILES)
    paths = {}
    for entry in files:
        require(type(entry) is dict)
        name, patch = entry.get('filename'), entry.get('patch')
        additions, deletions = entry.get('additions'), entry.get('deletions')
        require(isinstance(name, str) and 0 < len(name) <= 1024 and name not in paths)
        require(patch is None or isinstance(patch, str) and len(patch) <= MAX_PATCH)
        require(all(type(n) is int and 0 <= n <= 10_000_000 for n in (additions, deletions)))
        paths[name] = (patch, additions, deletions)
    entries = sorted(paths.items())
    digest = hashlib.sha256(json.dumps(entries, separators=(',', ':')).encode()).hexdigest()
    return entries, digest


def project(raw, files, secrets=(), *, set_phase=lambda _: None):
    set_phase(ProjectPhase.JSON)
    report = parse(raw)
    set_phase(ProjectPhase.SCHEMA)
    require(type(report) is dict and set(report) <= REVIEW_FIELDS, SchemaClass.STRUCTURE)
    findings = report.get('comments')
    if findings is None and 'comments' in report:
        findings = []
    require(type(findings) is list and len(findings) <= 500, SchemaClass.COMMENTS)
    partial = review_status(report, findings)
    set_phase(ProjectPhase.FINDING)
    entries, digest = indexed(files)
    indices = {path: i for i, (path, _) in enumerate(entries)}
    items = []
    for finding in findings:
        require(type(finding) is dict and set(finding) <= COMMENT_FIELDS)
        path, category, severity = (finding.get(key) for key in ('path', 'category', 'severity'))
        require(isinstance(path, str) and path in indices)
        require(isinstance(category, str) and category in CATEGORIES)
        require(isinstance(severity, str) and severity in SEVERITIES)
        text = finding.get('content')
        require(isinstance(text, str))
        require(all(isinstance(finding.get(k, ''), str) for k in ('thinking', 'existing_code', 'suggestion_code')))
        if not plain(text, secrets) or len(text) > MAX_BODY - 100 or contains_provider_value(finding, secrets):
            continue
        items.append([indices[path], finding.get('start_line'), finding.get('end_line'),
                      list(CATEGORIES).index(category), list(SEVERITIES).index(severity), text])
    payload = {'diff': digest, 'items': items[:MAX_FINDINGS], 'partial': partial}
    set_phase(ProjectPhase.RENDER)
    render(payload, files, '0' * 40)
    return payload


def check_payload(payload):
    require(type(payload) is dict and set(payload) == {'diff', 'items', 'partial'})
    require(isinstance(payload['diff'], str) and re.fullmatch(r'[0-9a-f]{64}', payload['diff']))
    require(type(payload['partial']) is bool and type(payload['items']) is list and len(payload['items']) <= MAX_FINDINGS)
    for item in payload['items']:
        require(type(item) is list and len(item) == 6 and all(type(v) is int for v in item[:5]))
        index, start, end, category, severity, text = item
        require(0 <= index < MAX_FILES and 1 <= start <= end <= 10_000_000 and end - start < 30)
        require(0 <= category < len(CATEGORIES) and 0 <= severity < len(SEVERITIES))
        require(plain(text))


def render(payload, files, head):
    check_payload(payload)
    entries, digest = indexed(files)
    require(payload['diff'] == digest and SHA.fullmatch(head))
    comments, seen, positions = [], set(), {}
    for index, start, end, category, severity, text in payload['items']:
        require(index < len(entries))
        path, patch_data = entries[index]
        require(len(path) <= 240 and PATH.fullmatch(path) and not {'.', '..'}.intersection(path.split('/')))
        if index not in positions:
            positions[index] = added_lines(*patch_data)
        require(all(line in positions[index] for line in range(start, end + 1)))
        require((index, end) not in seen)
        seen.add((index, end))
        label, level = list(CATEGORIES.values())[category], list(SEVERITIES.values())[severity]
        body = f'Categoria: {label}. Severidade: {level}. Achado: {text}'
        require(len(body) <= MAX_BODY)
        comment = {'path': path, 'line': end, 'side': 'RIGHT', 'body': body}
        if start < end:
            comment.update(start_line=start, start_side='RIGHT')
        comments.append(comment)
    summary = f'Open Code Review: {len(comments)} sinalização(ões) para verificação manual. Não é uma aprovação.'
    if payload['partial']:
        summary += ' Revisão parcial: limite de orçamento atingido.'
    return {'commit_id': head, 'event': 'COMMENT', 'comments': comments, 'body': summary}


def snapshot(request, repo, number, base, head):
    require(re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo) and re.fullmatch(r'[1-9][0-9]{0,8}', str(number)))
    require(SHA.fullmatch(base) and SHA.fullmatch(head))
    path = f'/repos/{repo}/pulls/{number}'

    def check():
        pr = request(path)
        require(pr['number'] == number and pr['state'] == 'open' and pr['draft'] is False)
        require(pr['head']['repo']['full_name'] == repo and pr['base']['repo']['full_name'] == repo)
        require(pr['head']['sha'] == head and pr['base']['sha'] == base and pr['base']['ref'] == 'main')
        count = pr['changed_files']
        require(type(count) is int and 0 <= count <= MAX_FILES)
        return count

    count = check()
    files = request(f'{path}/files?per_page={MAX_FILES}')
    require(type(files) is list and len(files) == count and count == check())
    return path, files


def publish(request, repo, number, base, head, payload):
    path, files = snapshot(request, repo, number, base, head)
    body = render(payload, files, head)
    if body['comments'] or payload['partial']:
        request(f'{path}/reviews', body)
    return len(body['comments'])


def publish_artifact(request, repo, number, base, head, artifact):
    try:
        require(stat.S_ISREG(artifact.lstat().st_mode))
        with artifact.open('rb') as source:
            payload = parse(source.read(MAX_ARTIFACT + 1), MAX_ARTIFACT)
    except OSError as exc:
        raise Refused() from exc
    check_payload(payload)
    return publish(request, repo, number, base, head, payload)


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def main(mode, set_phase):
    if mode == 'validate-provider':
        model = os.environ['OCR_LLM_MODEL']
        require(bool(model) and model == model.strip())
        raw = os.environ['OCR_LLM_URL']
        require(raw.isascii() and re.fullmatch(r'https://[^\s\\]+', raw))
        url = urlsplit(raw)
        require(url.hostname and not url.username and url.password is None and not url.fragment and not url.query)
        require(url.port is None or 0 < url.port <= 65535)
        return
    if mode == 'project':
        set_phase(ProjectPhase.SNAPSHOT)
    token = os.environ['GH_TOKEN']
    require(bool(token))
    opener = build_opener(NoRedirect())

    def request(path, body=None):
        req = Request('https://api.github.com' + path,
                      data=None if body is None else json.dumps(body).encode(),
                      headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
                               'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': 'application/json'})
        with opener.open(req, timeout=12) as response:
            if body is None:
                return parse(response.read(MAX_JSON + 1))

    args = (os.environ['GITHUB_REPOSITORY'], int(os.environ['PR_NUMBER']),
            os.environ['BASE_SHA'], os.environ['HEAD_SHA'])
    if mode == 'preflight':
        snapshot(request, *args)
    elif mode == 'project':
        _, files = snapshot(request, *args)
        set_phase(ProjectPhase.ENVIRONMENT)
        secrets = (os.environ['OCR_LLM_TOKEN'], os.environ['OCR_LLM_URL'], os.environ['OCR_LLM_MODEL'])
        require(all(secrets))
        set_phase(ProjectPhase.READ)
        work = Path(os.environ['RUNNER_TEMP']) / 'mexerica-ocr'
        with (work / 'result.json').open('rb') as raw:
            data = raw.read(MAX_JSON + 1)
        payload = project(data, files, secrets, set_phase=set_phase)
        set_phase(ProjectPhase.WRITE)
        projected = json.dumps(payload, ensure_ascii=False, separators=(',', ':')).encode('utf-8')
        require(len(projected) <= MAX_ARTIFACT)
        artifact = work / 'review.json'
        try:
            artifact.write_bytes(projected)
        except OSError:
            try:
                artifact.unlink(missing_ok=True)
            except OSError:
                pass
            raise
    elif mode == 'publish':
        artifact = Path(os.environ['RUNNER_TEMP']) / 'mexerica-ocr-publish/review.json'
        count = publish_artifact(request, *args, artifact)
        print(f'Open Code Review: {count} sinalização(ões) publicada(s).')
    else:
        raise Refused()


def cli():
    phase = None

    def set_phase(value):
        nonlocal phase
        phase = value

    try:
        main(sys.argv[1], set_phase)
    except Exception as exc:
        message = 'Open Code Review: operação recusada ou indisponível.'
        if phase is not None:
            message += f' Fase: {phase.value}.'
        if phase is ProjectPhase.SCHEMA and isinstance(exc, Refused) and isinstance(exc.schema, SchemaClass):
            message += f' Classe: {exc.schema.value}.'
            if isinstance(exc.warning_type, WarningType):
                message += f' Tipo: {exc.warning_type.value}.'
        print(message, file=sys.stderr)
        sys.exit(1)


if __name__ == '__main__':
    cli()
